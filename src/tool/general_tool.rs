pub mod go;
pub mod liberica;

use crate::cli::AvmApp;
use crate::io::{
    blocking, ArchiveExtractInfo, ArchiveType, DownloadExtractCallback, DownloadExtractState,
};
use crate::tool::{GeneralTool, InstallVersion};
use crate::HttpClient;
use async_trait::async_trait;
use smol_str::SmolStr;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

const TMP_PREFIX: &str = ".tmp.";

struct InstallCustomAction {
    hash: crate::FileHash,
    tool_dir: PathBuf,
    target_tag: SmolStr,
    target_dir: PathBuf,
    default: bool,
}

#[async_trait]
impl DownloadExtractCallback for InstallCustomAction {
    async fn on_downloaded(&mut self, info: &ArchiveExtractInfo) -> anyhow::Result<()> {
        crate::spawn_blocking({
            let hash = self.hash.clone();
            let archive_path = info.archive_path.clone();
            move || blocking::verify_hash(&hash, &archive_path)
        })
        .await?;
        Ok(())
    }

    async fn on_extracted(&mut self, info: &ArchiveExtractInfo) -> anyhow::Result<()> {
        let extracted_dir = info.extracted_dir.clone();
        let target_dir = self.target_dir.clone();
        let target_dir = crate::spawn_blocking(move || {
            let entries = std::fs::read_dir(&extracted_dir)?
                .take(2)
                .collect::<Result<Vec<_>, _>>()?;

            let move_source = if entries.len() == 1 {
                let entry = &entries[0];
                let path = entry.path();
                if path.is_dir() {
                    path
                } else {
                    extracted_dir
                }
            } else {
                extracted_dir
            };

            if target_dir.exists() {
                std::fs::remove_dir_all(&target_dir)?;
            }
            if let Some(parent) = target_dir.parent() {
                std::fs::create_dir_all(parent)?;
            }

            std::fs::rename(move_source, &target_dir)?;
            Ok(target_dir)
        })
        .await?;

        if self.default {
            let default_path = self.tool_dir.join(AvmApp::DEFAULT_TAG);
            let target_tag = self.target_tag.clone();
            crate::spawn_blocking(move || {
                blocking::set_alias_tag(
                    &target_tag,
                    &target_dir,
                    AvmApp::DEFAULT_TAG,
                    &default_path,
                )
            })
            .await?;
        }

        Ok(())
    }
}

pub struct InstallArgs<'a> {
    pub tool: &'a dyn GeneralTool,
    pub client: &'a HttpClient,
    pub tools_base: &'a Path,
    pub platform: Option<SmolStr>,
    pub flavor: Option<SmolStr>,
    pub install_version: InstallVersion,
    pub update: bool,
    pub default: bool,
}

impl InstallArgs<'_> {
    pub async fn install(self) -> anyhow::Result<(SmolStr, DownloadExtractState)> {
        let down_info = self
            .tool
            .get_down_info(
                self.platform.clone(),
                self.flavor.clone(),
                self.install_version,
            )
            .await?;
        let down_info = super::DownInfo::from_tool_down_info(
            down_info,
            self.platform.as_deref(),
            self.flavor.as_deref(),
        );
        if down_info.tag.starts_with(TMP_PREFIX) {
            anyhow::bail!("tag '{}' is reserved for temporary use", down_info.tag);
        }
        let tool_dir = self.tools_base.join(&self.tool.info().name);
        log::debug!("Tool dir: {}", tool_dir.display());
        let tag_dir = tool_dir.join(&down_info.tag);
        log::debug!("Tag dir: {}", tag_dir.display());
        let tag_dir = if self.update {
            tag_dir
        } else {
            let (tag_dir, exists) = crate::spawn_blocking(move || {
                let exists = tag_dir.exists();
                Ok((tag_dir, exists))
            })
            .await?;

            if exists {
                anyhow::bail!("\"{}\" already exists", down_info.tag);
            }

            tag_dir
        };

        let tmp_dir = tool_dir.join(format!("{}{}", TMP_PREFIX, down_info.tag));
        log::debug!("Tmp dir: {}", tmp_dir.display());
        let (tmp_dir, exists) = crate::spawn_blocking(move || {
            let exists = tmp_dir.exists();
            Ok((tmp_dir, exists))
        })
        .await?;
        if exists {
            anyhow::bail!("\"{}\" is installing", down_info.tag);
        }

        let state = DownloadExtractState::start(
            self.client,
            &down_info.url,
            tmp_dir,
            Box::new(InstallCustomAction {
                hash: down_info.hash,
                tool_dir,
                target_tag: down_info.tag.clone(),
                target_dir: tag_dir,
                default: self.default,
            }),
        )
        .await?;

        Ok((down_info.tag, state))
    }
}

pub(crate) async fn install_from_archive(
    tool: &dyn GeneralTool,
    tools_base: &Path,
    archive: PathBuf,
    target_tag: &str,
    hash: Option<&str>,
    update: bool,
    default: bool,
) -> anyhow::Result<()> {
    if target_tag.starts_with(TMP_PREFIX) {
        anyhow::bail!("tag '{}' is reserved for temporary use", target_tag);
    }
    let tool_dir = tools_base.join(&tool.info().name);
    log::debug!("Tool dir: {}", tool_dir.display());
    let tag_dir = tool_dir.join(target_tag);
    log::debug!("Tag dir: {}", tag_dir.display());
    let tag_dir = if update {
        tag_dir
    } else {
        let (tag_dir, exists) = crate::spawn_blocking(move || {
            let exists = tag_dir.exists();
            Ok((tag_dir, exists))
        })
        .await?;

        if exists {
            anyhow::bail!("\"{}\" already exists", target_tag);
        }

        tag_dir
    };

    let tmp_dir = tool_dir.join(format!("{}{}", TMP_PREFIX, target_tag));
    log::debug!("Tmp dir: {}", tmp_dir.display());
    let (tmp_dir, exists) = crate::spawn_blocking(move || {
        let exists = tmp_dir.exists();
        Ok((tmp_dir, exists))
    })
    .await?;
    if exists {
        anyhow::bail!("\"{}\" is installing", target_tag);
    }

    let archive_type = ArchiveType::from_path(archive.as_os_str().as_encoded_bytes())?;
    let hash = hash.map(serde_yaml_ng::from_str::<crate::FileHash>);
    let tag_dir = crate::spawn_blocking(move || {
        let mut tmp_dir = blocking::TmpDir {
            path: tmp_dir,
            should_not_block: false,
        };
        if let Some(hash) = hash {
            blocking::verify_hash(&hash?, &archive)?;
        }

        log::info!("Extracting ...");

        std::fs::remove_dir_all(&tmp_dir.path).ok();
        std::fs::create_dir_all(&tmp_dir.path)?;
        blocking::extract_archive(archive_type, &archive, &tmp_dir.path)?;
        std::fs::rename(&tmp_dir.path, &tag_dir)?;
        let tag_dir = std::mem::take(&mut tmp_dir.path);
        std::mem::forget(tmp_dir);
        Ok(tag_dir)
    })
    .await?;

    if default {
        let default_path = tool_dir.join(AvmApp::DEFAULT_TAG);
        let target_tag = target_tag.to_owned();
        crate::spawn_blocking(move || {
            blocking::set_alias_tag(&target_tag, &tag_dir, AvmApp::DEFAULT_TAG, &default_path)
        })
        .await?;
    }

    Ok(())
}

pub async fn get_downinfo(
    tool: &dyn GeneralTool,
    platform: Option<SmolStr>,
    flavor: Option<SmolStr>,
    install_version: InstallVersion,
) -> anyhow::Result<super::DownInfo> {
    let down_info = tool
        .get_down_info(platform.clone(), flavor.clone(), install_version)
        .await?;
    let down_info =
        super::DownInfo::from_tool_down_info(down_info, platform.as_deref(), flavor.as_deref());
    Ok(down_info)
}

pub async fn get_vers(
    tool: &dyn GeneralTool,
    platform: Option<SmolStr>,
    flavor: Option<SmolStr>,
    major_version: Option<SmolStr>,
) -> anyhow::Result<Vec<super::Version>> {
    tool.fetch_versions(platform, flavor, major_version).await
}

pub async fn delete_tag(
    tool: &dyn GeneralTool,
    tools_base: &Path,
    tag_to_delete: SmolStr,
    allow_dangling: bool,
) -> anyhow::Result<()> {
    let tool_dir = tools_base.join(&tool.info().name);

    crate::spawn_blocking(move || {
        if !allow_dangling {
            // Check if the tag is an alias target
            for (tag, alias_tag) in blocking::list_tags(&tool_dir, TMP_PREFIX)? {
                if alias_tag.as_ref() == Some(&tag_to_delete) {
                    anyhow::bail!(
                        "tag \"{}\" is an alias target of \"{}\", delete the alias first",
                        tag_to_delete,
                        tag
                    );
                }
            }
        }

        let tag_dir = tool_dir.join(&*tag_to_delete);
        // Attempt to remove the directory
        std::fs::remove_dir_all(&tag_dir).map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                anyhow::anyhow!("tag '{}' not found", tag_to_delete)
            } else {
                anyhow::Error::from(err)
                    .context(format!("failed to delete tag '{}'", tag_to_delete))
            }
        })?;
        Ok(())
    })
    .await
}

pub async fn list_tags(
    tool: &dyn GeneralTool,
    tools_base: &Path,
) -> anyhow::Result<Vec<(SmolStr, Option<SmolStr>)>> {
    let tool_dir = tools_base.join(&tool.info().name);
    crate::spawn_blocking(move || Ok(blocking::list_tags(&tool_dir, TMP_PREFIX)?)).await
}

pub async fn create_alias_tag(
    tool: &dyn GeneralTool,
    tools_base: &Path,
    src_tag: SmolStr,
    alias_tag: SmolStr,
) -> anyhow::Result<()> {
    let tool_dir = tools_base.join(&tool.info().name);
    let src_path = tool_dir.join(&src_tag);
    let alias_path = tool_dir.join(&alias_tag);
    log::debug!("Alias src path: {}", src_path.display());
    log::debug!("Alias path: {}", alias_path.display());

    crate::spawn_blocking(move || {
        blocking::set_alias_tag(&src_tag, &src_path, &alias_tag, &alias_path)
    })
    .await
}

pub async fn copy_tag(
    tool: &dyn GeneralTool,
    tools_base: &Path,
    src_tag: SmolStr,
    dest_tag: SmolStr,
) -> anyhow::Result<()> {
    let tool_dir = tools_base.join(&tool.info().name);
    if dest_tag == crate::cli::AvmApp::DEFAULT_TAG {
        anyhow::bail!("default tag is only allowed as an alias tag");
    }

    let src_path = tool_dir.join(&*src_tag);
    let dest_path = tool_dir.join(&*dest_tag);
    log::debug!("Copy src path: {}", src_path.display());
    log::debug!("Copy dest path: {}", dest_path.display());

    crate::spawn_blocking(move || {
        if !src_path.exists() {
            anyhow::bail!("src tag '{}' not found", src_tag);
        }
        if dest_path.exists() {
            anyhow::bail!("dest tag '{}' already exists", dest_tag);
        }

        let copy_options = fs_extra::dir::CopyOptions::new();
        fs_extra::dir::copy(&src_path, &dest_path, &copy_options)?;
        Ok(())
    })
    .await
}

pub fn get_tag_path(
    tool: &dyn GeneralTool,
    tools_base: &Path,
    tag: &str,
) -> anyhow::Result<PathBuf> {
    let tag_path = tools_base.join(&tool.info().name).join(tag);
    if !tag_path.exists() {
        anyhow::bail!("tag '{}' not found", tag);
    }
    Ok(tag_path)
}

pub fn get_exe_path(
    tool: &dyn GeneralTool,
    tools_base: &Path,
    tag: &str,
) -> anyhow::Result<PathBuf> {
    let tag_dir = get_tag_path(tool, tools_base, tag)?;
    tool.bin_path(&tag_dir)
}

pub async fn run(
    tool: &dyn GeneralTool,
    tools_base: &Path,
    tag: &str,
    args: Vec<OsString>,
) -> anyhow::Result<()> {
    let bin_path = get_exe_path(tool, tools_base, tag)?;
    let mut command = std::process::Command::new(bin_path);
    command.args(args);
    crate::spawn_blocking(move || {
        command.spawn()?.wait()?;
        Ok(())
    })
    .await
}

/// Clean up the temporary directories and dangling alias tags
pub async fn clean(tool: &dyn GeneralTool, tools_base: &Path) -> anyhow::Result<()> {
    let tool_dir = tools_base.join(&tool.info().name);

    crate::spawn_blocking(move || {
        let entries = match std::fs::read_dir(&tool_dir) {
            Ok(entries) => entries,
            Err(err) => {
                if err.kind() == std::io::ErrorKind::NotFound {
                    // Tool directory doesn't exist, nothing to clean.
                    log::debug!(
                        "Tool directory {} not found, nothing to clean.",
                        tool_dir.display()
                    );
                    return Ok(());
                }
                return Err(anyhow::Error::from(err).context(format!(
                    "Failed to read tool directory: {}",
                    tool_dir.display()
                )));
            }
        };

        log::info!("Cleaning up tool directory: {}", tool_dir.display());

        for entry_result in entries {
            let entry = match entry_result {
                Ok(entry) => entry,
                Err(err) => {
                    log::warn!(
                        "Failed to read directory entry in {}: {}",
                        tool_dir.display(),
                        err
                    );
                    continue; // Skip this entry
                }
            };

            let entry_path = entry.path();
            let file_name = entry.file_name();
            let file_name_str = file_name.to_string_lossy();

            // 1. Clean temporary directories
            if file_name_str.starts_with(TMP_PREFIX) {
                log::info!("Removing temporary directory: {}", entry_path.display());
                if let Err(err) = std::fs::remove_dir_all(&entry_path) {
                    log::warn!(
                        "Failed to remove temporary directory {}: {}",
                        entry_path.display(),
                        err
                    );
                }
                continue; // Move to the next entry
            }

            // 2. Check for dangling aliases (symlinks)
            match std::fs::symlink_metadata(&entry_path) {
                Ok(metadata) => {
                    if metadata.file_type().is_symlink() {
                        // Check if the target exists. We use metadata() which follows the link.
                        // If it fails (e.g., NotFound), the link is dangling.
                        if std::fs::metadata(&entry_path).is_err() {
                            log::info!("Removing dangling alias '{}'", entry_path.display(),);
                            // Use remove_file to remove dangling symlinks
                            if let Err(err) = std::fs::remove_file(&entry_path) {
                                log::warn!(
                                    "Failed to remove dangling alias {}: {}",
                                    entry_path.display(),
                                    err
                                );
                            }
                        }
                    }
                }
                Err(err) => {
                    // Ignore errors like NotFound if the entry was removed concurrently
                    if err.kind() != std::io::ErrorKind::NotFound {
                        log::warn!(
                            "Failed to get metadata for {}: {}",
                            entry_path.display(),
                            err
                        );
                    }
                }
            }
        }
        log::info!("Finished cleaning up {}", tool_dir.display());
        Ok(())
    })
    .await
}
