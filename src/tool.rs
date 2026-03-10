pub mod general_tool;
use std::{ffi::OsString, future::Future, path::PathBuf};

use serde::{Deserialize, Serialize};
use smol_str::{SmolStr, SmolStrBuilder};

pub struct ToolInfo {
    pub about: SmolStr,
    pub after_long_help: Option<SmolStr>,
    // If all_... is None, it means the tool doesn't have distinct platforms/flavors.
    pub all_platforms: Option<Vec<SmolStr>>,
    pub default_platform: Option<SmolStr>,
    pub all_flavors: Option<Vec<SmolStr>>,
    pub default_flavor: Option<SmolStr>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Version {
    pub version: SmolStr,
    #[serde(rename = "lts", default, skip_serializing_if = "is_false")]
    pub is_lts: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, Copy)]
pub struct VersionPrefix {
    pub major: u32,
    pub minor: Option<u32>,
    pub patch: Option<u32>,
}

impl VersionPrefix {
    pub fn parse(raw: &str) -> anyhow::Result<Self> {
        let mut parts = raw.split('.');
        let major = parts
            .next()
            .ok_or_else(|| anyhow::anyhow!("Version prefix is empty"))?
            .parse::<u32>()
            .map_err(|e| anyhow::anyhow!("Invalid major version in '{raw}': {e}"))?;
        let minor = parts
            .next()
            .map(|v| {
                v.parse::<u32>()
                    .map_err(|e| anyhow::anyhow!("Invalid minor version in '{raw}': {e}"))
            })
            .transpose()?;
        let patch = parts
            .next()
            .map(|v| {
                v.parse::<u32>()
                    .map_err(|e| anyhow::anyhow!("Invalid patch version in '{raw}': {e}"))
            })
            .transpose()?;
        if parts.next().is_some() {
            anyhow::bail!("Invalid version prefix '{raw}', expected at most 3 parts");
        }
        if patch.is_some() && minor.is_none() {
            anyhow::bail!("Invalid version prefix '{raw}', patch requires minor");
        }
        Ok(Self {
            major,
            minor,
            patch,
        })
    }

    pub fn matches(&self, major: u32, minor: u32, patch: u32) -> bool {
        if self.major != major {
            return false;
        }
        if self.minor.is_some_and(|m| m != minor) {
            return false;
        }
        if self.patch.is_some_and(|p| p != patch) {
            return false;
        }
        true
    }
}

/// Version filter for selecting version.
#[derive(Clone)]
pub struct VersionFilter {
    pub lts_only: bool,
    pub allow_prerelease: bool,
    pub version_prefix: Option<VersionPrefix>,
    pub exact_version: Option<SmolStr>,
}

pub struct ToolDownInfo {
    pub version: Version,
    pub url: SmolStr,
    pub hash: crate::FileHash,
}

#[derive(Serialize)]
pub struct DownInfo {
    pub tag: SmolStr,
    pub version: SmolStr,
    #[serde(rename = "lts")]
    pub is_lts: bool,
    pub url: SmolStr,
    pub hash: crate::FileHash,
}

impl DownInfo {
    pub fn from_tool_down_info(
        tool_down_info: ToolDownInfo,
        platform: Option<&str>,
        flavor: Option<&str>,
    ) -> Self {
        let mut target_tag = SmolStrBuilder::new();
        if let Some(p) = platform {
            target_tag.push_str(p);
            target_tag.push('_');
        }
        if let Some(f) = &flavor {
            target_tag.push_str(f);
            target_tag.push('_');
        }

        target_tag.push_str(&tool_down_info.version.version);

        Self {
            tag: target_tag.finish(),
            version: tool_down_info.version.version,
            is_lts: tool_down_info.version.is_lts,
            url: tool_down_info.url,
            hash: tool_down_info.hash,
        }
    }
}

pub trait GeneralTool: Send + Sync {
    fn info(&self) -> &ToolInfo;
    fn describe_flavor(&self, _flavor: &str) -> &'static str {
        "Tool-specific build flavor."
    }
    fn fetch_versions(
        &self,
        platform: Option<SmolStr>,
        flavor: Option<SmolStr>,
        version_filter: VersionFilter,
    ) -> impl Future<Output = anyhow::Result<Vec<Version>>> + Send;
    fn get_down_info(
        &self,
        platform: Option<SmolStr>,
        flavor: Option<SmolStr>,
        version_filter: VersionFilter,
    ) -> impl Future<Output = anyhow::Result<ToolDownInfo>> + Send;
    fn find_best_matching_local_tag<'a, I>(
        &self,
        tags_and_versions: I,
        version_filter: &VersionFilter,
    ) -> Option<SmolStr>
    where
        I: Iterator<Item = (&'a str, &'a Version)>;
    fn entry_path(&self, tag_dir: PathBuf) -> anyhow::Result<PathBuf>;
    fn run(
        &self,
        entry_path: PathBuf,
        args: Vec<OsString>,
    ) -> impl Future<Output = anyhow::Result<()>> + Send {
        async move {
            crate::spawn_blocking(move || {
                let mut command = std::process::Command::new(entry_path);
                command.args(args);
                command.spawn()?.wait()?;
                Ok(())
            })
            .await
        }
    }
}
