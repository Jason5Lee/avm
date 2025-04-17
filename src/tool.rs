pub mod general_tool;
use std::path::{Path, PathBuf};

use serde::Serialize;
use smol_str::SmolStr;

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

#[derive(Serialize)]
pub struct DownUrl {
    pub version: SmolStr,
    pub url: SmolStr,
    pub sha1: SmolStr,
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
    async fn get_down_url(
        &self,
        platform: Option<SmolStr>,
        flavor: Option<SmolStr>,
        version: InstallVersion,
    ) -> anyhow::Result<DownUrl>;
    fn bin_path(&self, instance_dir: &Path) -> anyhow::Result<PathBuf>;
}
