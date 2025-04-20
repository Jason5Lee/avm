pub mod liberica;

use crate::cli::AvmApp;
use crate::io::{blocking, ArchiveExtractInfo, DownloadExtractCustomAction, DownloadExtractState};
use crate::tool::{GeneralTool, InstallVersion};
use crate::HttpClient;
use async_trait::async_trait;
use sha1::Digest;
use smol_str::SmolStr;
use std::ffi::OsString;
use std::fs::File;
use std::path::{Path, PathBuf};

struct InstallCustomAction {
    sha1: SmolStr,
    tool_dir: PathBuf,
    target_tag: SmolStr,
    target_dir: PathBuf,
    default: bool,
}

#[async_trait]
impl DownloadExtractCustomAction for InstallCustomAction {
    async fn on_downloaded(&mut self, info: &ArchiveExtractInfo) -> anyhow::Result<()> {
        let sha1 = hex::decode(&self.sha1)?;
        let mut file = File::open(&info.archive_path)?;
        let mut hasher = sha1::Sha1::new();
        std::io::copy(&mut file, &mut hasher)?;
        let result = hasher.finalize();
        if result.as_slice() != sha1 {
            anyhow::bail!("sha1 verification failed");
        }

        log::debug!("sha1 verification passed");
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
                    &AvmApp::DEFAULT_TAG,
                    &default_path,
                )
            })
            .await?;
        }

        Ok(())
    }
}

pub async fn install(
    tool: &dyn GeneralTool,
    client: &HttpClient,
    tools_base: &Path,
    platform: Option<SmolStr>,
    flavor: Option<SmolStr>,
    install_version: InstallVersion,
    force: bool,
    default: bool,
) -> anyhow::Result<DownloadExtractState> {
    let mut target_tag = String::new();
    if let Some(p) = &platform {
        target_tag.push_str(&p);
        target_tag.push('_');
    }
    if let Some(f) = &flavor {
        target_tag.push_str(&f);
        target_tag.push('_');
    }

    let down_url = tool.get_down_url(platform, flavor, install_version).await?;
    target_tag.push_str(&down_url.version);
    let tool_dir = tools_base.join(&tool.info().name);
    log::debug!("Tool dir: {}", tool_dir.display());
    let instance_dir = tool_dir.join(&target_tag);
    log::debug!("Instance dir: {}", instance_dir.display());
    if instance_dir.exists() && !force {
        anyhow::bail!("\"{}\" already installed", target_tag);
    }

    let tmp_dir = tool_dir.join(format!("{}_tmp", target_tag));
    log::debug!("Tmp dir: {}", tmp_dir.display());
    if tmp_dir.exists() {
        anyhow::bail!("\"{}\" is installing", target_tag);
    }

    DownloadExtractState::start(
        client,
        &down_url.url,
        tmp_dir,
        Box::new(InstallCustomAction {
            sha1: down_url.sha1,
            tool_dir,
            target_tag: target_tag.into(),
            target_dir: instance_dir,
            default,
        }),
    )
    .await
}

pub async fn get_downurl(
    tool: &dyn GeneralTool,
    platform: Option<SmolStr>,
    flavor: Option<SmolStr>,
    install_version: InstallVersion,
) -> anyhow::Result<super::DownUrl> {
    tool.get_down_url(platform, flavor, install_version).await
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
) -> anyhow::Result<()> {
    let tool_dir = tools_base.join(&tool.info().name);

    crate::spawn_blocking(move || {
        // Check if the tag is an alias target
        for (tag, alias_tag) in blocking::list_tags(&tool_dir)? {
            if alias_tag.map_or(false, |at| at == tag_to_delete) {
                anyhow::bail!(
                    "tag '{}' is an alias target of '{}', delete the alias first",
                    tag_to_delete,
                    tag
                );
            }
        }

        let instance_dir = tool_dir.join(&*tag_to_delete);
        // Attempt to remove the directory
        std::fs::remove_dir_all(&instance_dir).map_err(|err| {
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
    crate::spawn_blocking(move || Ok(blocking::list_tags(&tool_dir)?)).await
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
    log::debug!("alias src path: {}", src_path.display());
    log::debug!("alias path: {}", alias_path.display());

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
    log::debug!("copy src path: {}", src_path.display());
    log::debug!("copy dest path: {}", dest_path.display());

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

pub fn get_bin_path(
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
    let bin_path = get_bin_path(tool, tools_base, tag)?;
    let mut command = std::process::Command::new(bin_path);
    command.args(args);
    crate::spawn_blocking(move || {
        command.spawn()?.wait()?;
        Ok(())
    })
    .await
}
