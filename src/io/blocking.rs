use std::path::{Path, PathBuf};

use anyhow::Context;
use flate2::read::GzDecoder;
use sha1::Digest;
use smol_str::SmolStr;
use zip::ZipArchive;

use crate::FileHash;

pub struct Operating {
    pub tmp_dir_path: PathBuf,
    pub drop_should_not_block: bool,
    lock_file_path: PathBuf,
}

pub enum CreateOperatingError {
    AlreadyOperating,
    Io(std::io::Error),
}

impl Operating {
    pub fn create_in_tmp_dir(tmp_dir_path: PathBuf) -> Result<Self, CreateOperatingError> {
        std::fs::create_dir_all(&tmp_dir_path).map_err(CreateOperatingError::Io)?;
        let lock_file_path = tmp_dir_path.join(".lock");
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_file_path)
        {
            Ok(_) => Ok(Self {
                tmp_dir_path,
                drop_should_not_block: false,
                lock_file_path,
            }),
            Err(err) => {
                if err.kind() == std::io::ErrorKind::AlreadyExists {
                    Err(CreateOperatingError::AlreadyOperating)
                } else {
                    // Best effort: only remove the temporary directory itself, do not touch children.
                    let _ = std::fs::remove_dir(&tmp_dir_path);
                    Err(CreateOperatingError::Io(err))
                }
            }
        }
    }

    fn remove(&self) {
        std::fs::remove_file(&self.lock_file_path).unwrap_or_else(|e| {
            if e.kind() != std::io::ErrorKind::NotFound {
                log::error!(
                    "Failed to remove lock file '{}': {}",
                    self.lock_file_path.display(),
                    e
                );
            }
        });
        std::fs::remove_dir_all(&self.tmp_dir_path).unwrap_or_else(|e| {
            log::error!(
                "Failed to remove directory '{}': {}",
                self.tmp_dir_path.display(),
                e
            );
        });
    }
}

impl Drop for Operating {
    fn drop(&mut self) {
        if self.drop_should_not_block && !crate::is_cancelled() {
            log::warn!("Blocking remove: {}", self.tmp_dir_path.display());
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
    return junction::create(src_path, link_path);

    #[cfg(unix)]
    return std::os::unix::fs::symlink(src_path, link_path);
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
    match std::fs::symlink_metadata(path) {
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

pub fn remove_link(path: &Path) -> anyhow::Result<()> {
    #[cfg(not(windows))]
    std::fs::remove_file(path)?;
    #[cfg(windows)]
    std::fs::remove_dir(path)?;
    Ok(())
}

pub fn set_alias_tag(
    src_tag: &str,
    src_path: &Path,
    alias_tag: &str,
    alias_path: &Path,
) -> anyhow::Result<()> {
    if !src_path.exists() {
        anyhow::bail!("Src tag \"{src_tag}\" not found");
    }

    match check_is_link(alias_path) {
        GetLinkResult::Link(_) => {
            remove_link(alias_path)?;
        }
        GetLinkResult::NotFound => {}
        GetLinkResult::NotLink => {
            anyhow::bail!("Tag \"{alias_tag}\" exists and is not an alias");
        }
        GetLinkResult::Err(err) => {
            return Err(err)
                .with_context(|| anyhow::anyhow!("Failed to check alias tag '{alias_tag}'"));
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
            GetLinkResult::Link(target) => {
                let target_name = target
                    .file_name()
                    .ok_or_else(|| {
                        std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!(
                                "Link target '{}' has no terminal path component",
                                target.display()
                            ),
                        )
                    })?
                    .to_string_lossy()
                    .into();
                tags.push((file_name, Some(target_name)));
            }
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
        update_digest_from_reader(&mut file, &mut hasher)?;
        if hasher.finalize().as_slice() != sha1_bytes.as_slice() {
            anyhow::bail!("Sha1 verification failed");
        }
    }

    if let Some(sha256) = &hash.sha256 {
        let mut file = std::fs::File::open(path)?;
        let sha256_bytes = hex::decode(sha256)?;
        let mut hasher = sha2::Sha256::new();
        update_digest_from_reader(&mut file, &mut hasher)?;
        if hasher.finalize().as_slice() != sha256_bytes.as_slice() {
            anyhow::bail!("Sha256 verification failed");
        }
    }

    if let Some(sha512) = &hash.sha512 {
        let mut file = std::fs::File::open(path)?;
        let sha512_bytes = hex::decode(sha512)?;
        let mut hasher = sha2::Sha512::new();
        update_digest_from_reader(&mut file, &mut hasher)?;
        if hasher.finalize().as_slice() != sha512_bytes.as_slice() {
            anyhow::bail!("Sha512 verification failed");
        }
    }

    log::debug!("Hash verification passed");
    Ok(())
}

fn update_digest_from_reader(
    reader: &mut impl std::io::Read,
    digest: &mut impl Digest,
) -> Result<(), std::io::Error> {
    let mut buffer = [0_u8; 8192];
    loop {
        let read_len = reader.read(&mut buffer)?;
        if read_len == 0 {
            return Ok(());
        }
        digest.update(&buffer[..read_len]);
    }
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
        super::ArchiveType::TarXz => {
            let tar_xz_reader = xz2::read::XzDecoder::new(archive_file);
            let mut archive = tar::Archive::new(tar_xz_reader);
            archive.unpack(extracted_dir).with_context(|| {
                anyhow::anyhow!(
                    "Failed to unpack tar.xz archive '{}' into '{}'.",
                    archive_path.display(),
                    extracted_dir.display()
                )
            })?;
        }
    }

    Ok(())
}
