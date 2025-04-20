use std::path::{Path, PathBuf};

use anyhow::Context;
use flate2::read::GzDecoder;
use smol_str::{SmolStr, ToSmolStr};
use zip::ZipArchive;

pub struct TmpDir {
    pub path: PathBuf,
    pub should_not_block: bool,
}

impl TmpDir {
    fn remove(&self) {
        std::fs::remove_dir_all(&self.path).unwrap_or_else(|e| {
            log::error!("Error removing directory '{}': {}", self.path.display(), e);
        });
    }
}

impl Drop for TmpDir {
    fn drop(&mut self) {
        if self.should_not_block && !crate::is_cancelled() {
            log::warn!("Blocking remove: {}", self.path.display());
        }

        self.remove();
    }
}

pub enum GetLinkResult<R> {
    Link(R),
    NotLink,
    NotFound,
    Err(std::io::Error),
}

pub fn create_link(src_path: &Path, link_path: &Path) -> std::io::Result<()> {
    let r: std::io::Result<()>;
    #[cfg(windows)]
    {
        r = junction::create(src_path, link_path);
    }

    #[cfg(unix)]
    {
        r = std::fs::symlink(src_path, link_path);
    }

    Ok(r?)
}

pub fn get_link_target(path: &Path) -> GetLinkResult<PathBuf> {
    match std::fs::read_link(path) {
        Ok(target) => GetLinkResult::Link(target),
        Err(err) => {
            if err.kind() == std::io::ErrorKind::NotFound {
                GetLinkResult::NotFound
            } else if err.raw_os_error() == Some(4390) {
                GetLinkResult::NotLink
            } else {
                GetLinkResult::Err(err)
            }
        }
    }
}

pub fn check_is_link(path: &Path) -> GetLinkResult<()> {
    match std::fs::metadata(path) {
        Ok(metadata) => {
            if metadata.is_symlink() {
                GetLinkResult::Link(())
            } else {
                GetLinkResult::NotLink
            }
        }
        Err(err) => {
            if err.kind() == std::io::ErrorKind::NotFound {
                GetLinkResult::NotFound
            } else {
                GetLinkResult::Err(err)
            }
        }
    }
}

pub fn set_alias_tag(
    src_tag: &str,
    src_path: &Path,
    alias_tag: &str,
    alias_path: &Path,
) -> anyhow::Result<()> {
    if !src_path.exists() {
        anyhow::bail!("src tag '{src_tag}' not found");
    }

    match check_is_link(&alias_path) {
        GetLinkResult::Link(_) => {
            std::fs::remove_dir(&alias_path)?;
        }
        GetLinkResult::NotFound => {}
        GetLinkResult::NotLink => {
            anyhow::bail!("alias tag '{alias_tag}' exists and is not an alias");
        }
        GetLinkResult::Err(err) => {
            return Err(err)
                .with_context(|| anyhow::anyhow!("failed to check alias tag '{alias_tag}'"));
        }
    }

    create_link(&src_path, &alias_path)?;

    Ok(())
}

pub fn list_tags(path: &Path) -> std::io::Result<Vec<(SmolStr, Option<SmolStr>)>> {
    let mut tags = Vec::new();
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let file_name = entry.file_name().to_string_lossy().to_smolstr();
        match get_link_target(&entry.path()) {
            GetLinkResult::NotFound => {}
            GetLinkResult::Err(err) => return Err(err),
            GetLinkResult::Link(target) => tags.push((
                file_name,
                Some(target.file_name().unwrap().to_string_lossy().to_smolstr()),
            )),
            GetLinkResult::NotLink => tags.push((file_name, None)),
        }
    }
    Ok(tags)
}

pub(super) fn extract_archive(
    archive_type: super::ArchiveType,
    archive_path: &Path,
    extracted_dir: &Path,
) -> Result<(), anyhow::Error> {
    std::fs::create_dir_all(extracted_dir)?;
    let archive_file = std::fs::File::open(&archive_path)?;
    match archive_type {
        super::ArchiveType::Zip => {
            let mut archive = ZipArchive::new(archive_file)?;

            for i in 0..archive.len() {
                let mut file = archive.by_index(i)?;
                let out_path = extracted_dir.join(file.mangled_name());

                if file.is_dir() {
                    std::fs::create_dir_all(&out_path)?;
                } else {
                    if let Some(p) = out_path.parent() {
                        if !p.exists() {
                            std::fs::create_dir_all(p)?;
                        }
                    }
                    let mut out_file = std::fs::File::create(&out_path)?;
                    std::io::copy(&mut file, &mut out_file)?;
                }

                #[cfg(unix)]
                {
                    if let Some(mode) = file.unix_mode() {
                        let permissions = fs::Permissions::from_mode(mode);
                        fs::set_permissions(&out_path, permissions)?;
                    }
                }
            }
        }
        super::ArchiveType::TarGz => {
            let tar_gz_reader = GzDecoder::new(archive_file);

            // 2. Create a tar Archive from the decompressed stream
            let mut archive = tar::Archive::new(tar_gz_reader);

            // 3. Unpack the contents into the specified directory
            // The `unpack` method handles creating directories and files.
            // It also attempts to restore permissions and timestamps.
            archive.unpack(extracted_dir).with_context(|| {
                anyhow::anyhow!(
                    "Failed to unpack tar.gz archive '{}' into '{}'.",
                    archive_path.display(),
                    extracted_dir.display()
                )
            })?;
        }
    }

    Ok(())
}
