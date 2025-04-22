pub mod general_tool;
use std::path::{Path, PathBuf};

use serde::Serialize;
use smol_str::{SmolStr, SmolStrBuilder};

// Define Paths struct here
#[derive(Clone)] // Add Clone derive for use in async block
pub struct Paths {
    pub tool_dir: PathBuf,
}

pub struct ToolInfo {
    pub name: SmolStr,
    pub about: SmolStr,
    pub after_long_help: Option<SmolStr>,
    pub all_platforms: Option<Vec<SmolStr>>,
    pub default_platform: Option<SmolStr>,
    pub all_flavors: Option<Vec<SmolStr>>,
    pub default_flavor: Option<SmolStr>,
    pub version_is_major: bool,
}

pub struct Version {
    pub version: SmolStr,
    pub major_version: SmolStr,
    pub is_lts: bool,
}

pub enum InstallVersion {
    Latest { major_version: SmolStr },
    Specific { version: SmolStr },
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

#[async_trait::async_trait]
pub trait GeneralTool {
    fn info(&self) -> &ToolInfo;
    async fn fetch_versions(
        &self,
        platform: Option<SmolStr>,
        flavor: Option<SmolStr>,
        major_version: Option<SmolStr>,
    ) -> anyhow::Result<Vec<Version>>;
    async fn get_down_info(
        &self,
        platform: Option<SmolStr>,
        flavor: Option<SmolStr>,
        version: InstallVersion,
    ) -> anyhow::Result<ToolDownInfo>;
    fn bin_path(&self, instance_dir: &Path) -> anyhow::Result<PathBuf>;
}
