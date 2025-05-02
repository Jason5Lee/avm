use std::path::{Path, PathBuf};

use anyhow::Context;
use flate2::read::GzDecoder;
use sha1::Digest;
use smol_str::SmolStr;
use zip::ZipArchive;

use crate::FileHash;

pub struct TmpDir {
    pub path: PathBuf,
    pub should_not_block: bool,
}

impl TmpDir {
    fn remove(&self) {
        std::fs::remove_dir_all(&self.path).unwrap_or_else(|e| {
            log::error!(
                "Failed to remove directory '{}': {}",
                self.path.display(),
                e
            );
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
    #[cfg(windows)]
    {
        junction::create(src_path, link_path)
    }

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(src_path, link_path)
    }
}

pub fn get_link_target(path: &Path) -> GetLinkResult<PathBuf> {
    let not_link_error: Option<i32>;
    #[cfg(windows)]
    {
        not_link_error = Some(4390);
    }
    #[cfg(not(windows))]
    {
        not_link_error = Some(22);
    }

    match std::fs::read_link(path) {
        Ok(target) => GetLinkResult::Link(target),
        Err(err) => {
            if err.kind() == std::io::ErrorKind::NotFound {
                GetLinkResult::NotFound
            } else if err.raw_os_error() == not_link_error {
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

    match check_is_link(alias_path) {
        GetLinkResult::Link(_) => {
            std::fs::remove_dir(alias_path)?;
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

    create_link(src_path, alias_path)?;

    Ok(())
}

pub fn list_tags(
    path: &Path,
    ignore_prefix: &str,
) -> std::io::Result<Vec<(SmolStr, Option<SmolStr>)>> {
    log::debug!("Listing tags in: {}", path.display());
    let mut tags = Vec::new();
    let entries = match std::fs::read_dir(path) {
        Ok(entries) => entries,
        Err(err) => {
            if err.kind() == std::io::ErrorKind::NotFound {
                return Ok(Vec::new());
            }
            return Err(err);
        }
    };

    for entry in entries {
        let entry = entry?;
        let file_name: SmolStr = entry.file_name().to_string_lossy().into();
        if file_name.starts_with(ignore_prefix) {
            continue;
        }
        match get_link_target(&entry.path()) {
            GetLinkResult::NotFound => {}
            GetLinkResult::Err(err) => return Err(err),
            GetLinkResult::Link(target) => tags.push((
                file_name,
                Some(target.file_name().unwrap().to_string_lossy().into()),
            )),
            GetLinkResult::NotLink => tags.push((file_name, None)),
        }
    }
    Ok(tags)
}

// It seems `pub(super)` cause problem. Use `pub(crate)` now before investigating the root cause.
pub(crate) fn verify_hash(hash: &FileHash, path: &Path) -> Result<(), anyhow::Error> {
    if let Some(sha1) = &hash.sha1 {
        let mut file = std::fs::File::open(path)?;
        let sha1_bytes = hex::decode(sha1)?;
        let mut hasher = sha1::Sha1::new();
        std::io::copy(&mut file, &mut hasher)?;
        if hasher.finalize().as_slice() != sha1_bytes.as_slice() {
            anyhow::bail!("sha1 verification failed");
        }
    }

    if let Some(sha256) = &hash.sha256 {
        let mut file = std::fs::File::open(path)?;
        let sha256_bytes = hex::decode(sha256)?;
        let mut hasher = sha2::Sha256::new();
        std::io::copy(&mut file, &mut hasher)?;
        if hasher.finalize().as_slice() != sha256_bytes.as_slice() {
            anyhow::bail!("sha256 verification failed");
        }
    }

    log::debug!("Hash verification passed");
    Ok(())
}

pub(crate) fn extract_archive(
    archive_type: super::ArchiveType,
    archive_path: &Path,
    extracted_dir: &Path,
) -> Result<(), anyhow::Error> {
    std::fs::create_dir_all(extracted_dir)?;
    let archive_file = std::fs::File::open(archive_path)?;
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
                    use std::os::unix::fs::PermissionsExt;

                    if let Some(mode) = file.unix_mode() {
                        let permissions = std::fs::Permissions::from_mode(mode);
                        std::fs::set_permissions(&out_path, permissions)?;
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
