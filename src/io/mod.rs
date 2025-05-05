use std::{fs::File, io::Write, path::PathBuf};

use async_trait::async_trait;
use smol_str::SmolStr;

use crate::HttpClient;

pub mod blocking;

#[derive(Clone, Copy)]
pub enum ArchiveType {
    Zip,
    TarGz,
    TarXz,
}

impl ArchiveType {
    pub(crate) fn from_path(path: &[u8]) -> anyhow::Result<ArchiveType> {
        if path.ends_with(b".zip") {
            Ok(ArchiveType::Zip)
        } else if path.ends_with(b".tar.gz") {
            Ok(ArchiveType::TarGz)
        } else if path.ends_with(b".tar.xz") {
            Ok(ArchiveType::TarXz)
        } else {
            Err(anyhow::anyhow!(
                "unknown archive type from {}",
                String::from_utf8_lossy(path)
            ))
        }
    }
}

pub enum VerifyMethod {
    None,
    Sha1(SmolStr),
}

pub struct ArchiveExtractInfo {
    pub archive_path: PathBuf,
    pub archive_type: ArchiveType,
    pub extracted_dir: PathBuf,
}

#[async_trait]
pub trait DownloadExtractCallback {
    async fn on_downloaded(&mut self, info: &ArchiveExtractInfo) -> anyhow::Result<()>;
    async fn on_extracted(&mut self, info: &ArchiveExtractInfo) -> anyhow::Result<()>;
}

struct DownloadingState {
    response: reqwest::Response,
    archive_file: File,
    total_size: Option<u64>,
    downloaded_size: u64,
}

enum DownloadExtractStateInner {
    Downloading(
        blocking::TmpDir,
        ArchiveExtractInfo,
        DownloadingState,
        Box<dyn DownloadExtractCallback + Send>,
    ),
    Extracting(
        blocking::TmpDir,
        ArchiveExtractInfo,
        Box<dyn DownloadExtractCallback + Send>,
    ),
    Stopped,
}

pub struct DownloadExtractState(DownloadExtractStateInner);
impl DownloadExtractState {
    pub async fn start(
        client: &HttpClient,
        url: &str,
        tmp_dir: PathBuf,
        custom_action: Box<dyn DownloadExtractCallback + Send>,
    ) -> anyhow::Result<Self> {
        let response = client.get(url).send().await?;
        if !response.status().is_success() {
            anyhow::bail!(
                "Failed to download '{}': {}\n{}",
                url,
                response.status(),
                response.text().await?
            );
        }

        let archive_type = ArchiveType::from_path(url.as_bytes())?;
        let (archive_file, archive_path, mut tmp_dir) = crate::spawn_blocking(move || {
            std::fs::create_dir_all(&tmp_dir)?;
            let tmp_dir = blocking::TmpDir {
                path: tmp_dir,
                should_not_block: false,
            };
            let archive_path = tmp_dir.path.join("download");
            let archive_file = std::fs::File::create(&archive_path)?;
            Ok((archive_file, archive_path, tmp_dir))
        })
        .await?;

        tmp_dir.should_not_block = true;
        let extracted_dir = tmp_dir.path.join("extracted");

        let total_size = response.content_length();
        Ok(DownloadExtractState(
            DownloadExtractStateInner::Downloading(
                tmp_dir,
                ArchiveExtractInfo {
                    archive_path,
                    archive_type,
                    extracted_dir,
                },
                DownloadingState {
                    response,
                    archive_file,
                    total_size,
                    downloaded_size: 0,
                },
                custom_action,
            ),
        ))
    }

    pub fn status(&self) -> crate::Status {
        match &self.0 {
            DownloadExtractStateInner::Downloading(
                _,
                _,
                DownloadingState {
                    total_size,
                    downloaded_size,
                    ..
                },
                _,
            ) => crate::Status::InProgress {
                name: "Downloading".into(),
                progress_ratio: total_size.map(|total| (*downloaded_size, total)),
            },
            DownloadExtractStateInner::Extracting(_, _, _) => crate::Status::InProgress {
                name: "Extracting".into(),
                progress_ratio: None,
            },
            DownloadExtractStateInner::Stopped => crate::Status::Stopped,
        }
    }

    async fn do_advance(
        self,
        abandoned_tmp_dir: &mut Option<blocking::TmpDir>,
    ) -> anyhow::Result<Self> {
        match self.0 {
            DownloadExtractStateInner::Downloading(
                tmp_dir,
                archive_extract_info,
                DownloadingState {
                    mut response,
                    mut archive_file,
                    downloaded_size,
                    total_size,
                },
                mut custom_action,
            ) => {
                *abandoned_tmp_dir = Some(tmp_dir);
                Ok(DownloadExtractState(
                    if let Some(chunk) = response.chunk().await? {
                        archive_file.write_all(&chunk)?;
                        DownloadExtractStateInner::Downloading(
                            abandoned_tmp_dir.take().unwrap(),
                            archive_extract_info,
                            DownloadingState {
                                response,
                                archive_file,
                                downloaded_size: downloaded_size + chunk.len() as u64,
                                total_size,
                            },
                            custom_action,
                        )
                    } else {
                        custom_action.on_downloaded(&archive_extract_info).await?;
                        DownloadExtractStateInner::Extracting(
                            abandoned_tmp_dir.take().unwrap(),
                            archive_extract_info,
                            custom_action,
                        )
                    },
                ))
            }
            DownloadExtractStateInner::Extracting(
                tmp_dir,
                mut archive_extract_info,
                mut custom_action,
            ) => {
                *abandoned_tmp_dir = Some(tmp_dir);
                archive_extract_info = crate::spawn_blocking(move || {
                    blocking::extract_archive(
                        archive_extract_info.archive_type,
                        &archive_extract_info.archive_path,
                        &archive_extract_info.extracted_dir,
                    )?;
                    Ok(archive_extract_info)
                })
                .await?;
                custom_action.on_extracted(&archive_extract_info).await?;
                abandoned_tmp_dir.as_mut().unwrap().should_not_block = false;
                Ok(DownloadExtractState(DownloadExtractStateInner::Stopped))
            }
            DownloadExtractStateInner::Stopped => Err(anyhow::anyhow!("Already stopped")),
        }
    }

    pub async fn advance(self) -> anyhow::Result<Self> {
        let mut local_tmp_dir: Option<blocking::TmpDir> = None;
        let result = self.do_advance(&mut local_tmp_dir).await;
        if let Some(mut tmp_dir) = local_tmp_dir {
            crate::spawn_blocking(move || {
                tmp_dir.should_not_block = false;
                Ok(()) // drop tmp_dir
            })
            .await?;
        }

        result
    }
}
