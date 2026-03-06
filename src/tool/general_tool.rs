pub mod go;
pub mod liberica;
pub mod node;

use crate::io::{
    blocking, ArchiveExtractInfo, ArchiveType, DownloadExtractCallback, DownloadExtractState,
};
use crate::tool::{GeneralTool, ToolInfo, Version, VersionFilter};
use crate::{HttpClient, Tag};
use async_trait::async_trait;
use rustc_hash::FxHashSet;
use smol_str::SmolStr;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

const TMP_PREFIX: &str = ".tmp.";
const DEFAULT_TAG: &str = "default";
const VERSION_INFO_FILE: &str = ".avm.version-info.toml";

pub fn default_tag() -> Tag {
    Tag::try_from(SmolStr::new(DEFAULT_TAG)).expect("Default tag is invalid")
}

struct InstallCustomAction {
    hash: crate::FileHash,
    version: Version,
    tool_dir: PathBuf,
    target_tag: SmolStr,
    target_dir: PathBuf,
    default: bool,
}

async fn create_operating(tmp_dir: PathBuf, tag: String) -> anyhow::Result<blocking::Operating> {
    crate::spawn_blocking(
        move || match blocking::Operating::create_in_tmp_dir(tmp_dir.clone()) {
            Ok(operating) => Ok(operating),
            Err(blocking::CreateOperatingError::AlreadyOperating) => {
                anyhow::bail!("\"{}\" is being operated", tag)
            }
            Err(blocking::CreateOperatingError::Io(err)) => {
                Err(anyhow::Error::from(err).context(format!(
                    "Failed to create operation lock under temporary directory '{}'",
                    tmp_dir.display()
                )))
            }
        },
    )
    .await
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
        let version = self.version.clone();
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
            write_version_info_file(&target_dir, &version)?;
            Ok(target_dir)
        })
        .await?;

        if self.default {
            let default_path = self.tool_dir.join(DEFAULT_TAG);
            let target_tag = self.target_tag.clone();
            crate::spawn_blocking(move || {
                blocking::set_alias_tag(&target_tag, &target_dir, DEFAULT_TAG, &default_path)
            })
            .await?;
        }

        Ok(())
    }
}

pub struct InstallArgs<'a, T: GeneralTool> {
    pub tool_name: &'a str,
    pub tool: &'a T,
    pub client: &'a HttpClient,
    pub tools_base: &'a Path,
    pub platform: Option<SmolStr>,
    pub flavor: Option<SmolStr>,
    pub install_version: VersionFilter,
    pub update: bool,
    pub default: bool,
}

impl<T: GeneralTool> InstallArgs<'_, T> {
    pub async fn install(self) -> anyhow::Result<(SmolStr, SmolStr, DownloadExtractState)> {
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
            anyhow::bail!("Tag \"{}\" is reserved for temporary use", down_info.tag);
        }
        let tool_dir = self.tools_base.join(self.tool_name);
        log::debug!("Tool dir: {}", tool_dir.display());
        let tag_dir = tool_dir.join(&down_info.tag);
        log::debug!("Tag dir: {}", tag_dir.display());
        let tmp_dir = tool_dir.join(format!("{}{}", TMP_PREFIX, down_info.tag));
        log::debug!("Tmp dir: {}", tmp_dir.display());
        let operating = create_operating(tmp_dir, down_info.tag.to_string()).await?;

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

        let state = DownloadExtractState::start(
            self.client,
            &down_info.url,
            operating,
            Box::new(InstallCustomAction {
                hash: down_info.hash,
                version: Version {
                    version: down_info.version.clone(),
                    is_lts: down_info.is_lts,
                },
                tool_dir,
                target_tag: down_info.tag.clone(),
                target_dir: tag_dir,
                default: self.default,
            }),
        )
        .await?;

        Ok((down_info.tag, down_info.url, state))
    }
}

pub struct LocalInstaller<'a> {
    pub tool_name: &'a str,
    pub tools_base: &'a Path,
    pub archive: PathBuf,
    pub target_tag: &'a str,
    pub version: Version,
    pub hash: Option<&'a str>,
    pub update: bool,
    pub default: bool,
}

impl LocalInstaller<'_> {
    pub async fn install(self) -> anyhow::Result<()> {
        let Self {
            tool_name,
            tools_base,
            archive,
            target_tag,
            version,
            hash,
            update,
            default,
        } = self;

        if target_tag.starts_with(TMP_PREFIX) {
            anyhow::bail!("Tag '{}' is reserved for temporary use", target_tag);
        }
        let tool_dir = tools_base.join(tool_name);
        log::debug!("Tool dir: {}", tool_dir.display());
        let tag_dir = tool_dir.join(target_tag);
        log::debug!("Tag dir: {}", tag_dir.display());
        let tmp_dir = tool_dir.join(format!("{}{}", TMP_PREFIX, target_tag));
        log::debug!("Tmp dir: {}", tmp_dir.display());
        let operating = create_operating(tmp_dir, target_tag.to_owned()).await?;

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

        let archive_type = ArchiveType::from_path(archive.as_os_str().as_encoded_bytes())?;
        let hash = hash.map(toml::from_str::<crate::FileHash>);
        let tag_dir = crate::spawn_blocking(move || {
            let mut operating = operating;
            if let Some(hash) = hash {
                blocking::verify_hash(&hash?, &archive)?;
            }

            log::info!("Extracting ...");

            let extracted_dir = operating.tmp_dir_path.join("extracted");
            std::fs::remove_dir_all(&extracted_dir).ok();
            std::fs::create_dir_all(&extracted_dir)?;
            blocking::extract_archive(archive_type, &archive, &extracted_dir)?;
            std::fs::remove_dir_all(&tag_dir).ok();
            std::fs::rename(&extracted_dir, &tag_dir)?;
            write_version_info_file(&tag_dir, &version)?;
            operating.drop_should_not_block = false;
            Ok(tag_dir)
        })
        .await?;

        if default {
            let default_path = tool_dir.join(DEFAULT_TAG);
            let target_tag = target_tag.to_owned();
            crate::spawn_blocking(move || {
                blocking::set_alias_tag(&target_tag, &tag_dir, DEFAULT_TAG, &default_path)
            })
            .await?;
        }

        Ok(())
    }
}

pub async fn get_downinfo(
    tool: &impl GeneralTool,
    platform: Option<SmolStr>,
    flavor: Option<SmolStr>,
    version_filter: VersionFilter,
) -> anyhow::Result<super::DownInfo> {
    let down_info = tool
        .get_down_info(platform.clone(), flavor.clone(), version_filter)
        .await?;
    let down_info =
        super::DownInfo::from_tool_down_info(down_info, platform.as_deref(), flavor.as_deref());
    Ok(down_info)
}

pub async fn get_vers(
    tool: &impl GeneralTool,
    platform: Option<SmolStr>,
    flavor: Option<SmolStr>,
    version_filter: VersionFilter,
) -> anyhow::Result<Vec<super::Version>> {
    tool.fetch_versions(platform, flavor, version_filter).await
}

pub async fn remove_tag(
    tool_name: &str,
    tools_base: &Path,
    tags_to_remove: Vec<SmolStr>,
    allow_dangling: bool,
) -> anyhow::Result<()> {
    let tool_dir = tools_base.join(tool_name);
    let tags_set = tags_to_remove.iter().cloned().collect::<FxHashSet<_>>();

    crate::spawn_blocking(move || {
        if !allow_dangling {
            // Check if the tag is an alias target
            for (tag, alias_tag) in blocking::list_tags(&tool_dir, TMP_PREFIX)? {
                if let Some(alias_tag) = alias_tag {
                    if !tags_set.contains(&tag) && tags_set.contains(&alias_tag) {
                        anyhow::bail!(
                            "Tag \"{}\" is an alias target of \"{}\", remove the alias first",
                            alias_tag,
                            tag
                        );
                    }
                }
            }
        }

        for tag in tags_to_remove {
            let tag_dir = tool_dir.join(&*tag);
            // Attempt to remove the directory
            std::fs::remove_dir_all(&tag_dir).map_err(|err| {
                if err.kind() == std::io::ErrorKind::NotFound {
                    anyhow::anyhow!("Tag \"{}\" not found", tag)
                } else {
                    anyhow::Error::from(err).context(format!("Failed to remove tag \"{}\"", tag))
                }
            })?;
        }
        Ok(())
    })
    .await
}

pub async fn list_tags(
    tool_name: &str,
    tools_base: &Path,
) -> anyhow::Result<Vec<(SmolStr, Option<SmolStr>)>> {
    let tool_dir = tools_base.join(tool_name);
    crate::spawn_blocking(move || Ok(blocking::list_tags(&tool_dir, TMP_PREFIX)?)).await
}

pub async fn create_alias_tag(
    tool_name: &str,
    tools_base: &Path,
    src_tag: SmolStr,
    alias_tag: SmolStr,
) -> anyhow::Result<()> {
    let tool_dir = tools_base.join(tool_name);
    let tmp_dir = tool_dir.join(format!("{}{}", TMP_PREFIX, alias_tag));
    let operating = create_operating(tmp_dir, alias_tag.to_string()).await?;
    let src_path = tool_dir.join(&src_tag);
    let alias_path = tool_dir.join(&alias_tag);
    log::debug!("Alias src path: {}", src_path.display());
    log::debug!("Alias path: {}", alias_path.display());

    crate::spawn_blocking(move || {
        let _operating = operating;
        blocking::set_alias_tag(&src_tag, &src_path, &alias_tag, &alias_path)
    })
    .await
}

pub async fn copy_tag(
    tool_name: &str,
    tools_base: &Path,
    src_tag: SmolStr,
    dest_tag: SmolStr,
) -> anyhow::Result<()> {
    let tool_dir = tools_base.join(tool_name);
    if dest_tag == DEFAULT_TAG {
        anyhow::bail!("\"{DEFAULT_TAG}\" tag is only allowed as an alias tag");
    }

    let src_path = tool_dir.join(&*src_tag);
    let dest_path = tool_dir.join(&*dest_tag);
    let tmp_dir = tool_dir.join(format!("{}{}", TMP_PREFIX, dest_tag));
    let operating = create_operating(tmp_dir, dest_tag.to_string()).await?;
    log::debug!("Copy src path: {}", src_path.display());
    log::debug!("Copy dest path: {}", dest_path.display());

    crate::spawn_blocking(move || {
        let operating = operating;
        if !src_path.exists() {
            anyhow::bail!("Src tag \"{}\" not found", src_tag);
        }
        if dest_path.exists() {
            anyhow::bail!("Dest tag \"{}\" already exists", dest_tag);
        }

        let tmp_copy_root = operating.tmp_dir_path.join("copy");
        std::fs::remove_dir_all(&tmp_copy_root).ok();
        std::fs::create_dir_all(&tmp_copy_root)?;

        let copy_options = fs_extra::dir::CopyOptions::new();
        fs_extra::dir::copy(&src_path, &tmp_copy_root, &copy_options)?;
        let copied_dir = tmp_copy_root.join(
            src_path
                .file_name()
                .ok_or_else(|| anyhow::anyhow!("Invalid source tag path"))?,
        );
        std::fs::rename(copied_dir, &dest_path)?;
        Ok(())
    })
    .await
}

pub async fn find_matching_local_tag(
    tool_name: &str,
    tool: &impl GeneralTool,
    tools_base: &Path,
    platform: Option<SmolStr>,
    flavor: Option<SmolStr>,
    version_filter: VersionFilter,
) -> anyhow::Result<Option<SmolStr>> {
    let tool_dir = tools_base.join(tool_name);
    let info = tool.info();
    let tag_prefixes = build_tag_prefixes(info, platform.as_deref(), flavor.as_deref());
    let local_tags_and_versions =
        crate::spawn_blocking(move || -> anyhow::Result<Vec<(SmolStr, Version)>> {
            let tags = blocking::list_tags(&tool_dir, TMP_PREFIX)?;
            let mut local_tags_and_versions = Vec::new();
            for (tag, _) in tags {
                let tag_path = tool_dir.join(&*tag);
                let version_info_path = tag_path.join(VERSION_INFO_FILE);
                let version_info_raw = match std::fs::read_to_string(&version_info_path) {
                    Ok(value) => value,
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(err) => {
                        log::warn!(
                            "Failed to read version info for tag '{}': {}",
                            tag,
                            anyhow::Error::from(err)
                                .context(version_info_path.display().to_string())
                        );
                        continue;
                    }
                };
                let version = match toml::from_str::<Version>(&version_info_raw) {
                    Ok(version) => version,
                    Err(err) => {
                        log::warn!(
                            "Failed to parse version info for tag '{}': {}",
                            tag,
                            anyhow::Error::from(err)
                                .context(version_info_path.display().to_string())
                        );
                        continue;
                    }
                };
                local_tags_and_versions.push((tag, version));
            }
            Ok(local_tags_and_versions)
        })
        .await?;
    let tags_and_versions = local_tags_and_versions
        .iter()
        .filter(|(tag, _)| parse_tag_version_start(tag, &tag_prefixes).is_some())
        .map(|(tag, version)| (&**tag, version));

    Ok(tool.find_best_matching_local_tag(tags_and_versions, &version_filter))
}

#[derive(Clone)]
struct TagPrefix {
    value: SmolStr,
}

impl TagPrefix {
    #[inline]
    fn version_start(&self) -> usize {
        self.value.len()
    }
}

fn build_tag_prefixes(
    info: &ToolInfo,
    platform: Option<&str>,
    flavor: Option<&str>,
) -> Vec<TagPrefix> {
    let platform_candidates = if let Some(platform) = platform {
        vec![Some(SmolStr::from(platform))]
    } else {
        let mut candidates = vec![None];
        if let Some(all_platforms) = info.all_platforms.as_ref() {
            candidates.extend(all_platforms.iter().cloned().map(Some));
        }
        candidates
    };
    let flavor_candidates = if let Some(flavor) = flavor {
        vec![Some(SmolStr::from(flavor))]
    } else {
        let mut candidates = vec![None];
        if let Some(all_flavors) = info.all_flavors.as_ref() {
            candidates.extend(all_flavors.iter().cloned().map(Some));
        }
        candidates
    };

    let mut tag_prefixes: Vec<_> = Vec::new();
    for platform in platform_candidates.iter() {
        for flavor in flavor_candidates.iter() {
            let mut prefix = String::new();
            if let Some(platform) = platform {
                prefix.push_str(platform);
                prefix.push('_');
            }
            if let Some(flavor) = flavor {
                prefix.push_str(flavor);
                prefix.push('_');
            }
            tag_prefixes.push(TagPrefix {
                value: SmolStr::from(prefix),
            });
        }
    }

    tag_prefixes.sort_by_key(|a| std::cmp::Reverse(a.version_start()));
    tag_prefixes
}

fn parse_tag_version_start(tag: &str, tag_prefixes: &[TagPrefix]) -> Option<usize> {
    for prefix in tag_prefixes {
        if !tag.starts_with(prefix.value.as_str()) {
            continue;
        }
        let version_start = prefix.version_start();
        if tag[version_start..].is_empty() {
            continue;
        }
        return Some(version_start);
    }
    None
}

fn write_version_info_file(tag_dir: &Path, version: &Version) -> anyhow::Result<()> {
    let version_info_path = tag_dir.join(VERSION_INFO_FILE);
    let content = toml::to_string(version)?;
    std::fs::write(version_info_path, content)?;
    Ok(())
}

pub fn get_tag_path(tool_name: &str, tools_base: &Path, tag: &str) -> anyhow::Result<PathBuf> {
    let tag_path = tools_base.join(tool_name).join(tag);
    if !tag_path.exists() {
        anyhow::bail!("Tag \"{}\" not found", tag);
    }
    Ok(tag_path)
}

pub fn get_entry_path(
    tool_name: &str,
    tool: &impl GeneralTool,
    tools_base: &Path,
    tag: &str,
) -> anyhow::Result<PathBuf> {
    let tag_dir = get_tag_path(tool_name, tools_base, tag)?;
    tool.entry_path(tag_dir)
}

pub async fn run_command(
    tool_name: &str,
    tool: &impl GeneralTool,
    tools_base: &Path,
    tag: &str,
    args: Vec<OsString>,
) -> anyhow::Result<std::process::Command> {
    let bin_path = get_entry_path(tool_name, tool, tools_base, tag)?;
    let mut command = std::process::Command::new(bin_path);
    command.args(args);
    Ok(command)
}

/// Clean up the temporary directories and dangling alias tags
pub async fn clean(tool_name: &str, tools_base: &Path) -> anyhow::Result<()> {
    let tool_dir = tools_base.join(tool_name);

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

        log::debug!("Cleaning up tool directory: {}", tool_dir.display());

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

            // Clean temporary directories
            if file_name_str.starts_with(TMP_PREFIX) {
                log::debug!("Removing temporary directory: {}", entry_path.display());
                if let Err(err) = std::fs::remove_dir_all(&entry_path) {
                    log::warn!(
                        "Failed to remove temporary directory {}: {}",
                        entry_path.display(),
                        err
                    );
                }
                continue; // Move to the next entry
            }

            // Check for dangling aliases (symlinks)
            match std::fs::symlink_metadata(&entry_path) {
                Ok(metadata) => {
                    if metadata.file_type().is_symlink() {
                        // Check if the target exists. We use metadata() which follows the link.
                        // If it fails (e.g., NotFound), the link is dangling.
                        if std::fs::metadata(&entry_path).is_err() {
                            log::debug!("Removing dangling alias '{}'", entry_path.display());
                            // Use remove_file to remove dangling symlinks
                            if let Err(err) = blocking::remove_link(&entry_path) {
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
        log::debug!("Finished cleaning up {}", tool_dir.display());
        Ok(())
    })
    .await
}
