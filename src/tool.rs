pub mod general_tool;
use std::{future::Future, path::PathBuf};

use serde::Serialize;
use smol_str::{SmolStr, SmolStrBuilder};

pub struct ToolInfo {
    pub name: SmolStr,
    pub about: SmolStr,
    pub after_long_help: Option<SmolStr>,
    // If None, it means the tool doesn't have distinct platforms/flavors.
    pub all_platforms: Option<Vec<SmolStr>>,
    pub default_platform: Option<SmolStr>,
    pub all_flavors: Option<Vec<SmolStr>>,
    pub default_flavor: Option<SmolStr>,
}

pub struct Version {
    pub version: SmolStr,
    pub major_version: SmolStr,
    pub is_lts: bool,
}

/// Version filter for selecting version.
pub struct VersionFilter {
    pub lts_only: bool,
    pub major_version: Option<SmolStr>,
    pub exact_version: Option<SmolStr>,
}

pub struct ToolDownInfo {
    pub version: SmolStr,
    pub url: SmolStr,
    pub hash: crate::FileHash,
}

#[derive(Serialize)]
pub struct DownInfo {
    pub tag: SmolStr,
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

        target_tag.push_str(&tool_down_info.version);

        Self {
            tag: target_tag.finish(),
            url: tool_down_info.url,
            hash: tool_down_info.hash,
        }
    }
}

pub trait GeneralTool: Send + Sync {
    fn info(&self) -> &ToolInfo;
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
    fn exe_path(&self, tag_dir: PathBuf) -> anyhow::Result<PathBuf>;
}
