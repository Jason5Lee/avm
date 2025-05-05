use anyhow::Context;
use fxhash::FxHashSet;
use serde::Deserialize;
use smol_str::{SmolStr, ToSmolStr};
use std::path::PathBuf;
use std::sync::Arc;

use crate::HttpClient;
use crate::{
    platform::{cpu, create_platform_string, current_cpu, current_os, os},
    tool::{ToolDownInfo, ToolInfo, Version, VersionFilter},
};

pub struct Tool {
    client: Arc<HttpClient>,
    info: ToolInfo,
    corresponding_file_dto_and_archive_suffix: Vec<(&'static str, &'static str)>,
}

const BASE_URL: &str = "https://nodejs.org/dist/";

impl crate::tool::GeneralTool for Tool {
    fn info(&self) -> &ToolInfo {
        &self.info
    }

    async fn fetch_versions(
        &self,
        platform: Option<SmolStr>,
        _flavor: Option<SmolStr>,
        version_filter: VersionFilter,
    ) -> anyhow::Result<Vec<Version>> {
        let platform = platform.ok_or_else(|| anyhow::anyhow!("Platform is required"))?;
        let (file_dto, _) = self.get_file_dto_and_archive_suffix(&platform);
        let version_filter = NodeVersionFilter::try_from(version_filter)?;

        let mut releases = self
            .fetch_node_releases(&self.client)
            .await?
            .into_iter()
            .filter_map(|r| {
                let (version_raw, version) = parse_node_version(&r.version)
                    .map_err(|e| log::error!("Failed to parse Node version: {}", e))
                    .ok()?;
                let lts = r.lts.is();

                if !version_filter.verify(version_raw, &version, lts) {
                    return None;
                }
                if !r.files.iter().any(|f| f == file_dto) {
                    return None;
                }
                Some((version, SmolStr::from(version_raw), lts))
            })
            .collect::<Vec<_>>();
        releases.sort_by(|a, b| a.0.cmp(&b.0));
        let mut versions = Vec::new();
        let mut version_set = FxHashSet::default();
        for release in releases {
            let version_raw = release.1;
            if version_set.insert(version_raw.clone()) {
                versions.push(Version {
                    version: version_raw,
                    major_version: release.0.major.to_smolstr(),
                    is_lts: release.2,
                });
            }
        }

        Ok(versions)
    }

    async fn get_down_info(
        &self,
        platform: Option<SmolStr>,
        _flavor: Option<SmolStr>,
        version: VersionFilter,
    ) -> anyhow::Result<ToolDownInfo> {
        let platform = platform.ok_or_else(|| anyhow::anyhow!("Platform is required"))?;
        let (file_dto, archive_suffix) = self.get_file_dto_and_archive_suffix(&platform);
        let version_filter = NodeVersionFilter::try_from(version)?;

        let release = self
            .fetch_node_releases(&self.client)
            .await?
            .into_iter()
            .filter_map(|r| {
                let (version_raw, version) = parse_node_version(&r.version)
                    .map_err(|e| log::error!("Failed to parse Node version: {}", e))
                    .ok()?;

                if !version_filter.verify(version_raw, &version, r.lts.is()) {
                    return None;
                }
                if !r.files.iter().any(|f| f == file_dto) {
                    return None;
                }
                Some((version, SmolStr::from(version_raw)))
            })
            .max_by(|a, b| a.0.cmp(&b.0));
        match release {
            Some((_, version_raw)) => {
                // Read the shasum file non-streamingly because it's not large.
                let url_dir = format!("{}/v{}", BASE_URL, version_raw);
                let sha256_content = self
                    .client
                    .get(&format!("{}/SHASUMS256.txt", url_dir))
                    .send()
                    .await?
                    .text()
                    .await?;
                let file_name = format!("node-v{}-{}", version_raw, archive_suffix);
                let sha256 = sha256_content
                    .lines()
                    .filter_map(|line| {
                        let mut split = line.split_whitespace();
                        let sha256 = split.next()?;
                        let filename = split.next()?;
                        if filename == file_name {
                            Some(SmolStr::from(sha256))
                        } else {
                            None
                        }
                    })
                    .next();
                if sha256.is_none() {
                    log::warn!("No sha256 found");
                }

                let url = smol_str::format_smolstr!("{}/{}", url_dir, file_name);
                Ok(ToolDownInfo {
                    version: version_raw,
                    url,
                    hash: crate::FileHash {
                        sha256,
                        ..Default::default()
                    },
                })
            }
            None => Err(anyhow::anyhow!("No download URL found.")),
        }
    }

    fn exe_path(&self, tag_dir: PathBuf) -> anyhow::Result<PathBuf> {
        let mut p = tag_dir;
        p.push("bin");
        #[cfg(windows)]
        p.push("node.exe");
        #[cfg(not(windows))]
        p.push("node");
        Ok(p)
    }
}

impl Tool {
    pub fn new(client: Arc<HttpClient>) -> Self {
        let (all_platforms, corresponding_file_dto_and_archive_suffix) =
            Self::get_platforms_and_corresponding_file_dto_and_archive_suffix();

        let default_platform = current_cpu().and_then(|cpu| {
            let os = current_os()?;
            let p = create_platform_string(cpu, os);
            all_platforms.iter().find(|&k| p == *k).cloned()
        });

        Tool {
            client,
            info: ToolInfo {
                name: "node".into(),
                about: "Node.js JavaScript runtime".into(),
                after_long_help: None,
                all_platforms: Some(all_platforms),
                default_platform,
                all_flavors: None,
                default_flavor: None,
            },
            corresponding_file_dto_and_archive_suffix,
        }
    }

    #[rustfmt::skip]
    fn get_platforms_and_corresponding_file_dto_and_archive_suffix(
    ) -> (Vec<SmolStr>, Vec<(&'static str, &'static str)>) {
        let mut platforms = Vec::new();
        let mut file_dto_and_archive_suffix = Vec::new();

        let mut add =
            |cpu: &str, os: &str, file_dto: &'static str, archive_suffix: &'static str| {
                platforms.push(create_platform_string(cpu, os));
                file_dto_and_archive_suffix.push((file_dto, archive_suffix));
            };

        // --- Linux ---
        add(cpu::X64, os::LINUX, "linux-x64", "linux-x64.tar.xz");
        add(cpu::X86, os::LINUX, "linux-x86", "linux-x86.tar.xz");
        add(cpu::ARM64, os::LINUX, "linux-arm64", "linux-arm64.tar.xz");
        add(cpu::ARMV6L, os::LINUX, "linux-armv6l", "linux-armv6l.tar.xz");
        add(cpu::ARMV7L, os::LINUX, "linux-armv7l", "linux-armv7l.tar.xz");
        add(cpu::PPC64LE, os::LINUX, "linux-ppc64le", "linux-ppc64le.tar.xz");
        add(cpu::S390X, os::LINUX, "linux-s390x", "linux-s390x.tar.xz");

        // --- Windows ---
        add(cpu::X64, os::WIN, "win-x64-zip", "win-x64.zip");
        add(cpu::X86, os::WIN, "win-x86-zip", "win-x86.zip");
        add(cpu::ARM64, os::WIN, "win-arm64-zip", "win-arm64.zip");

        // --- macOS (Darwin) ---
        add(cpu::ARM64, os::MAC, "osx-arm64-tar", "darwin-arm64.tar.xz");
        add(cpu::X64, os::MAC, "osx-x64-tar", "darwin-x64.tar.xz");
        add(cpu::X86, os::MAC, "osx-x86-tar", "darwin-x86.tar.xz");

        // Others
        add(cpu::X64, os::SOLARIS, "sunos-x64", "sunos-x64.tar.xz");
        add(cpu::X86, os::SOLARIS, "sunos-x86", "sunos-x86.tar.xz");
        add(cpu::PPC64, os::AIX, "aix-ppc64", "aix-ppc64.tar.gz");

        (platforms, file_dto_and_archive_suffix)
    }

    fn get_file_dto_and_archive_suffix(&self, platform: &SmolStr) -> (&'static str, &'static str) {
        let platform_index = self
            .info
            .all_platforms
            .as_ref()
            .unwrap()
            .iter()
            .position(|p| p == platform)
            .unwrap();
        self.corresponding_file_dto_and_archive_suffix[platform_index]
    }

    async fn fetch_node_releases(&self, client: &HttpClient) -> reqwest::Result<Vec<ReleaseDto>> {
        client
            .get(&format!("{BASE_URL}index.json"))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
    }
}

#[allow(dead_code)] // value in `String` is not used, but required for deserialization
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum LtsDto {
    String(SmolStr),
    Bool(bool),
}

impl LtsDto {
    fn is(&self) -> bool {
        match self {
            LtsDto::String(_) => true,
            LtsDto::Bool(b) => *b,
        }
    }
}

#[derive(Debug, Deserialize)]
struct ReleaseDto {
    version: SmolStr,
    lts: LtsDto,
    files: Vec<SmolStr>,
}

/// Represents a parsed Node.js version.
#[derive(PartialOrd, Ord, Debug, PartialEq, Eq, Clone)]
pub struct NodeVersion {
    major: u32,
    minor: u32,
    patch: u32,
}

struct NodeVersionFilter {
    lts_only: bool,
    major_version: Option<u32>,
    exact_version: Option<SmolStr>,
}

impl NodeVersionFilter {
    fn verify(&self, raw_version: &str, version: &NodeVersion, is_lts: bool) -> bool {
        if self.lts_only && !is_lts {
            return false;
        }
        if self
            .major_version
            .is_some_and(|major_version| version.major != major_version)
        {
            return false;
        }
        if self
            .exact_version
            .as_ref()
            .is_some_and(|exact_version| raw_version != exact_version)
        {
            return false;
        }
        true
    }
}

impl TryFrom<VersionFilter> for NodeVersionFilter {
    type Error = anyhow::Error;

    fn try_from(value: VersionFilter) -> Result<Self, Self::Error> {
        Ok(Self {
            lts_only: value.lts_only,
            major_version: value
                .major_version
                .map(|v| v.parse::<u32>().context("Invalid major version"))
                .transpose()?,
            exact_version: value.exact_version,
        })
    }
}
/// Parses a Node.js version string and returns the trimmed version string and parsed NodeVersion.
pub fn parse_node_version(s: &str) -> anyhow::Result<(&str, NodeVersion)> {
    // Remove 'v' prefix if present
    let raw_version = s.strip_prefix('v').unwrap_or(s);
    if raw_version.is_empty() {
        return Err(anyhow::anyhow!("Input string '{}' has no version part", s));
    }

    // Split into major.minor.patch
    let parts: Vec<&str> = raw_version.split('.').collect();
    if parts.len() != 3 {
        return Err(anyhow::anyhow!("Invalid version format: {}", s));
    }

    // Parse major version
    let major = parts[0]
        .parse::<u32>()
        .map_err(|e| anyhow::anyhow!("Invalid major version '{}' in '{}': {}", parts[0], s, e))?;

    // Parse minor version
    let minor = parts[1]
        .parse::<u32>()
        .map_err(|e| anyhow::anyhow!("Invalid minor version '{}' in '{}': {}", parts[1], s, e))?;

    // Parse patch version
    let patch = parts[2]
        .parse::<u32>()
        .map_err(|e| anyhow::anyhow!("Invalid patch version '{}' in '{}': {}", parts[2], s, e))?;

    Ok((
        raw_version,
        NodeVersion {
            major,
            minor,
            patch,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[rustfmt::skip]
    fn test_parse_node_version() {
        assert_eq!(parse_node_version("v23.11.0").unwrap(), ("23.11.0", NodeVersion { major: 23, minor: 11, patch: 0 }));
        assert_eq!(parse_node_version("v23.10.0").unwrap(), ("23.10.0", NodeVersion { major: 23, minor: 10, patch: 0 }));
        assert_eq!(parse_node_version("v23.9.0").unwrap(), ("23.9.0", NodeVersion { major: 23, minor: 9, patch: 0 }));
        assert_eq!(parse_node_version("v23.8.0").unwrap(), ("23.8.0", NodeVersion { major: 23, minor: 8, patch: 0 }));
        assert_eq!(parse_node_version("v23.7.0").unwrap(), ("23.7.0", NodeVersion { major: 23, minor: 7, patch: 0 }));
        assert_eq!(parse_node_version("v23.6.1").unwrap(), ("23.6.1", NodeVersion { major: 23, minor: 6, patch: 1 }));
        assert_eq!(parse_node_version("v23.6.0").unwrap(), ("23.6.0", NodeVersion { major: 23, minor: 6, patch: 0 }));
        assert_eq!(parse_node_version("v23.5.0").unwrap(), ("23.5.0", NodeVersion { major: 23, minor: 5, patch: 0 }));
        assert_eq!(parse_node_version("v23.4.0").unwrap(), ("23.4.0", NodeVersion { major: 23, minor: 4, patch: 0 }));
        assert_eq!(parse_node_version("v23.3.0").unwrap(), ("23.3.0", NodeVersion { major: 23, minor: 3, patch: 0 }));
        assert_eq!(parse_node_version("v23.2.0").unwrap(), ("23.2.0", NodeVersion { major: 23, minor: 2, patch: 0 }));
        assert_eq!(parse_node_version("v23.1.0").unwrap(), ("23.1.0", NodeVersion { major: 23, minor: 1, patch: 0 }));
        assert_eq!(parse_node_version("v23.0.0").unwrap(), ("23.0.0", NodeVersion { major: 23, minor: 0, patch: 0 }));
        assert_eq!(parse_node_version("v22.15.0").unwrap(), ("22.15.0", NodeVersion { major: 22, minor: 15, patch: 0 }));
        assert_eq!(parse_node_version("v22.14.0").unwrap(), ("22.14.0", NodeVersion { major: 22, minor: 14, patch: 0 }));
        assert_eq!(parse_node_version("v22.13.1").unwrap(), ("22.13.1", NodeVersion { major: 22, minor: 13, patch: 1 }));
        assert_eq!(parse_node_version("v22.13.0").unwrap(), ("22.13.0", NodeVersion { major: 22, minor: 13, patch: 0 }));
        assert_eq!(parse_node_version("v22.12.0").unwrap(), ("22.12.0", NodeVersion { major: 22, minor: 12, patch: 0 }));
        assert_eq!(parse_node_version("v22.11.0").unwrap(), ("22.11.0", NodeVersion { major: 22, minor: 11, patch: 0 }));
        assert_eq!(parse_node_version("v22.10.0").unwrap(), ("22.10.0", NodeVersion { major: 22, minor: 10, patch: 0 }));
        assert_eq!(parse_node_version("v22.9.0").unwrap(), ("22.9.0", NodeVersion { major: 22, minor: 9, patch: 0 }));
        assert_eq!(parse_node_version("v22.8.0").unwrap(), ("22.8.0", NodeVersion { major: 22, minor: 8, patch: 0 }));
        assert_eq!(parse_node_version("v22.7.0").unwrap(), ("22.7.0", NodeVersion { major: 22, minor: 7, patch: 0 }));
        assert_eq!(parse_node_version("v22.6.0").unwrap(), ("22.6.0", NodeVersion { major: 22, minor: 6, patch: 0 }));
        assert_eq!(parse_node_version("v22.5.1").unwrap(), ("22.5.1", NodeVersion { major: 22, minor: 5, patch: 1 }));
        assert_eq!(parse_node_version("v22.5.0").unwrap(), ("22.5.0", NodeVersion { major: 22, minor: 5, patch: 0 }));
        assert_eq!(parse_node_version("v22.4.1").unwrap(), ("22.4.1", NodeVersion { major: 22, minor: 4, patch: 1 }));
        assert_eq!(parse_node_version("v22.4.0").unwrap(), ("22.4.0", NodeVersion { major: 22, minor: 4, patch: 0 }));
        assert_eq!(parse_node_version("v22.3.0").unwrap(), ("22.3.0", NodeVersion { major: 22, minor: 3, patch: 0 }));
        assert_eq!(parse_node_version("v22.2.0").unwrap(), ("22.2.0", NodeVersion { major: 22, minor: 2, patch: 0 }));
        assert_eq!(parse_node_version("v22.1.0").unwrap(), ("22.1.0", NodeVersion { major: 22, minor: 1, patch: 0 }));
        assert_eq!(parse_node_version("v22.0.0").unwrap(), ("22.0.0", NodeVersion { major: 22, minor: 0, patch: 0 }));
        assert_eq!(parse_node_version("v21.7.3").unwrap(), ("21.7.3", NodeVersion { major: 21, minor: 7, patch: 3 }));
        assert_eq!(parse_node_version("v21.7.2").unwrap(), ("21.7.2", NodeVersion { major: 21, minor: 7, patch: 2 }));
        assert_eq!(parse_node_version("v21.7.1").unwrap(), ("21.7.1", NodeVersion { major: 21, minor: 7, patch: 1 }));
        assert_eq!(parse_node_version("v21.7.0").unwrap(), ("21.7.0", NodeVersion { major: 21, minor: 7, patch: 0 }));
        assert_eq!(parse_node_version("v21.6.2").unwrap(), ("21.6.2", NodeVersion { major: 21, minor: 6, patch: 2 }));
        assert_eq!(parse_node_version("v21.6.1").unwrap(), ("21.6.1", NodeVersion { major: 21, minor: 6, patch: 1 }));
        assert_eq!(parse_node_version("v21.6.0").unwrap(), ("21.6.0", NodeVersion { major: 21, minor: 6, patch: 0 }));
        assert_eq!(parse_node_version("v21.5.0").unwrap(), ("21.5.0", NodeVersion { major: 21, minor: 5, patch: 0 }));
        assert_eq!(parse_node_version("v21.4.0").unwrap(), ("21.4.0", NodeVersion { major: 21, minor: 4, patch: 0 }));
        assert_eq!(parse_node_version("v21.3.0").unwrap(), ("21.3.0", NodeVersion { major: 21, minor: 3, patch: 0 }));
        assert_eq!(parse_node_version("v21.2.0").unwrap(), ("21.2.0", NodeVersion { major: 21, minor: 2, patch: 0 }));
        assert_eq!(parse_node_version("v21.1.0").unwrap(), ("21.1.0", NodeVersion { major: 21, minor: 1, patch: 0 }));
        assert_eq!(parse_node_version("v21.0.0").unwrap(), ("21.0.0", NodeVersion { major: 21, minor: 0, patch: 0 }));
        assert_eq!(parse_node_version("v20.19.1").unwrap(), ("20.19.1", NodeVersion { major: 20, minor: 19, patch: 1 }));
        assert_eq!(parse_node_version("v20.19.0").unwrap(), ("20.19.0", NodeVersion { major: 20, minor: 19, patch: 0 }));
        assert_eq!(parse_node_version("v20.18.3").unwrap(), ("20.18.3", NodeVersion { major: 20, minor: 18, patch: 3 }));
        assert_eq!(parse_node_version("v20.18.2").unwrap(), ("20.18.2", NodeVersion { major: 20, minor: 18, patch: 2 }));
        assert_eq!(parse_node_version("v20.18.1").unwrap(), ("20.18.1", NodeVersion { major: 20, minor: 18, patch: 1 }));
        assert_eq!(parse_node_version("v20.18.0").unwrap(), ("20.18.0", NodeVersion { major: 20, minor: 18, patch: 0 }));
        assert_eq!(parse_node_version("v20.17.0").unwrap(), ("20.17.0", NodeVersion { major: 20, minor: 17, patch: 0 }));
        assert_eq!(parse_node_version("v20.16.0").unwrap(), ("20.16.0", NodeVersion { major: 20, minor: 16, patch: 0 }));
        assert_eq!(parse_node_version("v20.15.1").unwrap(), ("20.15.1", NodeVersion { major: 20, minor: 15, patch: 1 }));
        assert_eq!(parse_node_version("v20.15.0").unwrap(), ("20.15.0", NodeVersion { major: 20, minor: 15, patch: 0 }));
        assert_eq!(parse_node_version("v20.14.0").unwrap(), ("20.14.0", NodeVersion { major: 20, minor: 14, patch: 0 }));
        assert_eq!(parse_node_version("v20.13.1").unwrap(), ("20.13.1", NodeVersion { major: 20, minor: 13, patch: 1 }));
        assert_eq!(parse_node_version("v20.13.0").unwrap(), ("20.13.0", NodeVersion { major: 20, minor: 13, patch: 0 }));
        assert_eq!(parse_node_version("v20.12.2").unwrap(), ("20.12.2", NodeVersion { major: 20, minor: 12, patch: 2 }));
        assert_eq!(parse_node_version("v20.12.1").unwrap(), ("20.12.1", NodeVersion { major: 20, minor: 12, patch: 1 }));
        assert_eq!(parse_node_version("v20.12.0").unwrap(), ("20.12.0", NodeVersion { major: 20, minor: 12, patch: 0 }));
        assert_eq!(parse_node_version("v20.11.1").unwrap(), ("20.11.1", NodeVersion { major: 20, minor: 11, patch: 1 }));
        assert_eq!(parse_node_version("v20.11.0").unwrap(), ("20.11.0", NodeVersion { major: 20, minor: 11, patch: 0 }));
        assert_eq!(parse_node_version("v20.10.0").unwrap(), ("20.10.0", NodeVersion { major: 20, minor: 10, patch: 0 }));
        assert_eq!(parse_node_version("v20.9.0").unwrap(), ("20.9.0", NodeVersion { major: 20, minor: 9, patch: 0 }));
        assert_eq!(parse_node_version("v20.8.1").unwrap(), ("20.8.1", NodeVersion { major: 20, minor: 8, patch: 1 }));
        assert_eq!(parse_node_version("v20.8.0").unwrap(), ("20.8.0", NodeVersion { major: 20, minor: 8, patch: 0 }));
        assert_eq!(parse_node_version("v20.7.0").unwrap(), ("20.7.0", NodeVersion { major: 20, minor: 7, patch: 0 }));
        assert_eq!(parse_node_version("v20.6.1").unwrap(), ("20.6.1", NodeVersion { major: 20, minor: 6, patch: 1 }));
        assert_eq!(parse_node_version("v20.6.0").unwrap(), ("20.6.0", NodeVersion { major: 20, minor: 6, patch: 0 }));
        assert_eq!(parse_node_version("v20.5.1").unwrap(), ("20.5.1", NodeVersion { major: 20, minor: 5, patch: 1 }));
        assert_eq!(parse_node_version("v20.5.0").unwrap(), ("20.5.0", NodeVersion { major: 20, minor: 5, patch: 0 }));
        assert_eq!(parse_node_version("v20.4.0").unwrap(), ("20.4.0", NodeVersion { major: 20, minor: 4, patch: 0 }));
        assert_eq!(parse_node_version("v20.3.1").unwrap(), ("20.3.1", NodeVersion { major: 20, minor: 3, patch: 1 }));
        assert_eq!(parse_node_version("v20.3.0").unwrap(), ("20.3.0", NodeVersion { major: 20, minor: 3, patch: 0 }));
        assert_eq!(parse_node_version("v20.2.0").unwrap(), ("20.2.0", NodeVersion { major: 20, minor: 2, patch: 0 }));
        assert_eq!(parse_node_version("v20.1.0").unwrap(), ("20.1.0", NodeVersion { major: 20, minor: 1, patch: 0 }));
        assert_eq!(parse_node_version("v20.0.0").unwrap(), ("20.0.0", NodeVersion { major: 20, minor: 0, patch: 0 }));
        assert_eq!(parse_node_version("v19.9.0").unwrap(), ("19.9.0", NodeVersion { major: 19, minor: 9, patch: 0 }));
        assert_eq!(parse_node_version("v19.8.1").unwrap(), ("19.8.1", NodeVersion { major: 19, minor: 8, patch: 1 }));
        assert_eq!(parse_node_version("v19.8.0").unwrap(), ("19.8.0", NodeVersion { major: 19, minor: 8, patch: 0 }));
        assert_eq!(parse_node_version("v19.7.0").unwrap(), ("19.7.0", NodeVersion { major: 19, minor: 7, patch: 0 }));
        assert_eq!(parse_node_version("v19.6.1").unwrap(), ("19.6.1", NodeVersion { major: 19, minor: 6, patch: 1 }));
        assert_eq!(parse_node_version("v19.6.0").unwrap(), ("19.6.0", NodeVersion { major: 19, minor: 6, patch: 0 }));
        assert_eq!(parse_node_version("v19.5.0").unwrap(), ("19.5.0", NodeVersion { major: 19, minor: 5, patch: 0 }));
        assert_eq!(parse_node_version("v19.4.0").unwrap(), ("19.4.0", NodeVersion { major: 19, minor: 4, patch: 0 }));
        assert_eq!(parse_node_version("v19.3.0").unwrap(), ("19.3.0", NodeVersion { major: 19, minor: 3, patch: 0 }));
        assert_eq!(parse_node_version("v19.2.0").unwrap(), ("19.2.0", NodeVersion { major: 19, minor: 2, patch: 0 }));
        assert_eq!(parse_node_version("v19.1.0").unwrap(), ("19.1.0", NodeVersion { major: 19, minor: 1, patch: 0 }));
        assert_eq!(parse_node_version("v19.0.1").unwrap(), ("19.0.1", NodeVersion { major: 19, minor: 0, patch: 1 }));
        assert_eq!(parse_node_version("v19.0.0").unwrap(), ("19.0.0", NodeVersion { major: 19, minor: 0, patch: 0 }));
        assert_eq!(parse_node_version("v18.20.8").unwrap(), ("18.20.8", NodeVersion { major: 18, minor: 20, patch: 8 }));
        assert_eq!(parse_node_version("v18.20.7").unwrap(), ("18.20.7", NodeVersion { major: 18, minor: 20, patch: 7 }));
        assert_eq!(parse_node_version("v18.20.6").unwrap(), ("18.20.6", NodeVersion { major: 18, minor: 20, patch: 6 }));
        assert_eq!(parse_node_version("v18.20.5").unwrap(), ("18.20.5", NodeVersion { major: 18, minor: 20, patch: 5 }));
        assert_eq!(parse_node_version("v18.20.4").unwrap(), ("18.20.4", NodeVersion { major: 18, minor: 20, patch: 4 }));
        assert_eq!(parse_node_version("v18.20.3").unwrap(), ("18.20.3", NodeVersion { major: 18, minor: 20, patch: 3 }));
        assert_eq!(parse_node_version("v18.20.2").unwrap(), ("18.20.2", NodeVersion { major: 18, minor: 20, patch: 2 }));
        assert_eq!(parse_node_version("v18.20.1").unwrap(), ("18.20.1", NodeVersion { major: 18, minor: 20, patch: 1 }));
        assert_eq!(parse_node_version("v18.20.0").unwrap(), ("18.20.0", NodeVersion { major: 18, minor: 20, patch: 0 }));
        assert_eq!(parse_node_version("v18.19.1").unwrap(), ("18.19.1", NodeVersion { major: 18, minor: 19, patch: 1 }));
        assert_eq!(parse_node_version("v18.19.0").unwrap(), ("18.19.0", NodeVersion { major: 18, minor: 19, patch: 0 }));
        assert_eq!(parse_node_version("v18.18.2").unwrap(), ("18.18.2", NodeVersion { major: 18, minor: 18, patch: 2 }));
        assert_eq!(parse_node_version("v18.18.1").unwrap(), ("18.18.1", NodeVersion { major: 18, minor: 18, patch: 1 }));
        assert_eq!(parse_node_version("v18.18.0").unwrap(), ("18.18.0", NodeVersion { major: 18, minor: 18, patch: 0 }));
        assert_eq!(parse_node_version("v18.17.1").unwrap(), ("18.17.1", NodeVersion { major: 18, minor: 17, patch: 1 }));
        assert_eq!(parse_node_version("v18.17.0").unwrap(), ("18.17.0", NodeVersion { major: 18, minor: 17, patch: 0 }));
        assert_eq!(parse_node_version("v18.16.1").unwrap(), ("18.16.1", NodeVersion { major: 18, minor: 16, patch: 1 }));
        assert_eq!(parse_node_version("v18.16.0").unwrap(), ("18.16.0", NodeVersion { major: 18, minor: 16, patch: 0 }));
        assert_eq!(parse_node_version("v18.15.0").unwrap(), ("18.15.0", NodeVersion { major: 18, minor: 15, patch: 0 }));
        assert_eq!(parse_node_version("v18.14.2").unwrap(), ("18.14.2", NodeVersion { major: 18, minor: 14, patch: 2 }));
        assert_eq!(parse_node_version("v18.14.1").unwrap(), ("18.14.1", NodeVersion { major: 18, minor: 14, patch: 1 }));
        assert_eq!(parse_node_version("v18.14.0").unwrap(), ("18.14.0", NodeVersion { major: 18, minor: 14, patch: 0 }));
        assert_eq!(parse_node_version("v18.13.0").unwrap(), ("18.13.0", NodeVersion { major: 18, minor: 13, patch: 0 }));
        assert_eq!(parse_node_version("v18.12.1").unwrap(), ("18.12.1", NodeVersion { major: 18, minor: 12, patch: 1 }));
        assert_eq!(parse_node_version("v18.12.0").unwrap(), ("18.12.0", NodeVersion { major: 18, minor: 12, patch: 0 }));
        assert_eq!(parse_node_version("v18.11.0").unwrap(), ("18.11.0", NodeVersion { major: 18, minor: 11, patch: 0 }));
        assert_eq!(parse_node_version("v18.10.0").unwrap(), ("18.10.0", NodeVersion { major: 18, minor: 10, patch: 0 }));
        assert_eq!(parse_node_version("v18.9.1").unwrap(), ("18.9.1", NodeVersion { major: 18, minor: 9, patch: 1 }));
        assert_eq!(parse_node_version("v18.9.0").unwrap(), ("18.9.0", NodeVersion { major: 18, minor: 9, patch: 0 }));
        assert_eq!(parse_node_version("v18.8.0").unwrap(), ("18.8.0", NodeVersion { major: 18, minor: 8, patch: 0 }));
        assert_eq!(parse_node_version("v18.7.0").unwrap(), ("18.7.0", NodeVersion { major: 18, minor: 7, patch: 0 }));
        assert_eq!(parse_node_version("v18.6.0").unwrap(), ("18.6.0", NodeVersion { major: 18, minor: 6, patch: 0 }));
        assert_eq!(parse_node_version("v18.5.0").unwrap(), ("18.5.0", NodeVersion { major: 18, minor: 5, patch: 0 }));
        assert_eq!(parse_node_version("v18.4.0").unwrap(), ("18.4.0", NodeVersion { major: 18, minor: 4, patch: 0 }));
        assert_eq!(parse_node_version("v18.3.0").unwrap(), ("18.3.0", NodeVersion { major: 18, minor: 3, patch: 0 }));
        assert_eq!(parse_node_version("v18.2.0").unwrap(), ("18.2.0", NodeVersion { major: 18, minor: 2, patch: 0 }));
        assert_eq!(parse_node_version("v18.1.0").unwrap(), ("18.1.0", NodeVersion { major: 18, minor: 1, patch: 0 }));
        assert_eq!(parse_node_version("v18.0.0").unwrap(), ("18.0.0", NodeVersion { major: 18, minor: 0, patch: 0 }));
        assert_eq!(parse_node_version("v17.9.1").unwrap(), ("17.9.1", NodeVersion { major: 17, minor: 9, patch: 1 }));
        assert_eq!(parse_node_version("v17.9.0").unwrap(), ("17.9.0", NodeVersion { major: 17, minor: 9, patch: 0 }));
        assert_eq!(parse_node_version("v17.8.0").unwrap(), ("17.8.0", NodeVersion { major: 17, minor: 8, patch: 0 }));
        assert_eq!(parse_node_version("v17.7.2").unwrap(), ("17.7.2", NodeVersion { major: 17, minor: 7, patch: 2 }));
        assert_eq!(parse_node_version("v17.7.1").unwrap(), ("17.7.1", NodeVersion { major: 17, minor: 7, patch: 1 }));
        assert_eq!(parse_node_version("v17.7.0").unwrap(), ("17.7.0", NodeVersion { major: 17, minor: 7, patch: 0 }));
        assert_eq!(parse_node_version("v17.6.0").unwrap(), ("17.6.0", NodeVersion { major: 17, minor: 6, patch: 0 }));
        assert_eq!(parse_node_version("v17.5.0").unwrap(), ("17.5.0", NodeVersion { major: 17, minor: 5, patch: 0 }));
        assert_eq!(parse_node_version("v17.4.0").unwrap(), ("17.4.0", NodeVersion { major: 17, minor: 4, patch: 0 }));
        assert_eq!(parse_node_version("v17.3.1").unwrap(), ("17.3.1", NodeVersion { major: 17, minor: 3, patch: 1 }));
        assert_eq!(parse_node_version("v17.3.0").unwrap(), ("17.3.0", NodeVersion { major: 17, minor: 3, patch: 0 }));
        assert_eq!(parse_node_version("v17.2.0").unwrap(), ("17.2.0", NodeVersion { major: 17, minor: 2, patch: 0 }));
        assert_eq!(parse_node_version("v17.1.0").unwrap(), ("17.1.0", NodeVersion { major: 17, minor: 1, patch: 0 }));
        assert_eq!(parse_node_version("v17.0.1").unwrap(), ("17.0.1", NodeVersion { major: 17, minor: 0, patch: 1 }));
        assert_eq!(parse_node_version("v17.0.0").unwrap(), ("17.0.0", NodeVersion { major: 17, minor: 0, patch: 0 }));
        assert_eq!(parse_node_version("v16.20.2").unwrap(), ("16.20.2", NodeVersion { major: 16, minor: 20, patch: 2 }));
        assert_eq!(parse_node_version("v16.20.1").unwrap(), ("16.20.1", NodeVersion { major: 16, minor: 20, patch: 1 }));
        assert_eq!(parse_node_version("v16.20.0").unwrap(), ("16.20.0", NodeVersion { major: 16, minor: 20, patch: 0 }));
        assert_eq!(parse_node_version("v16.19.1").unwrap(), ("16.19.1", NodeVersion { major: 16, minor: 19, patch: 1 }));
        assert_eq!(parse_node_version("v16.19.0").unwrap(), ("16.19.0", NodeVersion { major: 16, minor: 19, patch: 0 }));
        assert_eq!(parse_node_version("v16.18.1").unwrap(), ("16.18.1", NodeVersion { major: 16, minor: 18, patch: 1 }));
        assert_eq!(parse_node_version("v16.18.0").unwrap(), ("16.18.0", NodeVersion { major: 16, minor: 18, patch: 0 }));
        assert_eq!(parse_node_version("v16.17.1").unwrap(), ("16.17.1", NodeVersion { major: 16, minor: 17, patch: 1 }));
        assert_eq!(parse_node_version("v16.17.0").unwrap(), ("16.17.0", NodeVersion { major: 16, minor: 17, patch: 0 }));
        assert_eq!(parse_node_version("v16.16.0").unwrap(), ("16.16.0", NodeVersion { major: 16, minor: 16, patch: 0 }));
        assert_eq!(parse_node_version("v16.15.1").unwrap(), ("16.15.1", NodeVersion { major: 16, minor: 15, patch: 1 }));
        assert_eq!(parse_node_version("v16.15.0").unwrap(), ("16.15.0", NodeVersion { major: 16, minor: 15, patch: 0 }));
        assert_eq!(parse_node_version("v16.14.2").unwrap(), ("16.14.2", NodeVersion { major: 16, minor: 14, patch: 2 }));
        assert_eq!(parse_node_version("v16.14.1").unwrap(), ("16.14.1", NodeVersion { major: 16, minor: 14, patch: 1 }));
        assert_eq!(parse_node_version("v16.14.0").unwrap(), ("16.14.0", NodeVersion { major: 16, minor: 14, patch: 0 }));
        assert_eq!(parse_node_version("v16.13.2").unwrap(), ("16.13.2", NodeVersion { major: 16, minor: 13, patch: 2 }));
        assert_eq!(parse_node_version("v16.13.1").unwrap(), ("16.13.1", NodeVersion { major: 16, minor: 13, patch: 1 }));
        assert_eq!(parse_node_version("v16.13.0").unwrap(), ("16.13.0", NodeVersion { major: 16, minor: 13, patch: 0 }));
        assert_eq!(parse_node_version("v16.12.0").unwrap(), ("16.12.0", NodeVersion { major: 16, minor: 12, patch: 0 }));
        assert_eq!(parse_node_version("v16.11.1").unwrap(), ("16.11.1", NodeVersion { major: 16, minor: 11, patch: 1 }));
        assert_eq!(parse_node_version("v16.11.0").unwrap(), ("16.11.0", NodeVersion { major: 16, minor: 11, patch: 0 }));
        assert_eq!(parse_node_version("v16.10.0").unwrap(), ("16.10.0", NodeVersion { major: 16, minor: 10, patch: 0 }));
        assert_eq!(parse_node_version("v16.9.1").unwrap(), ("16.9.1", NodeVersion { major: 16, minor: 9, patch: 1 }));
        assert_eq!(parse_node_version("v16.9.0").unwrap(), ("16.9.0", NodeVersion { major: 16, minor: 9, patch: 0 }));
        assert_eq!(parse_node_version("v16.8.0").unwrap(), ("16.8.0", NodeVersion { major: 16, minor: 8, patch: 0 }));
        assert_eq!(parse_node_version("v16.7.0").unwrap(), ("16.7.0", NodeVersion { major: 16, minor: 7, patch: 0 }));
        assert_eq!(parse_node_version("v16.6.2").unwrap(), ("16.6.2", NodeVersion { major: 16, minor: 6, patch: 2 }));
        assert_eq!(parse_node_version("v16.6.1").unwrap(), ("16.6.1", NodeVersion { major: 16, minor: 6, patch: 1 }));
        assert_eq!(parse_node_version("v16.6.0").unwrap(), ("16.6.0", NodeVersion { major: 16, minor: 6, patch: 0 }));
        assert_eq!(parse_node_version("v16.5.0").unwrap(), ("16.5.0", NodeVersion { major: 16, minor: 5, patch: 0 }));
        assert_eq!(parse_node_version("v16.4.2").unwrap(), ("16.4.2", NodeVersion { major: 16, minor: 4, patch: 2 }));
        assert_eq!(parse_node_version("v16.4.1").unwrap(), ("16.4.1", NodeVersion { major: 16, minor: 4, patch: 1 }));
        assert_eq!(parse_node_version("v16.4.0").unwrap(), ("16.4.0", NodeVersion { major: 16, minor: 4, patch: 0 }));
        assert_eq!(parse_node_version("v16.3.0").unwrap(), ("16.3.0", NodeVersion { major: 16, minor: 3, patch: 0 }));
        assert_eq!(parse_node_version("v16.2.0").unwrap(), ("16.2.0", NodeVersion { major: 16, minor: 2, patch: 0 }));
        assert_eq!(parse_node_version("v16.1.0").unwrap(), ("16.1.0", NodeVersion { major: 16, minor: 1, patch: 0 }));
        assert_eq!(parse_node_version("v16.0.0").unwrap(), ("16.0.0", NodeVersion { major: 16, minor: 0, patch: 0 }));
        assert_eq!(parse_node_version("v15.14.0").unwrap(), ("15.14.0", NodeVersion { major: 15, minor: 14, patch: 0 }));
        assert_eq!(parse_node_version("v15.13.0").unwrap(), ("15.13.0", NodeVersion { major: 15, minor: 13, patch: 0 }));
        assert_eq!(parse_node_version("v15.12.0").unwrap(), ("15.12.0", NodeVersion { major: 15, minor: 12, patch: 0 }));
        assert_eq!(parse_node_version("v15.11.0").unwrap(), ("15.11.0", NodeVersion { major: 15, minor: 11, patch: 0 }));
        assert_eq!(parse_node_version("v15.10.0").unwrap(), ("15.10.0", NodeVersion { major: 15, minor: 10, patch: 0 }));
        assert_eq!(parse_node_version("v15.9.0").unwrap(), ("15.9.0", NodeVersion { major: 15, minor: 9, patch: 0 }));
        assert_eq!(parse_node_version("v15.8.0").unwrap(), ("15.8.0", NodeVersion { major: 15, minor: 8, patch: 0 }));
        assert_eq!(parse_node_version("v15.7.0").unwrap(), ("15.7.0", NodeVersion { major: 15, minor: 7, patch: 0 }));
        assert_eq!(parse_node_version("v15.6.0").unwrap(), ("15.6.0", NodeVersion { major: 15, minor: 6, patch: 0 }));
        assert_eq!(parse_node_version("v15.5.1").unwrap(), ("15.5.1", NodeVersion { major: 15, minor: 5, patch: 1 }));
        assert_eq!(parse_node_version("v15.5.0").unwrap(), ("15.5.0", NodeVersion { major: 15, minor: 5, patch: 0 }));
        assert_eq!(parse_node_version("v15.4.0").unwrap(), ("15.4.0", NodeVersion { major: 15, minor: 4, patch: 0 }));
        assert_eq!(parse_node_version("v15.3.0").unwrap(), ("15.3.0", NodeVersion { major: 15, minor: 3, patch: 0 }));
        assert_eq!(parse_node_version("v15.2.1").unwrap(), ("15.2.1", NodeVersion { major: 15, minor: 2, patch: 1 }));
        assert_eq!(parse_node_version("v15.2.0").unwrap(), ("15.2.0", NodeVersion { major: 15, minor: 2, patch: 0 }));
        assert_eq!(parse_node_version("v15.1.0").unwrap(), ("15.1.0", NodeVersion { major: 15, minor: 1, patch: 0 }));
        assert_eq!(parse_node_version("v15.0.1").unwrap(), ("15.0.1", NodeVersion { major: 15, minor: 0, patch: 1 }));
        assert_eq!(parse_node_version("v15.0.0").unwrap(), ("15.0.0", NodeVersion { major: 15, minor: 0, patch: 0 }));
        assert_eq!(parse_node_version("v14.21.3").unwrap(), ("14.21.3", NodeVersion { major: 14, minor: 21, patch: 3 }));
        assert_eq!(parse_node_version("v14.21.2").unwrap(), ("14.21.2", NodeVersion { major: 14, minor: 21, patch: 2 }));
        assert_eq!(parse_node_version("v14.21.1").unwrap(), ("14.21.1", NodeVersion { major: 14, minor: 21, patch: 1 }));
        assert_eq!(parse_node_version("v14.21.0").unwrap(), ("14.21.0", NodeVersion { major: 14, minor: 21, patch: 0 }));
        assert_eq!(parse_node_version("v14.20.1").unwrap(), ("14.20.1", NodeVersion { major: 14, minor: 20, patch: 1 }));
        assert_eq!(parse_node_version("v14.20.0").unwrap(), ("14.20.0", NodeVersion { major: 14, minor: 20, patch: 0 }));
        assert_eq!(parse_node_version("v14.19.3").unwrap(), ("14.19.3", NodeVersion { major: 14, minor: 19, patch: 3 }));
        assert_eq!(parse_node_version("v14.19.2").unwrap(), ("14.19.2", NodeVersion { major: 14, minor: 19, patch: 2 }));
        assert_eq!(parse_node_version("v14.19.1").unwrap(), ("14.19.1", NodeVersion { major: 14, minor: 19, patch: 1 }));
        assert_eq!(parse_node_version("v14.19.0").unwrap(), ("14.19.0", NodeVersion { major: 14, minor: 19, patch: 0 }));
        assert_eq!(parse_node_version("v14.18.3").unwrap(), ("14.18.3", NodeVersion { major: 14, minor: 18, patch: 3 }));
        assert_eq!(parse_node_version("v14.18.2").unwrap(), ("14.18.2", NodeVersion { major: 14, minor: 18, patch: 2 }));
        assert_eq!(parse_node_version("v14.18.1").unwrap(), ("14.18.1", NodeVersion { major: 14, minor: 18, patch: 1 }));
        assert_eq!(parse_node_version("v14.18.0").unwrap(), ("14.18.0", NodeVersion { major: 14, minor: 18, patch: 0 }));
        assert_eq!(parse_node_version("v14.17.6").unwrap(), ("14.17.6", NodeVersion { major: 14, minor: 17, patch: 6 }));
        assert_eq!(parse_node_version("v14.17.5").unwrap(), ("14.17.5", NodeVersion { major: 14, minor: 17, patch: 5 }));
        assert_eq!(parse_node_version("v14.17.4").unwrap(), ("14.17.4", NodeVersion { major: 14, minor: 17, patch: 4 }));
        assert_eq!(parse_node_version("v14.17.3").unwrap(), ("14.17.3", NodeVersion { major: 14, minor: 17, patch: 3 }));
        assert_eq!(parse_node_version("v14.17.2").unwrap(), ("14.17.2", NodeVersion { major: 14, minor: 17, patch: 2 }));
        assert_eq!(parse_node_version("v14.17.1").unwrap(), ("14.17.1", NodeVersion { major: 14, minor: 17, patch: 1 }));
        assert_eq!(parse_node_version("v14.17.0").unwrap(), ("14.17.0", NodeVersion { major: 14, minor: 17, patch: 0 }));
        assert_eq!(parse_node_version("v14.16.1").unwrap(), ("14.16.1", NodeVersion { major: 14, minor: 16, patch: 1 }));
        assert_eq!(parse_node_version("v14.16.0").unwrap(), ("14.16.0", NodeVersion { major: 14, minor: 16, patch: 0 }));
        assert_eq!(parse_node_version("v14.15.5").unwrap(), ("14.15.5", NodeVersion { major: 14, minor: 15, patch: 5 }));
        assert_eq!(parse_node_version("v14.15.4").unwrap(), ("14.15.4", NodeVersion { major: 14, minor: 15, patch: 4 }));
        assert_eq!(parse_node_version("v14.15.3").unwrap(), ("14.15.3", NodeVersion { major: 14, minor: 15, patch: 3 }));
        assert_eq!(parse_node_version("v14.15.2").unwrap(), ("14.15.2", NodeVersion { major: 14, minor: 15, patch: 2 }));
        assert_eq!(parse_node_version("v14.15.1").unwrap(), ("14.15.1", NodeVersion { major: 14, minor: 15, patch: 1 }));
        assert_eq!(parse_node_version("v14.15.0").unwrap(), ("14.15.0", NodeVersion { major: 14, minor: 15, patch: 0 }));
        assert_eq!(parse_node_version("v14.14.0").unwrap(), ("14.14.0", NodeVersion { major: 14, minor: 14, patch: 0 }));
        assert_eq!(parse_node_version("v14.13.1").unwrap(), ("14.13.1", NodeVersion { major: 14, minor: 13, patch: 1 }));
        assert_eq!(parse_node_version("v14.13.0").unwrap(), ("14.13.0", NodeVersion { major: 14, minor: 13, patch: 0 }));
        assert_eq!(parse_node_version("v14.12.0").unwrap(), ("14.12.0", NodeVersion { major: 14, minor: 12, patch: 0 }));
        assert_eq!(parse_node_version("v14.11.0").unwrap(), ("14.11.0", NodeVersion { major: 14, minor: 11, patch: 0 }));
        assert_eq!(parse_node_version("v14.10.1").unwrap(), ("14.10.1", NodeVersion { major: 14, minor: 10, patch: 1 }));
        assert_eq!(parse_node_version("v14.10.0").unwrap(), ("14.10.0", NodeVersion { major: 14, minor: 10, patch: 0 }));
        assert_eq!(parse_node_version("v14.9.0").unwrap(), ("14.9.0", NodeVersion { major: 14, minor: 9, patch: 0 }));
        assert_eq!(parse_node_version("v14.8.0").unwrap(), ("14.8.0", NodeVersion { major: 14, minor: 8, patch: 0 }));
        assert_eq!(parse_node_version("v14.7.0").unwrap(), ("14.7.0", NodeVersion { major: 14, minor: 7, patch: 0 }));
        assert_eq!(parse_node_version("v14.6.0").unwrap(), ("14.6.0", NodeVersion { major: 14, minor: 6, patch: 0 }));
        assert_eq!(parse_node_version("v14.5.0").unwrap(), ("14.5.0", NodeVersion { major: 14, minor: 5, patch: 0 }));
        assert_eq!(parse_node_version("v14.4.0").unwrap(), ("14.4.0", NodeVersion { major: 14, minor: 4, patch: 0 }));
        assert_eq!(parse_node_version("v14.3.0").unwrap(), ("14.3.0", NodeVersion { major: 14, minor: 3, patch: 0 }));
        assert_eq!(parse_node_version("v14.2.0").unwrap(), ("14.2.0", NodeVersion { major: 14, minor: 2, patch: 0 }));
        assert_eq!(parse_node_version("v14.1.0").unwrap(), ("14.1.0", NodeVersion { major: 14, minor: 1, patch: 0 }));
        assert_eq!(parse_node_version("v14.0.0").unwrap(), ("14.0.0", NodeVersion { major: 14, minor: 0, patch: 0 }));
        assert_eq!(parse_node_version("v13.14.0").unwrap(), ("13.14.0", NodeVersion { major: 13, minor: 14, patch: 0 }));
        assert_eq!(parse_node_version("v13.13.0").unwrap(), ("13.13.0", NodeVersion { major: 13, minor: 13, patch: 0 }));
        assert_eq!(parse_node_version("v13.12.0").unwrap(), ("13.12.0", NodeVersion { major: 13, minor: 12, patch: 0 }));
        assert_eq!(parse_node_version("v13.11.0").unwrap(), ("13.11.0", NodeVersion { major: 13, minor: 11, patch: 0 }));
        assert_eq!(parse_node_version("v13.10.1").unwrap(), ("13.10.1", NodeVersion { major: 13, minor: 10, patch: 1 }));
        assert_eq!(parse_node_version("v13.10.0").unwrap(), ("13.10.0", NodeVersion { major: 13, minor: 10, patch: 0 }));
        assert_eq!(parse_node_version("v13.9.0").unwrap(), ("13.9.0", NodeVersion { major: 13, minor: 9, patch: 0 }));
        assert_eq!(parse_node_version("v13.8.0").unwrap(), ("13.8.0", NodeVersion { major: 13, minor: 8, patch: 0 }));
        assert_eq!(parse_node_version("v13.7.0").unwrap(), ("13.7.0", NodeVersion { major: 13, minor: 7, patch: 0 }));
        assert_eq!(parse_node_version("v13.6.0").unwrap(), ("13.6.0", NodeVersion { major: 13, minor: 6, patch: 0 }));
        assert_eq!(parse_node_version("v13.5.0").unwrap(), ("13.5.0", NodeVersion { major: 13, minor: 5, patch: 0 }));
        assert_eq!(parse_node_version("v13.4.0").unwrap(), ("13.4.0", NodeVersion { major: 13, minor: 4, patch: 0 }));
        assert_eq!(parse_node_version("v13.3.0").unwrap(), ("13.3.0", NodeVersion { major: 13, minor: 3, patch: 0 }));
        assert_eq!(parse_node_version("v13.2.0").unwrap(), ("13.2.0", NodeVersion { major: 13, minor: 2, patch: 0 }));
        assert_eq!(parse_node_version("v13.1.0").unwrap(), ("13.1.0", NodeVersion { major: 13, minor: 1, patch: 0 }));
        assert_eq!(parse_node_version("v13.0.1").unwrap(), ("13.0.1", NodeVersion { major: 13, minor: 0, patch: 1 }));
        assert_eq!(parse_node_version("v13.0.0").unwrap(), ("13.0.0", NodeVersion { major: 13, minor: 0, patch: 0 }));
        assert_eq!(parse_node_version("v12.22.12").unwrap(), ("12.22.12", NodeVersion { major: 12, minor: 22, patch: 12 }));
        assert_eq!(parse_node_version("v12.22.11").unwrap(), ("12.22.11", NodeVersion { major: 12, minor: 22, patch: 11 }));
        assert_eq!(parse_node_version("v12.22.10").unwrap(), ("12.22.10", NodeVersion { major: 12, minor: 22, patch: 10 }));
        assert_eq!(parse_node_version("v12.22.9").unwrap(), ("12.22.9", NodeVersion { major: 12, minor: 22, patch: 9 }));
        assert_eq!(parse_node_version("v12.22.8").unwrap(), ("12.22.8", NodeVersion { major: 12, minor: 22, patch: 8 }));
        assert_eq!(parse_node_version("v12.22.7").unwrap(), ("12.22.7", NodeVersion { major: 12, minor: 22, patch: 7 }));
        assert_eq!(parse_node_version("v12.22.6").unwrap(), ("12.22.6", NodeVersion { major: 12, minor: 22, patch: 6 }));
        assert_eq!(parse_node_version("v12.22.5").unwrap(), ("12.22.5", NodeVersion { major: 12, minor: 22, patch: 5 }));
        assert_eq!(parse_node_version("v12.22.4").unwrap(), ("12.22.4", NodeVersion { major: 12, minor: 22, patch: 4 }));
        assert_eq!(parse_node_version("v12.22.3").unwrap(), ("12.22.3", NodeVersion { major: 12, minor: 22, patch: 3 }));
        assert_eq!(parse_node_version("v12.22.2").unwrap(), ("12.22.2", NodeVersion { major: 12, minor: 22, patch: 2 }));
        assert_eq!(parse_node_version("v12.22.1").unwrap(), ("12.22.1", NodeVersion { major: 12, minor: 22, patch: 1 }));
        assert_eq!(parse_node_version("v12.22.0").unwrap(), ("12.22.0", NodeVersion { major: 12, minor: 22, patch: 0 }));
        assert_eq!(parse_node_version("v12.21.0").unwrap(), ("12.21.0", NodeVersion { major: 12, minor: 21, patch: 0 }));
        assert_eq!(parse_node_version("v12.20.2").unwrap(), ("12.20.2", NodeVersion { major: 12, minor: 20, patch: 2 }));
        assert_eq!(parse_node_version("v12.20.1").unwrap(), ("12.20.1", NodeVersion { major: 12, minor: 20, patch: 1 }));
        assert_eq!(parse_node_version("v12.20.0").unwrap(), ("12.20.0", NodeVersion { major: 12, minor: 20, patch: 0 }));
        assert_eq!(parse_node_version("v12.19.1").unwrap(), ("12.19.1", NodeVersion { major: 12, minor: 19, patch: 1 }));
        assert_eq!(parse_node_version("v12.19.0").unwrap(), ("12.19.0", NodeVersion { major: 12, minor: 19, patch: 0 }));
        assert_eq!(parse_node_version("v12.18.4").unwrap(), ("12.18.4", NodeVersion { major: 12, minor: 18, patch: 4 }));
        assert_eq!(parse_node_version("v12.18.3").unwrap(), ("12.18.3", NodeVersion { major: 12, minor: 18, patch: 3 }));
        assert_eq!(parse_node_version("v12.18.2").unwrap(), ("12.18.2", NodeVersion { major: 12, minor: 18, patch: 2 }));
        assert_eq!(parse_node_version("v12.18.1").unwrap(), ("12.18.1", NodeVersion { major: 12, minor: 18, patch: 1 }));
        assert_eq!(parse_node_version("v12.18.0").unwrap(), ("12.18.0", NodeVersion { major: 12, minor: 18, patch: 0 }));
        assert_eq!(parse_node_version("v12.17.0").unwrap(), ("12.17.0", NodeVersion { major: 12, minor: 17, patch: 0 }));
        assert_eq!(parse_node_version("v12.16.3").unwrap(), ("12.16.3", NodeVersion { major: 12, minor: 16, patch: 3 }));
        assert_eq!(parse_node_version("v12.16.2").unwrap(), ("12.16.2", NodeVersion { major: 12, minor: 16, patch: 2 }));
        assert_eq!(parse_node_version("v12.16.1").unwrap(), ("12.16.1", NodeVersion { major: 12, minor: 16, patch: 1 }));
        assert_eq!(parse_node_version("v12.16.0").unwrap(), ("12.16.0", NodeVersion { major: 12, minor: 16, patch: 0 }));
        assert_eq!(parse_node_version("v12.15.0").unwrap(), ("12.15.0", NodeVersion { major: 12, minor: 15, patch: 0 }));
        assert_eq!(parse_node_version("v12.14.1").unwrap(), ("12.14.1", NodeVersion { major: 12, minor: 14, patch: 1 }));
        assert_eq!(parse_node_version("v12.14.0").unwrap(), ("12.14.0", NodeVersion { major: 12, minor: 14, patch: 0 }));
        assert_eq!(parse_node_version("v12.13.1").unwrap(), ("12.13.1", NodeVersion { major: 12, minor: 13, patch: 1 }));
        assert_eq!(parse_node_version("v12.13.0").unwrap(), ("12.13.0", NodeVersion { major: 12, minor: 13, patch: 0 }));
        assert_eq!(parse_node_version("v12.12.0").unwrap(), ("12.12.0", NodeVersion { major: 12, minor: 12, patch: 0 }));
        assert_eq!(parse_node_version("v12.11.1").unwrap(), ("12.11.1", NodeVersion { major: 12, minor: 11, patch: 1 }));
        assert_eq!(parse_node_version("v12.11.0").unwrap(), ("12.11.0", NodeVersion { major: 12, minor: 11, patch: 0 }));
        assert_eq!(parse_node_version("v12.10.0").unwrap(), ("12.10.0", NodeVersion { major: 12, minor: 10, patch: 0 }));
        assert_eq!(parse_node_version("v12.9.1").unwrap(), ("12.9.1", NodeVersion { major: 12, minor: 9, patch: 1 }));
        assert_eq!(parse_node_version("v12.9.0").unwrap(), ("12.9.0", NodeVersion { major: 12, minor: 9, patch: 0 }));
        assert_eq!(parse_node_version("v12.8.1").unwrap(), ("12.8.1", NodeVersion { major: 12, minor: 8, patch: 1 }));
        assert_eq!(parse_node_version("v12.8.0").unwrap(), ("12.8.0", NodeVersion { major: 12, minor: 8, patch: 0 }));
        assert_eq!(parse_node_version("v12.7.0").unwrap(), ("12.7.0", NodeVersion { major: 12, minor: 7, patch: 0 }));
        assert_eq!(parse_node_version("v12.6.0").unwrap(), ("12.6.0", NodeVersion { major: 12, minor: 6, patch: 0 }));
        assert_eq!(parse_node_version("v12.5.0").unwrap(), ("12.5.0", NodeVersion { major: 12, minor: 5, patch: 0 }));
        assert_eq!(parse_node_version("v12.4.0").unwrap(), ("12.4.0", NodeVersion { major: 12, minor: 4, patch: 0 }));
        assert_eq!(parse_node_version("v12.3.1").unwrap(), ("12.3.1", NodeVersion { major: 12, minor: 3, patch: 1 }));
        assert_eq!(parse_node_version("v12.3.0").unwrap(), ("12.3.0", NodeVersion { major: 12, minor: 3, patch: 0 }));
        assert_eq!(parse_node_version("v12.2.0").unwrap(), ("12.2.0", NodeVersion { major: 12, minor: 2, patch: 0 }));
        assert_eq!(parse_node_version("v12.1.0").unwrap(), ("12.1.0", NodeVersion { major: 12, minor: 1, patch: 0 }));
        assert_eq!(parse_node_version("v12.0.0").unwrap(), ("12.0.0", NodeVersion { major: 12, minor: 0, patch: 0 }));
        assert_eq!(parse_node_version("v11.15.0").unwrap(), ("11.15.0", NodeVersion { major: 11, minor: 15, patch: 0 }));
        assert_eq!(parse_node_version("v11.14.0").unwrap(), ("11.14.0", NodeVersion { major: 11, minor: 14, patch: 0 }));
        assert_eq!(parse_node_version("v11.13.0").unwrap(), ("11.13.0", NodeVersion { major: 11, minor: 13, patch: 0 }));
        assert_eq!(parse_node_version("v11.12.0").unwrap(), ("11.12.0", NodeVersion { major: 11, minor: 12, patch: 0 }));
        assert_eq!(parse_node_version("v11.11.0").unwrap(), ("11.11.0", NodeVersion { major: 11, minor: 11, patch: 0 }));
        assert_eq!(parse_node_version("v11.10.1").unwrap(), ("11.10.1", NodeVersion { major: 11, minor: 10, patch: 1 }));
        assert_eq!(parse_node_version("v11.10.0").unwrap(), ("11.10.0", NodeVersion { major: 11, minor: 10, patch: 0 }));
        assert_eq!(parse_node_version("v11.9.0").unwrap(), ("11.9.0", NodeVersion { major: 11, minor: 9, patch: 0 }));
        assert_eq!(parse_node_version("v11.8.0").unwrap(), ("11.8.0", NodeVersion { major: 11, minor: 8, patch: 0 }));
        assert_eq!(parse_node_version("v11.7.0").unwrap(), ("11.7.0", NodeVersion { major: 11, minor: 7, patch: 0 }));
        assert_eq!(parse_node_version("v11.6.0").unwrap(), ("11.6.0", NodeVersion { major: 11, minor: 6, patch: 0 }));
        assert_eq!(parse_node_version("v11.5.0").unwrap(), ("11.5.0", NodeVersion { major: 11, minor: 5, patch: 0 }));
        assert_eq!(parse_node_version("v11.4.0").unwrap(), ("11.4.0", NodeVersion { major: 11, minor: 4, patch: 0 }));
        assert_eq!(parse_node_version("v11.3.0").unwrap(), ("11.3.0", NodeVersion { major: 11, minor: 3, patch: 0 }));
        assert_eq!(parse_node_version("v11.2.0").unwrap(), ("11.2.0", NodeVersion { major: 11, minor: 2, patch: 0 }));
        assert_eq!(parse_node_version("v11.1.0").unwrap(), ("11.1.0", NodeVersion { major: 11, minor: 1, patch: 0 }));
        assert_eq!(parse_node_version("v11.0.0").unwrap(), ("11.0.0", NodeVersion { major: 11, minor: 0, patch: 0 }));
        assert_eq!(parse_node_version("v10.24.1").unwrap(), ("10.24.1", NodeVersion { major: 10, minor: 24, patch: 1 }));
        assert_eq!(parse_node_version("v10.24.0").unwrap(), ("10.24.0", NodeVersion { major: 10, minor: 24, patch: 0 }));
        assert_eq!(parse_node_version("v10.23.3").unwrap(), ("10.23.3", NodeVersion { major: 10, minor: 23, patch: 3 }));
        assert_eq!(parse_node_version("v10.23.2").unwrap(), ("10.23.2", NodeVersion { major: 10, minor: 23, patch: 2 }));
        assert_eq!(parse_node_version("v10.23.1").unwrap(), ("10.23.1", NodeVersion { major: 10, minor: 23, patch: 1 }));
        assert_eq!(parse_node_version("v10.23.0").unwrap(), ("10.23.0", NodeVersion { major: 10, minor: 23, patch: 0 }));
        assert_eq!(parse_node_version("v10.22.1").unwrap(), ("10.22.1", NodeVersion { major: 10, minor: 22, patch: 1 }));
        assert_eq!(parse_node_version("v10.22.0").unwrap(), ("10.22.0", NodeVersion { major: 10, minor: 22, patch: 0 }));
        assert_eq!(parse_node_version("v10.21.0").unwrap(), ("10.21.0", NodeVersion { major: 10, minor: 21, patch: 0 }));
        assert_eq!(parse_node_version("v10.20.1").unwrap(), ("10.20.1", NodeVersion { major: 10, minor: 20, patch: 1 }));
        assert_eq!(parse_node_version("v10.20.0").unwrap(), ("10.20.0", NodeVersion { major: 10, minor: 20, patch: 0 }));
        assert_eq!(parse_node_version("v10.19.0").unwrap(), ("10.19.0", NodeVersion { major: 10, minor: 19, patch: 0 }));
        assert_eq!(parse_node_version("v10.18.1").unwrap(), ("10.18.1", NodeVersion { major: 10, minor: 18, patch: 1 }));
        assert_eq!(parse_node_version("v10.18.0").unwrap(), ("10.18.0", NodeVersion { major: 10, minor: 18, patch: 0 }));
        assert_eq!(parse_node_version("v10.17.0").unwrap(), ("10.17.0", NodeVersion { major: 10, minor: 17, patch: 0 }));
        assert_eq!(parse_node_version("v10.16.3").unwrap(), ("10.16.3", NodeVersion { major: 10, minor: 16, patch: 3 }));
        assert_eq!(parse_node_version("v10.16.2").unwrap(), ("10.16.2", NodeVersion { major: 10, minor: 16, patch: 2 }));
        assert_eq!(parse_node_version("v10.16.1").unwrap(), ("10.16.1", NodeVersion { major: 10, minor: 16, patch: 1 }));
        assert_eq!(parse_node_version("v10.16.0").unwrap(), ("10.16.0", NodeVersion { major: 10, minor: 16, patch: 0 }));
        assert_eq!(parse_node_version("v10.15.3").unwrap(), ("10.15.3", NodeVersion { major: 10, minor: 15, patch: 3 }));
        assert_eq!(parse_node_version("v10.15.2").unwrap(), ("10.15.2", NodeVersion { major: 10, minor: 15, patch: 2 }));
        assert_eq!(parse_node_version("v10.15.1").unwrap(), ("10.15.1", NodeVersion { major: 10, minor: 15, patch: 1 }));
        assert_eq!(parse_node_version("v10.15.0").unwrap(), ("10.15.0", NodeVersion { major: 10, minor: 15, patch: 0 }));
        assert_eq!(parse_node_version("v10.14.2").unwrap(), ("10.14.2", NodeVersion { major: 10, minor: 14, patch: 2 }));
        assert_eq!(parse_node_version("v10.14.1").unwrap(), ("10.14.1", NodeVersion { major: 10, minor: 14, patch: 1 }));
        assert_eq!(parse_node_version("v10.14.0").unwrap(), ("10.14.0", NodeVersion { major: 10, minor: 14, patch: 0 }));
        assert_eq!(parse_node_version("v10.13.0").unwrap(), ("10.13.0", NodeVersion { major: 10, minor: 13, patch: 0 }));
        assert_eq!(parse_node_version("v10.12.0").unwrap(), ("10.12.0", NodeVersion { major: 10, minor: 12, patch: 0 }));
        assert_eq!(parse_node_version("v10.11.0").unwrap(), ("10.11.0", NodeVersion { major: 10, minor: 11, patch: 0 }));
        assert_eq!(parse_node_version("v10.10.0").unwrap(), ("10.10.0", NodeVersion { major: 10, minor: 10, patch: 0 }));
        assert_eq!(parse_node_version("v10.9.0").unwrap(), ("10.9.0", NodeVersion { major: 10, minor: 9, patch: 0 }));
        assert_eq!(parse_node_version("v10.8.0").unwrap(), ("10.8.0", NodeVersion { major: 10, minor: 8, patch: 0 }));
        assert_eq!(parse_node_version("v10.7.0").unwrap(), ("10.7.0", NodeVersion { major: 10, minor: 7, patch: 0 }));
        assert_eq!(parse_node_version("v10.6.0").unwrap(), ("10.6.0", NodeVersion { major: 10, minor: 6, patch: 0 }));
        assert_eq!(parse_node_version("v10.5.0").unwrap(), ("10.5.0", NodeVersion { major: 10, minor: 5, patch: 0 }));
        assert_eq!(parse_node_version("v10.4.1").unwrap(), ("10.4.1", NodeVersion { major: 10, minor: 4, patch: 1 }));
        assert_eq!(parse_node_version("v10.4.0").unwrap(), ("10.4.0", NodeVersion { major: 10, minor: 4, patch: 0 }));
        assert_eq!(parse_node_version("v10.3.0").unwrap(), ("10.3.0", NodeVersion { major: 10, minor: 3, patch: 0 }));
        assert_eq!(parse_node_version("v10.2.1").unwrap(), ("10.2.1", NodeVersion { major: 10, minor: 2, patch: 1 }));
        assert_eq!(parse_node_version("v10.2.0").unwrap(), ("10.2.0", NodeVersion { major: 10, minor: 2, patch: 0 }));
        assert_eq!(parse_node_version("v10.1.0").unwrap(), ("10.1.0", NodeVersion { major: 10, minor: 1, patch: 0 }));
        assert_eq!(parse_node_version("v10.0.0").unwrap(), ("10.0.0", NodeVersion { major: 10, minor: 0, patch: 0 }));
        assert_eq!(parse_node_version("v9.11.2").unwrap(), ("9.11.2", NodeVersion { major: 9, minor: 11, patch: 2 }));
        assert_eq!(parse_node_version("v9.11.1").unwrap(), ("9.11.1", NodeVersion { major: 9, minor: 11, patch: 1 }));
        assert_eq!(parse_node_version("v9.11.0").unwrap(), ("9.11.0", NodeVersion { major: 9, minor: 11, patch: 0 }));
        assert_eq!(parse_node_version("v9.10.1").unwrap(), ("9.10.1", NodeVersion { major: 9, minor: 10, patch: 1 }));
        assert_eq!(parse_node_version("v9.10.0").unwrap(), ("9.10.0", NodeVersion { major: 9, minor: 10, patch: 0 }));
        assert_eq!(parse_node_version("v9.9.0").unwrap(), ("9.9.0", NodeVersion { major: 9, minor: 9, patch: 0 }));
        assert_eq!(parse_node_version("v9.8.0").unwrap(), ("9.8.0", NodeVersion { major: 9, minor: 8, patch: 0 }));
        assert_eq!(parse_node_version("v9.7.1").unwrap(), ("9.7.1", NodeVersion { major: 9, minor: 7, patch: 1 }));
        assert_eq!(parse_node_version("v9.7.0").unwrap(), ("9.7.0", NodeVersion { major: 9, minor: 7, patch: 0 }));
        assert_eq!(parse_node_version("v9.6.1").unwrap(), ("9.6.1", NodeVersion { major: 9, minor: 6, patch: 1 }));
        assert_eq!(parse_node_version("v9.6.0").unwrap(), ("9.6.0", NodeVersion { major: 9, minor: 6, patch: 0 }));
        assert_eq!(parse_node_version("v9.5.0").unwrap(), ("9.5.0", NodeVersion { major: 9, minor: 5, patch: 0 }));
        assert_eq!(parse_node_version("v9.4.0").unwrap(), ("9.4.0", NodeVersion { major: 9, minor: 4, patch: 0 }));
        assert_eq!(parse_node_version("v9.3.0").unwrap(), ("9.3.0", NodeVersion { major: 9, minor: 3, patch: 0 }));
        assert_eq!(parse_node_version("v9.2.1").unwrap(), ("9.2.1", NodeVersion { major: 9, minor: 2, patch: 1 }));
        assert_eq!(parse_node_version("v9.2.0").unwrap(), ("9.2.0", NodeVersion { major: 9, minor: 2, patch: 0 }));
        assert_eq!(parse_node_version("v9.1.0").unwrap(), ("9.1.0", NodeVersion { major: 9, minor: 1, patch: 0 }));
        assert_eq!(parse_node_version("v9.0.0").unwrap(), ("9.0.0", NodeVersion { major: 9, minor: 0, patch: 0 }));
        assert_eq!(parse_node_version("v8.17.0").unwrap(), ("8.17.0", NodeVersion { major: 8, minor: 17, patch: 0 }));
        assert_eq!(parse_node_version("v8.16.2").unwrap(), ("8.16.2", NodeVersion { major: 8, minor: 16, patch: 2 }));
        assert_eq!(parse_node_version("v8.16.1").unwrap(), ("8.16.1", NodeVersion { major: 8, minor: 16, patch: 1 }));
        assert_eq!(parse_node_version("v8.16.0").unwrap(), ("8.16.0", NodeVersion { major: 8, minor: 16, patch: 0 }));
        assert_eq!(parse_node_version("v8.15.1").unwrap(), ("8.15.1", NodeVersion { major: 8, minor: 15, patch: 1 }));
        assert_eq!(parse_node_version("v8.15.0").unwrap(), ("8.15.0", NodeVersion { major: 8, minor: 15, patch: 0 }));
        assert_eq!(parse_node_version("v8.14.1").unwrap(), ("8.14.1", NodeVersion { major: 8, minor: 14, patch: 1 }));
        assert_eq!(parse_node_version("v8.14.0").unwrap(), ("8.14.0", NodeVersion { major: 8, minor: 14, patch: 0 }));
        assert_eq!(parse_node_version("v8.13.0").unwrap(), ("8.13.0", NodeVersion { major: 8, minor: 13, patch: 0 }));
        assert_eq!(parse_node_version("v8.12.0").unwrap(), ("8.12.0", NodeVersion { major: 8, minor: 12, patch: 0 }));
        assert_eq!(parse_node_version("v8.11.4").unwrap(), ("8.11.4", NodeVersion { major: 8, minor: 11, patch: 4 }));
        assert_eq!(parse_node_version("v8.11.3").unwrap(), ("8.11.3", NodeVersion { major: 8, minor: 11, patch: 3 }));
        assert_eq!(parse_node_version("v8.11.2").unwrap(), ("8.11.2", NodeVersion { major: 8, minor: 11, patch: 2 }));
        assert_eq!(parse_node_version("v8.11.1").unwrap(), ("8.11.1", NodeVersion { major: 8, minor: 11, patch: 1 }));
        assert_eq!(parse_node_version("v8.11.0").unwrap(), ("8.11.0", NodeVersion { major: 8, minor: 11, patch: 0 }));
        assert_eq!(parse_node_version("v8.10.0").unwrap(), ("8.10.0", NodeVersion { major: 8, minor: 10, patch: 0 }));
        assert_eq!(parse_node_version("v8.9.4").unwrap(), ("8.9.4", NodeVersion { major: 8, minor: 9, patch: 4 }));
        assert_eq!(parse_node_version("v8.9.3").unwrap(), ("8.9.3", NodeVersion { major: 8, minor: 9, patch: 3 }));
        assert_eq!(parse_node_version("v8.9.2").unwrap(), ("8.9.2", NodeVersion { major: 8, minor: 9, patch: 2 }));
        assert_eq!(parse_node_version("v8.9.1").unwrap(), ("8.9.1", NodeVersion { major: 8, minor: 9, patch: 1 }));
        assert_eq!(parse_node_version("v8.9.0").unwrap(), ("8.9.0", NodeVersion { major: 8, minor: 9, patch: 0 }));
        assert_eq!(parse_node_version("v8.8.1").unwrap(), ("8.8.1", NodeVersion { major: 8, minor: 8, patch: 1 }));
        assert_eq!(parse_node_version("v8.8.0").unwrap(), ("8.8.0", NodeVersion { major: 8, minor: 8, patch: 0 }));
        assert_eq!(parse_node_version("v8.7.0").unwrap(), ("8.7.0", NodeVersion { major: 8, minor: 7, patch: 0 }));
        assert_eq!(parse_node_version("v8.6.0").unwrap(), ("8.6.0", NodeVersion { major: 8, minor: 6, patch: 0 }));
        assert_eq!(parse_node_version("v8.5.0").unwrap(), ("8.5.0", NodeVersion { major: 8, minor: 5, patch: 0 }));
        assert_eq!(parse_node_version("v8.4.0").unwrap(), ("8.4.0", NodeVersion { major: 8, minor: 4, patch: 0 }));
        assert_eq!(parse_node_version("v8.3.0").unwrap(), ("8.3.0", NodeVersion { major: 8, minor: 3, patch: 0 }));
        assert_eq!(parse_node_version("v8.2.1").unwrap(), ("8.2.1", NodeVersion { major: 8, minor: 2, patch: 1 }));
        assert_eq!(parse_node_version("v8.2.0").unwrap(), ("8.2.0", NodeVersion { major: 8, minor: 2, patch: 0 }));
        assert_eq!(parse_node_version("v8.1.4").unwrap(), ("8.1.4", NodeVersion { major: 8, minor: 1, patch: 4 }));
        assert_eq!(parse_node_version("v8.1.3").unwrap(), ("8.1.3", NodeVersion { major: 8, minor: 1, patch: 3 }));
        assert_eq!(parse_node_version("v8.1.2").unwrap(), ("8.1.2", NodeVersion { major: 8, minor: 1, patch: 2 }));
        assert_eq!(parse_node_version("v8.1.1").unwrap(), ("8.1.1", NodeVersion { major: 8, minor: 1, patch: 1 }));
        assert_eq!(parse_node_version("v8.1.0").unwrap(), ("8.1.0", NodeVersion { major: 8, minor: 1, patch: 0 }));
        assert_eq!(parse_node_version("v8.0.0").unwrap(), ("8.0.0", NodeVersion { major: 8, minor: 0, patch: 0 }));
        assert_eq!(parse_node_version("v7.10.1").unwrap(), ("7.10.1", NodeVersion { major: 7, minor: 10, patch: 1 }));
        assert_eq!(parse_node_version("v7.10.0").unwrap(), ("7.10.0", NodeVersion { major: 7, minor: 10, patch: 0 }));
        assert_eq!(parse_node_version("v7.9.0").unwrap(), ("7.9.0", NodeVersion { major: 7, minor: 9, patch: 0 }));
        assert_eq!(parse_node_version("v7.8.0").unwrap(), ("7.8.0", NodeVersion { major: 7, minor: 8, patch: 0 }));
        assert_eq!(parse_node_version("v7.7.4").unwrap(), ("7.7.4", NodeVersion { major: 7, minor: 7, patch: 4 }));
        assert_eq!(parse_node_version("v7.7.3").unwrap(), ("7.7.3", NodeVersion { major: 7, minor: 7, patch: 3 }));
        assert_eq!(parse_node_version("v7.7.2").unwrap(), ("7.7.2", NodeVersion { major: 7, minor: 7, patch: 2 }));
        assert_eq!(parse_node_version("v7.7.1").unwrap(), ("7.7.1", NodeVersion { major: 7, minor: 7, patch: 1 }));
        assert_eq!(parse_node_version("v7.7.0").unwrap(), ("7.7.0", NodeVersion { major: 7, minor: 7, patch: 0 }));
        assert_eq!(parse_node_version("v7.6.0").unwrap(), ("7.6.0", NodeVersion { major: 7, minor: 6, patch: 0 }));
        assert_eq!(parse_node_version("v7.5.0").unwrap(), ("7.5.0", NodeVersion { major: 7, minor: 5, patch: 0 }));
        assert_eq!(parse_node_version("v7.4.0").unwrap(), ("7.4.0", NodeVersion { major: 7, minor: 4, patch: 0 }));
        assert_eq!(parse_node_version("v7.3.0").unwrap(), ("7.3.0", NodeVersion { major: 7, minor: 3, patch: 0 }));
        assert_eq!(parse_node_version("v7.2.1").unwrap(), ("7.2.1", NodeVersion { major: 7, minor: 2, patch: 1 }));
        assert_eq!(parse_node_version("v7.2.0").unwrap(), ("7.2.0", NodeVersion { major: 7, minor: 2, patch: 0 }));
        assert_eq!(parse_node_version("v7.1.0").unwrap(), ("7.1.0", NodeVersion { major: 7, minor: 1, patch: 0 }));
        assert_eq!(parse_node_version("v7.0.0").unwrap(), ("7.0.0", NodeVersion { major: 7, minor: 0, patch: 0 }));
        assert_eq!(parse_node_version("v6.17.1").unwrap(), ("6.17.1", NodeVersion { major: 6, minor: 17, patch: 1 }));
        assert_eq!(parse_node_version("v6.17.0").unwrap(), ("6.17.0", NodeVersion { major: 6, minor: 17, patch: 0 }));
        assert_eq!(parse_node_version("v6.16.0").unwrap(), ("6.16.0", NodeVersion { major: 6, minor: 16, patch: 0 }));
        assert_eq!(parse_node_version("v6.15.1").unwrap(), ("6.15.1", NodeVersion { major: 6, minor: 15, patch: 1 }));
        assert_eq!(parse_node_version("v6.15.0").unwrap(), ("6.15.0", NodeVersion { major: 6, minor: 15, patch: 0 }));
        assert_eq!(parse_node_version("v6.14.4").unwrap(), ("6.14.4", NodeVersion { major: 6, minor: 14, patch: 4 }));
        assert_eq!(parse_node_version("v6.14.3").unwrap(), ("6.14.3", NodeVersion { major: 6, minor: 14, patch: 3 }));
        assert_eq!(parse_node_version("v6.14.2").unwrap(), ("6.14.2", NodeVersion { major: 6, minor: 14, patch: 2 }));
        assert_eq!(parse_node_version("v6.14.1").unwrap(), ("6.14.1", NodeVersion { major: 6, minor: 14, patch: 1 }));
        assert_eq!(parse_node_version("v6.14.0").unwrap(), ("6.14.0", NodeVersion { major: 6, minor: 14, patch: 0 }));
        assert_eq!(parse_node_version("v6.13.1").unwrap(), ("6.13.1", NodeVersion { major: 6, minor: 13, patch: 1 }));
        assert_eq!(parse_node_version("v6.13.0").unwrap(), ("6.13.0", NodeVersion { major: 6, minor: 13, patch: 0 }));
        assert_eq!(parse_node_version("v6.12.3").unwrap(), ("6.12.3", NodeVersion { major: 6, minor: 12, patch: 3 }));
        assert_eq!(parse_node_version("v6.12.2").unwrap(), ("6.12.2", NodeVersion { major: 6, minor: 12, patch: 2 }));
        assert_eq!(parse_node_version("v6.12.1").unwrap(), ("6.12.1", NodeVersion { major: 6, minor: 12, patch: 1 }));
        assert_eq!(parse_node_version("v6.12.0").unwrap(), ("6.12.0", NodeVersion { major: 6, minor: 12, patch: 0 }));
        assert_eq!(parse_node_version("v6.11.5").unwrap(), ("6.11.5", NodeVersion { major: 6, minor: 11, patch: 5 }));
        assert_eq!(parse_node_version("v6.11.4").unwrap(), ("6.11.4", NodeVersion { major: 6, minor: 11, patch: 4 }));
        assert_eq!(parse_node_version("v6.11.3").unwrap(), ("6.11.3", NodeVersion { major: 6, minor: 11, patch: 3 }));
        assert_eq!(parse_node_version("v6.11.2").unwrap(), ("6.11.2", NodeVersion { major: 6, minor: 11, patch: 2 }));
        assert_eq!(parse_node_version("v6.11.1").unwrap(), ("6.11.1", NodeVersion { major: 6, minor: 11, patch: 1 }));
        assert_eq!(parse_node_version("v6.11.0").unwrap(), ("6.11.0", NodeVersion { major: 6, minor: 11, patch: 0 }));
        assert_eq!(parse_node_version("v6.10.3").unwrap(), ("6.10.3", NodeVersion { major: 6, minor: 10, patch: 3 }));
        assert_eq!(parse_node_version("v6.10.2").unwrap(), ("6.10.2", NodeVersion { major: 6, minor: 10, patch: 2 }));
        assert_eq!(parse_node_version("v6.10.1").unwrap(), ("6.10.1", NodeVersion { major: 6, minor: 10, patch: 1 }));
        assert_eq!(parse_node_version("v6.10.0").unwrap(), ("6.10.0", NodeVersion { major: 6, minor: 10, patch: 0 }));
        assert_eq!(parse_node_version("v6.9.5").unwrap(), ("6.9.5", NodeVersion { major: 6, minor: 9, patch: 5 }));
        assert_eq!(parse_node_version("v6.9.4").unwrap(), ("6.9.4", NodeVersion { major: 6, minor: 9, patch: 4 }));
        assert_eq!(parse_node_version("v6.9.3").unwrap(), ("6.9.3", NodeVersion { major: 6, minor: 9, patch: 3 }));
        assert_eq!(parse_node_version("v6.9.2").unwrap(), ("6.9.2", NodeVersion { major: 6, minor: 9, patch: 2 }));
        assert_eq!(parse_node_version("v6.9.1").unwrap(), ("6.9.1", NodeVersion { major: 6, minor: 9, patch: 1 }));
        assert_eq!(parse_node_version("v6.9.0").unwrap(), ("6.9.0", NodeVersion { major: 6, minor: 9, patch: 0 }));
        assert_eq!(parse_node_version("v6.8.1").unwrap(), ("6.8.1", NodeVersion { major: 6, minor: 8, patch: 1 }));
        assert_eq!(parse_node_version("v6.8.0").unwrap(), ("6.8.0", NodeVersion { major: 6, minor: 8, patch: 0 }));
        assert_eq!(parse_node_version("v6.7.0").unwrap(), ("6.7.0", NodeVersion { major: 6, minor: 7, patch: 0 }));
        assert_eq!(parse_node_version("v6.6.0").unwrap(), ("6.6.0", NodeVersion { major: 6, minor: 6, patch: 0 }));
        assert_eq!(parse_node_version("v6.5.0").unwrap(), ("6.5.0", NodeVersion { major: 6, minor: 5, patch: 0 }));
        assert_eq!(parse_node_version("v6.4.0").unwrap(), ("6.4.0", NodeVersion { major: 6, minor: 4, patch: 0 }));
        assert_eq!(parse_node_version("v6.3.1").unwrap(), ("6.3.1", NodeVersion { major: 6, minor: 3, patch: 1 }));
        assert_eq!(parse_node_version("v6.3.0").unwrap(), ("6.3.0", NodeVersion { major: 6, minor: 3, patch: 0 }));
        assert_eq!(parse_node_version("v6.2.2").unwrap(), ("6.2.2", NodeVersion { major: 6, minor: 2, patch: 2 }));
        assert_eq!(parse_node_version("v6.2.1").unwrap(), ("6.2.1", NodeVersion { major: 6, minor: 2, patch: 1 }));
        assert_eq!(parse_node_version("v6.2.0").unwrap(), ("6.2.0", NodeVersion { major: 6, minor: 2, patch: 0 }));
        assert_eq!(parse_node_version("v6.1.0").unwrap(), ("6.1.0", NodeVersion { major: 6, minor: 1, patch: 0 }));
        assert_eq!(parse_node_version("v6.0.0").unwrap(), ("6.0.0", NodeVersion { major: 6, minor: 0, patch: 0 }));
        assert_eq!(parse_node_version("v5.12.0").unwrap(), ("5.12.0", NodeVersion { major: 5, minor: 12, patch: 0 }));
        assert_eq!(parse_node_version("v5.11.1").unwrap(), ("5.11.1", NodeVersion { major: 5, minor: 11, patch: 1 }));
        assert_eq!(parse_node_version("v5.11.0").unwrap(), ("5.11.0", NodeVersion { major: 5, minor: 11, patch: 0 }));
        assert_eq!(parse_node_version("v5.10.1").unwrap(), ("5.10.1", NodeVersion { major: 5, minor: 10, patch: 1 }));
        assert_eq!(parse_node_version("v5.10.0").unwrap(), ("5.10.0", NodeVersion { major: 5, minor: 10, patch: 0 }));
        assert_eq!(parse_node_version("v5.9.1").unwrap(), ("5.9.1", NodeVersion { major: 5, minor: 9, patch: 1 }));
        assert_eq!(parse_node_version("v5.9.0").unwrap(), ("5.9.0", NodeVersion { major: 5, minor: 9, patch: 0 }));
        assert_eq!(parse_node_version("v5.8.0").unwrap(), ("5.8.0", NodeVersion { major: 5, minor: 8, patch: 0 }));
        assert_eq!(parse_node_version("v5.7.1").unwrap(), ("5.7.1", NodeVersion { major: 5, minor: 7, patch: 1 }));
        assert_eq!(parse_node_version("v5.7.0").unwrap(), ("5.7.0", NodeVersion { major: 5, minor: 7, patch: 0 }));
        assert_eq!(parse_node_version("v5.6.0").unwrap(), ("5.6.0", NodeVersion { major: 5, minor: 6, patch: 0 }));
        assert_eq!(parse_node_version("v5.5.0").unwrap(), ("5.5.0", NodeVersion { major: 5, minor: 5, patch: 0 }));
        assert_eq!(parse_node_version("v5.4.1").unwrap(), ("5.4.1", NodeVersion { major: 5, minor: 4, patch: 1 }));
        assert_eq!(parse_node_version("v5.4.0").unwrap(), ("5.4.0", NodeVersion { major: 5, minor: 4, patch: 0 }));
        assert_eq!(parse_node_version("v5.3.0").unwrap(), ("5.3.0", NodeVersion { major: 5, minor: 3, patch: 0 }));
        assert_eq!(parse_node_version("v5.2.0").unwrap(), ("5.2.0", NodeVersion { major: 5, minor: 2, patch: 0 }));
        assert_eq!(parse_node_version("v5.1.1").unwrap(), ("5.1.1", NodeVersion { major: 5, minor: 1, patch: 1 }));
        assert_eq!(parse_node_version("v5.1.0").unwrap(), ("5.1.0", NodeVersion { major: 5, minor: 1, patch: 0 }));
        assert_eq!(parse_node_version("v5.0.0").unwrap(), ("5.0.0", NodeVersion { major: 5, minor: 0, patch: 0 }));
        assert_eq!(parse_node_version("v4.9.1").unwrap(), ("4.9.1", NodeVersion { major: 4, minor: 9, patch: 1 }));
        assert_eq!(parse_node_version("v4.9.0").unwrap(), ("4.9.0", NodeVersion { major: 4, minor: 9, patch: 0 }));
        assert_eq!(parse_node_version("v4.8.7").unwrap(), ("4.8.7", NodeVersion { major: 4, minor: 8, patch: 7 }));
        assert_eq!(parse_node_version("v4.8.6").unwrap(), ("4.8.6", NodeVersion { major: 4, minor: 8, patch: 6 }));
        assert_eq!(parse_node_version("v4.8.5").unwrap(), ("4.8.5", NodeVersion { major: 4, minor: 8, patch: 5 }));
        assert_eq!(parse_node_version("v4.8.4").unwrap(), ("4.8.4", NodeVersion { major: 4, minor: 8, patch: 4 }));
        assert_eq!(parse_node_version("v4.8.3").unwrap(), ("4.8.3", NodeVersion { major: 4, minor: 8, patch: 3 }));
        assert_eq!(parse_node_version("v4.8.2").unwrap(), ("4.8.2", NodeVersion { major: 4, minor: 8, patch: 2 }));
        assert_eq!(parse_node_version("v4.8.1").unwrap(), ("4.8.1", NodeVersion { major: 4, minor: 8, patch: 1 }));
        assert_eq!(parse_node_version("v4.8.0").unwrap(), ("4.8.0", NodeVersion { major: 4, minor: 8, patch: 0 }));
        assert_eq!(parse_node_version("v4.7.3").unwrap(), ("4.7.3", NodeVersion { major: 4, minor: 7, patch: 3 }));
        assert_eq!(parse_node_version("v4.7.2").unwrap(), ("4.7.2", NodeVersion { major: 4, minor: 7, patch: 2 }));
        assert_eq!(parse_node_version("v4.7.1").unwrap(), ("4.7.1", NodeVersion { major: 4, minor: 7, patch: 1 }));
        assert_eq!(parse_node_version("v4.7.0").unwrap(), ("4.7.0", NodeVersion { major: 4, minor: 7, patch: 0 }));
        assert_eq!(parse_node_version("v4.6.2").unwrap(), ("4.6.2", NodeVersion { major: 4, minor: 6, patch: 2 }));
        assert_eq!(parse_node_version("v4.6.1").unwrap(), ("4.6.1", NodeVersion { major: 4, minor: 6, patch: 1 }));
        assert_eq!(parse_node_version("v4.6.0").unwrap(), ("4.6.0", NodeVersion { major: 4, minor: 6, patch: 0 }));
        assert_eq!(parse_node_version("v4.5.0").unwrap(), ("4.5.0", NodeVersion { major: 4, minor: 5, patch: 0 }));
        assert_eq!(parse_node_version("v4.4.7").unwrap(), ("4.4.7", NodeVersion { major: 4, minor: 4, patch: 7 }));
        assert_eq!(parse_node_version("v4.4.6").unwrap(), ("4.4.6", NodeVersion { major: 4, minor: 4, patch: 6 }));
        assert_eq!(parse_node_version("v4.4.5").unwrap(), ("4.4.5", NodeVersion { major: 4, minor: 4, patch: 5 }));
        assert_eq!(parse_node_version("v4.4.4").unwrap(), ("4.4.4", NodeVersion { major: 4, minor: 4, patch: 4 }));
        assert_eq!(parse_node_version("v4.4.3").unwrap(), ("4.4.3", NodeVersion { major: 4, minor: 4, patch: 3 }));
        assert_eq!(parse_node_version("v4.4.2").unwrap(), ("4.4.2", NodeVersion { major: 4, minor: 4, patch: 2 }));
        assert_eq!(parse_node_version("v4.4.1").unwrap(), ("4.4.1", NodeVersion { major: 4, minor: 4, patch: 1 }));
        assert_eq!(parse_node_version("v4.4.0").unwrap(), ("4.4.0", NodeVersion { major: 4, minor: 4, patch: 0 }));
        assert_eq!(parse_node_version("v4.3.2").unwrap(), ("4.3.2", NodeVersion { major: 4, minor: 3, patch: 2 }));
        assert_eq!(parse_node_version("v4.3.1").unwrap(), ("4.3.1", NodeVersion { major: 4, minor: 3, patch: 1 }));
        assert_eq!(parse_node_version("v4.3.0").unwrap(), ("4.3.0", NodeVersion { major: 4, minor: 3, patch: 0 }));
        assert_eq!(parse_node_version("v4.2.6").unwrap(), ("4.2.6", NodeVersion { major: 4, minor: 2, patch: 6 }));
        assert_eq!(parse_node_version("v4.2.5").unwrap(), ("4.2.5", NodeVersion { major: 4, minor: 2, patch: 5 }));
        assert_eq!(parse_node_version("v4.2.4").unwrap(), ("4.2.4", NodeVersion { major: 4, minor: 2, patch: 4 }));
        assert_eq!(parse_node_version("v4.2.3").unwrap(), ("4.2.3", NodeVersion { major: 4, minor: 2, patch: 3 }));
        assert_eq!(parse_node_version("v4.2.2").unwrap(), ("4.2.2", NodeVersion { major: 4, minor: 2, patch: 2 }));
        assert_eq!(parse_node_version("v4.2.1").unwrap(), ("4.2.1", NodeVersion { major: 4, minor: 2, patch: 1 }));
        assert_eq!(parse_node_version("v4.2.0").unwrap(), ("4.2.0", NodeVersion { major: 4, minor: 2, patch: 0 }));
        assert_eq!(parse_node_version("v4.1.2").unwrap(), ("4.1.2", NodeVersion { major: 4, minor: 1, patch: 2 }));
        assert_eq!(parse_node_version("v4.1.1").unwrap(), ("4.1.1", NodeVersion { major: 4, minor: 1, patch: 1 }));
        assert_eq!(parse_node_version("v4.1.0").unwrap(), ("4.1.0", NodeVersion { major: 4, minor: 1, patch: 0 }));
        assert_eq!(parse_node_version("v4.0.0").unwrap(), ("4.0.0", NodeVersion { major: 4, minor: 0, patch: 0 }));
        assert_eq!(parse_node_version("v0.12.18").unwrap(), ("0.12.18", NodeVersion { major: 0, minor: 12, patch: 18 }));
        assert_eq!(parse_node_version("v0.12.17").unwrap(), ("0.12.17", NodeVersion { major: 0, minor: 12, patch: 17 }));
        assert_eq!(parse_node_version("v0.12.16").unwrap(), ("0.12.16", NodeVersion { major: 0, minor: 12, patch: 16 }));
        assert_eq!(parse_node_version("v0.12.15").unwrap(), ("0.12.15", NodeVersion { major: 0, minor: 12, patch: 15 }));
        assert_eq!(parse_node_version("v0.12.14").unwrap(), ("0.12.14", NodeVersion { major: 0, minor: 12, patch: 14 }));
        assert_eq!(parse_node_version("v0.12.13").unwrap(), ("0.12.13", NodeVersion { major: 0, minor: 12, patch: 13 }));
        assert_eq!(parse_node_version("v0.12.12").unwrap(), ("0.12.12", NodeVersion { major: 0, minor: 12, patch: 12 }));
        assert_eq!(parse_node_version("v0.12.11").unwrap(), ("0.12.11", NodeVersion { major: 0, minor: 12, patch: 11 }));
        assert_eq!(parse_node_version("v0.12.10").unwrap(), ("0.12.10", NodeVersion { major: 0, minor: 12, patch: 10 }));
        assert_eq!(parse_node_version("v0.12.9").unwrap(), ("0.12.9", NodeVersion { major: 0, minor: 12, patch: 9 }));
        assert_eq!(parse_node_version("v0.12.8").unwrap(), ("0.12.8", NodeVersion { major: 0, minor: 12, patch: 8 }));
        assert_eq!(parse_node_version("v0.12.7").unwrap(), ("0.12.7", NodeVersion { major: 0, minor: 12, patch: 7 }));
        assert_eq!(parse_node_version("v0.12.6").unwrap(), ("0.12.6", NodeVersion { major: 0, minor: 12, patch: 6 }));
        assert_eq!(parse_node_version("v0.12.5").unwrap(), ("0.12.5", NodeVersion { major: 0, minor: 12, patch: 5 }));
        assert_eq!(parse_node_version("v0.12.4").unwrap(), ("0.12.4", NodeVersion { major: 0, minor: 12, patch: 4 }));
        assert_eq!(parse_node_version("v0.12.3").unwrap(), ("0.12.3", NodeVersion { major: 0, minor: 12, patch: 3 }));
        assert_eq!(parse_node_version("v0.12.2").unwrap(), ("0.12.2", NodeVersion { major: 0, minor: 12, patch: 2 }));
        assert_eq!(parse_node_version("v0.12.1").unwrap(), ("0.12.1", NodeVersion { major: 0, minor: 12, patch: 1 }));
        assert_eq!(parse_node_version("v0.12.0").unwrap(), ("0.12.0", NodeVersion { major: 0, minor: 12, patch: 0 }));
        assert_eq!(parse_node_version("v0.11.16").unwrap(), ("0.11.16", NodeVersion { major: 0, minor: 11, patch: 16 }));
        assert_eq!(parse_node_version("v0.11.15").unwrap(), ("0.11.15", NodeVersion { major: 0, minor: 11, patch: 15 }));
        assert_eq!(parse_node_version("v0.11.14").unwrap(), ("0.11.14", NodeVersion { major: 0, minor: 11, patch: 14 }));
        assert_eq!(parse_node_version("v0.11.13").unwrap(), ("0.11.13", NodeVersion { major: 0, minor: 11, patch: 13 }));
        assert_eq!(parse_node_version("v0.11.12").unwrap(), ("0.11.12", NodeVersion { major: 0, minor: 11, patch: 12 }));
        assert_eq!(parse_node_version("v0.11.11").unwrap(), ("0.11.11", NodeVersion { major: 0, minor: 11, patch: 11 }));
        assert_eq!(parse_node_version("v0.11.10").unwrap(), ("0.11.10", NodeVersion { major: 0, minor: 11, patch: 10 }));
        assert_eq!(parse_node_version("v0.11.9").unwrap(), ("0.11.9", NodeVersion { major: 0, minor: 11, patch: 9 }));
        assert_eq!(parse_node_version("v0.11.8").unwrap(), ("0.11.8", NodeVersion { major: 0, minor: 11, patch: 8 }));
        assert_eq!(parse_node_version("v0.11.7").unwrap(), ("0.11.7", NodeVersion { major: 0, minor: 11, patch: 7 }));
        assert_eq!(parse_node_version("v0.11.6").unwrap(), ("0.11.6", NodeVersion { major: 0, minor: 11, patch: 6 }));
        assert_eq!(parse_node_version("v0.11.5").unwrap(), ("0.11.5", NodeVersion { major: 0, minor: 11, patch: 5 }));
        assert_eq!(parse_node_version("v0.11.4").unwrap(), ("0.11.4", NodeVersion { major: 0, minor: 11, patch: 4 }));
        assert_eq!(parse_node_version("v0.11.3").unwrap(), ("0.11.3", NodeVersion { major: 0, minor: 11, patch: 3 }));
        assert_eq!(parse_node_version("v0.11.2").unwrap(), ("0.11.2", NodeVersion { major: 0, minor: 11, patch: 2 }));
        assert_eq!(parse_node_version("v0.11.1").unwrap(), ("0.11.1", NodeVersion { major: 0, minor: 11, patch: 1 }));
        assert_eq!(parse_node_version("v0.11.0").unwrap(), ("0.11.0", NodeVersion { major: 0, minor: 11, patch: 0 }));
        assert_eq!(parse_node_version("v0.10.48").unwrap(), ("0.10.48", NodeVersion { major: 0, minor: 10, patch: 48 }));
        assert_eq!(parse_node_version("v0.10.47").unwrap(), ("0.10.47", NodeVersion { major: 0, minor: 10, patch: 47 }));
        assert_eq!(parse_node_version("v0.10.46").unwrap(), ("0.10.46", NodeVersion { major: 0, minor: 10, patch: 46 }));
        assert_eq!(parse_node_version("v0.10.45").unwrap(), ("0.10.45", NodeVersion { major: 0, minor: 10, patch: 45 }));
        assert_eq!(parse_node_version("v0.10.44").unwrap(), ("0.10.44", NodeVersion { major: 0, minor: 10, patch: 44 }));
        assert_eq!(parse_node_version("v0.10.43").unwrap(), ("0.10.43", NodeVersion { major: 0, minor: 10, patch: 43 }));
        assert_eq!(parse_node_version("v0.10.42").unwrap(), ("0.10.42", NodeVersion { major: 0, minor: 10, patch: 42 }));
        assert_eq!(parse_node_version("v0.10.41").unwrap(), ("0.10.41", NodeVersion { major: 0, minor: 10, patch: 41 }));
        assert_eq!(parse_node_version("v0.10.40").unwrap(), ("0.10.40", NodeVersion { major: 0, minor: 10, patch: 40 }));
        assert_eq!(parse_node_version("v0.10.39").unwrap(), ("0.10.39", NodeVersion { major: 0, minor: 10, patch: 39 }));
        assert_eq!(parse_node_version("v0.10.38").unwrap(), ("0.10.38", NodeVersion { major: 0, minor: 10, patch: 38 }));
        assert_eq!(parse_node_version("v0.10.37").unwrap(), ("0.10.37", NodeVersion { major: 0, minor: 10, patch: 37 }));
        assert_eq!(parse_node_version("v0.10.36").unwrap(), ("0.10.36", NodeVersion { major: 0, minor: 10, patch: 36 }));
        assert_eq!(parse_node_version("v0.10.35").unwrap(), ("0.10.35", NodeVersion { major: 0, minor: 10, patch: 35 }));
        assert_eq!(parse_node_version("v0.10.34").unwrap(), ("0.10.34", NodeVersion { major: 0, minor: 10, patch: 34 }));
        assert_eq!(parse_node_version("v0.10.33").unwrap(), ("0.10.33", NodeVersion { major: 0, minor: 10, patch: 33 }));
        assert_eq!(parse_node_version("v0.10.32").unwrap(), ("0.10.32", NodeVersion { major: 0, minor: 10, patch: 32 }));
        assert_eq!(parse_node_version("v0.10.31").unwrap(), ("0.10.31", NodeVersion { major: 0, minor: 10, patch: 31 }));
        assert_eq!(parse_node_version("v0.10.30").unwrap(), ("0.10.30", NodeVersion { major: 0, minor: 10, patch: 30 }));
        assert_eq!(parse_node_version("v0.10.29").unwrap(), ("0.10.29", NodeVersion { major: 0, minor: 10, patch: 29 }));
        assert_eq!(parse_node_version("v0.10.28").unwrap(), ("0.10.28", NodeVersion { major: 0, minor: 10, patch: 28 }));
        assert_eq!(parse_node_version("v0.10.27").unwrap(), ("0.10.27", NodeVersion { major: 0, minor: 10, patch: 27 }));
        assert_eq!(parse_node_version("v0.10.26").unwrap(), ("0.10.26", NodeVersion { major: 0, minor: 10, patch: 26 }));
        assert_eq!(parse_node_version("v0.10.25").unwrap(), ("0.10.25", NodeVersion { major: 0, minor: 10, patch: 25 }));
        assert_eq!(parse_node_version("v0.10.24").unwrap(), ("0.10.24", NodeVersion { major: 0, minor: 10, patch: 24 }));
        assert_eq!(parse_node_version("v0.10.23").unwrap(), ("0.10.23", NodeVersion { major: 0, minor: 10, patch: 23 }));
        assert_eq!(parse_node_version("v0.10.22").unwrap(), ("0.10.22", NodeVersion { major: 0, minor: 10, patch: 22 }));
        assert_eq!(parse_node_version("v0.10.21").unwrap(), ("0.10.21", NodeVersion { major: 0, minor: 10, patch: 21 }));
        assert_eq!(parse_node_version("v0.10.20").unwrap(), ("0.10.20", NodeVersion { major: 0, minor: 10, patch: 20 }));
        assert_eq!(parse_node_version("v0.10.19").unwrap(), ("0.10.19", NodeVersion { major: 0, minor: 10, patch: 19 }));
        assert_eq!(parse_node_version("v0.10.18").unwrap(), ("0.10.18", NodeVersion { major: 0, minor: 10, patch: 18 }));
        assert_eq!(parse_node_version("v0.10.17").unwrap(), ("0.10.17", NodeVersion { major: 0, minor: 10, patch: 17 }));
        assert_eq!(parse_node_version("v0.10.16").unwrap(), ("0.10.16", NodeVersion { major: 0, minor: 10, patch: 16 }));
        assert_eq!(parse_node_version("v0.10.15").unwrap(), ("0.10.15", NodeVersion { major: 0, minor: 10, patch: 15 }));
        assert_eq!(parse_node_version("v0.10.14").unwrap(), ("0.10.14", NodeVersion { major: 0, minor: 10, patch: 14 }));
        assert_eq!(parse_node_version("v0.10.13").unwrap(), ("0.10.13", NodeVersion { major: 0, minor: 10, patch: 13 }));
        assert_eq!(parse_node_version("v0.10.12").unwrap(), ("0.10.12", NodeVersion { major: 0, minor: 10, patch: 12 }));
        assert_eq!(parse_node_version("v0.10.11").unwrap(), ("0.10.11", NodeVersion { major: 0, minor: 10, patch: 11 }));
        assert_eq!(parse_node_version("v0.10.10").unwrap(), ("0.10.10", NodeVersion { major: 0, minor: 10, patch: 10 }));
        assert_eq!(parse_node_version("v0.10.9").unwrap(), ("0.10.9", NodeVersion { major: 0, minor: 10, patch: 9 }));
        assert_eq!(parse_node_version("v0.10.8").unwrap(), ("0.10.8", NodeVersion { major: 0, minor: 10, patch: 8 }));
        assert_eq!(parse_node_version("v0.10.7").unwrap(), ("0.10.7", NodeVersion { major: 0, minor: 10, patch: 7 }));
        assert_eq!(parse_node_version("v0.10.6").unwrap(), ("0.10.6", NodeVersion { major: 0, minor: 10, patch: 6 }));
        assert_eq!(parse_node_version("v0.10.5").unwrap(), ("0.10.5", NodeVersion { major: 0, minor: 10, patch: 5 }));
        assert_eq!(parse_node_version("v0.10.4").unwrap(), ("0.10.4", NodeVersion { major: 0, minor: 10, patch: 4 }));
        assert_eq!(parse_node_version("v0.10.3").unwrap(), ("0.10.3", NodeVersion { major: 0, minor: 10, patch: 3 }));
        assert_eq!(parse_node_version("v0.10.2").unwrap(), ("0.10.2", NodeVersion { major: 0, minor: 10, patch: 2 }));
        assert_eq!(parse_node_version("v0.10.1").unwrap(), ("0.10.1", NodeVersion { major: 0, minor: 10, patch: 1 }));
        assert_eq!(parse_node_version("v0.10.0").unwrap(), ("0.10.0", NodeVersion { major: 0, minor: 10, patch: 0 }));
        assert_eq!(parse_node_version("v0.9.12").unwrap(), ("0.9.12", NodeVersion { major: 0, minor: 9, patch: 12 }));
        assert_eq!(parse_node_version("v0.9.11").unwrap(), ("0.9.11", NodeVersion { major: 0, minor: 9, patch: 11 }));
        assert_eq!(parse_node_version("v0.9.10").unwrap(), ("0.9.10", NodeVersion { major: 0, minor: 9, patch: 10 }));
        assert_eq!(parse_node_version("v0.9.9").unwrap(), ("0.9.9", NodeVersion { major: 0, minor: 9, patch: 9 }));
        assert_eq!(parse_node_version("v0.9.8").unwrap(), ("0.9.8", NodeVersion { major: 0, minor: 9, patch: 8 }));
        assert_eq!(parse_node_version("v0.9.7").unwrap(), ("0.9.7", NodeVersion { major: 0, minor: 9, patch: 7 }));
        assert_eq!(parse_node_version("v0.9.6").unwrap(), ("0.9.6", NodeVersion { major: 0, minor: 9, patch: 6 }));
        assert_eq!(parse_node_version("v0.9.5").unwrap(), ("0.9.5", NodeVersion { major: 0, minor: 9, patch: 5 }));
        assert_eq!(parse_node_version("v0.9.4").unwrap(), ("0.9.4", NodeVersion { major: 0, minor: 9, patch: 4 }));
        assert_eq!(parse_node_version("v0.9.3").unwrap(), ("0.9.3", NodeVersion { major: 0, minor: 9, patch: 3 }));
        assert_eq!(parse_node_version("v0.9.2").unwrap(), ("0.9.2", NodeVersion { major: 0, minor: 9, patch: 2 }));
        assert_eq!(parse_node_version("v0.9.1").unwrap(), ("0.9.1", NodeVersion { major: 0, minor: 9, patch: 1 }));
        assert_eq!(parse_node_version("v0.9.0").unwrap(), ("0.9.0", NodeVersion { major: 0, minor: 9, patch: 0 }));
        assert_eq!(parse_node_version("v0.8.28").unwrap(), ("0.8.28", NodeVersion { major: 0, minor: 8, patch: 28 }));
        assert_eq!(parse_node_version("v0.8.27").unwrap(), ("0.8.27", NodeVersion { major: 0, minor: 8, patch: 27 }));
        assert_eq!(parse_node_version("v0.8.26").unwrap(), ("0.8.26", NodeVersion { major: 0, minor: 8, patch: 26 }));
        assert_eq!(parse_node_version("v0.8.25").unwrap(), ("0.8.25", NodeVersion { major: 0, minor: 8, patch: 25 }));
        assert_eq!(parse_node_version("v0.8.24").unwrap(), ("0.8.24", NodeVersion { major: 0, minor: 8, patch: 24 }));
        assert_eq!(parse_node_version("v0.8.23").unwrap(), ("0.8.23", NodeVersion { major: 0, minor: 8, patch: 23 }));
        assert_eq!(parse_node_version("v0.8.22").unwrap(), ("0.8.22", NodeVersion { major: 0, minor: 8, patch: 22 }));
        assert_eq!(parse_node_version("v0.8.21").unwrap(), ("0.8.21", NodeVersion { major: 0, minor: 8, patch: 21 }));
        assert_eq!(parse_node_version("v0.8.20").unwrap(), ("0.8.20", NodeVersion { major: 0, minor: 8, patch: 20 }));
        assert_eq!(parse_node_version("v0.8.19").unwrap(), ("0.8.19", NodeVersion { major: 0, minor: 8, patch: 19 }));
        assert_eq!(parse_node_version("v0.8.18").unwrap(), ("0.8.18", NodeVersion { major: 0, minor: 8, patch: 18 }));
        assert_eq!(parse_node_version("v0.8.17").unwrap(), ("0.8.17", NodeVersion { major: 0, minor: 8, patch: 17 }));
        assert_eq!(parse_node_version("v0.8.16").unwrap(), ("0.8.16", NodeVersion { major: 0, minor: 8, patch: 16 }));
        assert_eq!(parse_node_version("v0.8.15").unwrap(), ("0.8.15", NodeVersion { major: 0, minor: 8, patch: 15 }));
        assert_eq!(parse_node_version("v0.8.14").unwrap(), ("0.8.14", NodeVersion { major: 0, minor: 8, patch: 14 }));
        assert_eq!(parse_node_version("v0.8.13").unwrap(), ("0.8.13", NodeVersion { major: 0, minor: 8, patch: 13 }));
        assert_eq!(parse_node_version("v0.8.12").unwrap(), ("0.8.12", NodeVersion { major: 0, minor: 8, patch: 12 }));
        assert_eq!(parse_node_version("v0.8.11").unwrap(), ("0.8.11", NodeVersion { major: 0, minor: 8, patch: 11 }));
        assert_eq!(parse_node_version("v0.8.10").unwrap(), ("0.8.10", NodeVersion { major: 0, minor: 8, patch: 10 }));
        assert_eq!(parse_node_version("v0.8.9").unwrap(), ("0.8.9", NodeVersion { major: 0, minor: 8, patch: 9 }));
        assert_eq!(parse_node_version("v0.8.8").unwrap(), ("0.8.8", NodeVersion { major: 0, minor: 8, patch: 8 }));
        assert_eq!(parse_node_version("v0.8.7").unwrap(), ("0.8.7", NodeVersion { major: 0, minor: 8, patch: 7 }));
        assert_eq!(parse_node_version("v0.8.6").unwrap(), ("0.8.6", NodeVersion { major: 0, minor: 8, patch: 6 }));
        assert_eq!(parse_node_version("v0.8.5").unwrap(), ("0.8.5", NodeVersion { major: 0, minor: 8, patch: 5 }));
        assert_eq!(parse_node_version("v0.8.4").unwrap(), ("0.8.4", NodeVersion { major: 0, minor: 8, patch: 4 }));
        assert_eq!(parse_node_version("v0.8.3").unwrap(), ("0.8.3", NodeVersion { major: 0, minor: 8, patch: 3 }));
        assert_eq!(parse_node_version("v0.8.2").unwrap(), ("0.8.2", NodeVersion { major: 0, minor: 8, patch: 2 }));
        assert_eq!(parse_node_version("v0.8.1").unwrap(), ("0.8.1", NodeVersion { major: 0, minor: 8, patch: 1 }));
        assert_eq!(parse_node_version("v0.8.0").unwrap(), ("0.8.0", NodeVersion { major: 0, minor: 8, patch: 0 }));
        assert_eq!(parse_node_version("v0.7.12").unwrap(), ("0.7.12", NodeVersion { major: 0, minor: 7, patch: 12 }));
        assert_eq!(parse_node_version("v0.7.11").unwrap(), ("0.7.11", NodeVersion { major: 0, minor: 7, patch: 11 }));
        assert_eq!(parse_node_version("v0.7.10").unwrap(), ("0.7.10", NodeVersion { major: 0, minor: 7, patch: 10 }));
        assert_eq!(parse_node_version("v0.7.9").unwrap(), ("0.7.9", NodeVersion { major: 0, minor: 7, patch: 9 }));
        assert_eq!(parse_node_version("v0.7.8").unwrap(), ("0.7.8", NodeVersion { major: 0, minor: 7, patch: 8 }));
        assert_eq!(parse_node_version("v0.7.7").unwrap(), ("0.7.7", NodeVersion { major: 0, minor: 7, patch: 7 }));
        assert_eq!(parse_node_version("v0.7.6").unwrap(), ("0.7.6", NodeVersion { major: 0, minor: 7, patch: 6 }));
        assert_eq!(parse_node_version("v0.7.5").unwrap(), ("0.7.5", NodeVersion { major: 0, minor: 7, patch: 5 }));
        assert_eq!(parse_node_version("v0.7.4").unwrap(), ("0.7.4", NodeVersion { major: 0, minor: 7, patch: 4 }));
        assert_eq!(parse_node_version("v0.7.3").unwrap(), ("0.7.3", NodeVersion { major: 0, minor: 7, patch: 3 }));
        assert_eq!(parse_node_version("v0.7.2").unwrap(), ("0.7.2", NodeVersion { major: 0, minor: 7, patch: 2 }));
        assert_eq!(parse_node_version("v0.7.1").unwrap(), ("0.7.1", NodeVersion { major: 0, minor: 7, patch: 1 }));
        assert_eq!(parse_node_version("v0.7.0").unwrap(), ("0.7.0", NodeVersion { major: 0, minor: 7, patch: 0 }));
        assert_eq!(parse_node_version("v0.6.21").unwrap(), ("0.6.21", NodeVersion { major: 0, minor: 6, patch: 21 }));
        assert_eq!(parse_node_version("v0.6.20").unwrap(), ("0.6.20", NodeVersion { major: 0, minor: 6, patch: 20 }));
        assert_eq!(parse_node_version("v0.6.19").unwrap(), ("0.6.19", NodeVersion { major: 0, minor: 6, patch: 19 }));
        assert_eq!(parse_node_version("v0.6.18").unwrap(), ("0.6.18", NodeVersion { major: 0, minor: 6, patch: 18 }));
        assert_eq!(parse_node_version("v0.6.17").unwrap(), ("0.6.17", NodeVersion { major: 0, minor: 6, patch: 17 }));
        assert_eq!(parse_node_version("v0.6.16").unwrap(), ("0.6.16", NodeVersion { major: 0, minor: 6, patch: 16 }));
        assert_eq!(parse_node_version("v0.6.15").unwrap(), ("0.6.15", NodeVersion { major: 0, minor: 6, patch: 15 }));
        assert_eq!(parse_node_version("v0.6.14").unwrap(), ("0.6.14", NodeVersion { major: 0, minor: 6, patch: 14 }));
        assert_eq!(parse_node_version("v0.6.13").unwrap(), ("0.6.13", NodeVersion { major: 0, minor: 6, patch: 13 }));
        assert_eq!(parse_node_version("v0.6.12").unwrap(), ("0.6.12", NodeVersion { major: 0, minor: 6, patch: 12 }));
        assert_eq!(parse_node_version("v0.6.11").unwrap(), ("0.6.11", NodeVersion { major: 0, minor: 6, patch: 11 }));
        assert_eq!(parse_node_version("v0.6.10").unwrap(), ("0.6.10", NodeVersion { major: 0, minor: 6, patch: 10 }));
        assert_eq!(parse_node_version("v0.6.9").unwrap(), ("0.6.9", NodeVersion { major: 0, minor: 6, patch: 9 }));
        assert_eq!(parse_node_version("v0.6.8").unwrap(), ("0.6.8", NodeVersion { major: 0, minor: 6, patch: 8 }));
        assert_eq!(parse_node_version("v0.6.7").unwrap(), ("0.6.7", NodeVersion { major: 0, minor: 6, patch: 7 }));
        assert_eq!(parse_node_version("v0.6.6").unwrap(), ("0.6.6", NodeVersion { major: 0, minor: 6, patch: 6 }));
        assert_eq!(parse_node_version("v0.6.5").unwrap(), ("0.6.5", NodeVersion { major: 0, minor: 6, patch: 5 }));
        assert_eq!(parse_node_version("v0.6.4").unwrap(), ("0.6.4", NodeVersion { major: 0, minor: 6, patch: 4 }));
        assert_eq!(parse_node_version("v0.6.3").unwrap(), ("0.6.3", NodeVersion { major: 0, minor: 6, patch: 3 }));
        assert_eq!(parse_node_version("v0.6.2").unwrap(), ("0.6.2", NodeVersion { major: 0, minor: 6, patch: 2 }));
        assert_eq!(parse_node_version("v0.6.1").unwrap(), ("0.6.1", NodeVersion { major: 0, minor: 6, patch: 1 }));
        assert_eq!(parse_node_version("v0.6.0").unwrap(), ("0.6.0", NodeVersion { major: 0, minor: 6, patch: 0 }));
        assert_eq!(parse_node_version("v0.5.10").unwrap(), ("0.5.10", NodeVersion { major: 0, minor: 5, patch: 10 }));
        assert_eq!(parse_node_version("v0.5.9").unwrap(), ("0.5.9", NodeVersion { major: 0, minor: 5, patch: 9 }));
        assert_eq!(parse_node_version("v0.5.8").unwrap(), ("0.5.8", NodeVersion { major: 0, minor: 5, patch: 8 }));
        assert_eq!(parse_node_version("v0.5.7").unwrap(), ("0.5.7", NodeVersion { major: 0, minor: 5, patch: 7 }));
        assert_eq!(parse_node_version("v0.5.6").unwrap(), ("0.5.6", NodeVersion { major: 0, minor: 5, patch: 6 }));
        assert_eq!(parse_node_version("v0.5.5").unwrap(), ("0.5.5", NodeVersion { major: 0, minor: 5, patch: 5 }));
        assert_eq!(parse_node_version("v0.5.4").unwrap(), ("0.5.4", NodeVersion { major: 0, minor: 5, patch: 4 }));
        assert_eq!(parse_node_version("v0.5.3").unwrap(), ("0.5.3", NodeVersion { major: 0, minor: 5, patch: 3 }));
        assert_eq!(parse_node_version("v0.5.2").unwrap(), ("0.5.2", NodeVersion { major: 0, minor: 5, patch: 2 }));
        assert_eq!(parse_node_version("v0.5.1").unwrap(), ("0.5.1", NodeVersion { major: 0, minor: 5, patch: 1 }));
        assert_eq!(parse_node_version("v0.5.0").unwrap(), ("0.5.0", NodeVersion { major: 0, minor: 5, patch: 0 }));
        assert_eq!(parse_node_version("v0.4.12").unwrap(), ("0.4.12", NodeVersion { major: 0, minor: 4, patch: 12 }));
        assert_eq!(parse_node_version("v0.4.11").unwrap(), ("0.4.11", NodeVersion { major: 0, minor: 4, patch: 11 }));
        assert_eq!(parse_node_version("v0.4.10").unwrap(), ("0.4.10", NodeVersion { major: 0, minor: 4, patch: 10 }));
        assert_eq!(parse_node_version("v0.4.9").unwrap(), ("0.4.9", NodeVersion { major: 0, minor: 4, patch: 9 }));
        assert_eq!(parse_node_version("v0.4.8").unwrap(), ("0.4.8", NodeVersion { major: 0, minor: 4, patch: 8 }));
        assert_eq!(parse_node_version("v0.4.7").unwrap(), ("0.4.7", NodeVersion { major: 0, minor: 4, patch: 7 }));
        assert_eq!(parse_node_version("v0.4.6").unwrap(), ("0.4.6", NodeVersion { major: 0, minor: 4, patch: 6 }));
        assert_eq!(parse_node_version("v0.4.5").unwrap(), ("0.4.5", NodeVersion { major: 0, minor: 4, patch: 5 }));
        assert_eq!(parse_node_version("v0.4.4").unwrap(), ("0.4.4", NodeVersion { major: 0, minor: 4, patch: 4 }));
        assert_eq!(parse_node_version("v0.4.3").unwrap(), ("0.4.3", NodeVersion { major: 0, minor: 4, patch: 3 }));
        assert_eq!(parse_node_version("v0.4.2").unwrap(), ("0.4.2", NodeVersion { major: 0, minor: 4, patch: 2 }));
        assert_eq!(parse_node_version("v0.4.1").unwrap(), ("0.4.1", NodeVersion { major: 0, minor: 4, patch: 1 }));
        assert_eq!(parse_node_version("v0.4.0").unwrap(), ("0.4.0", NodeVersion { major: 0, minor: 4, patch: 0 }));
        assert_eq!(parse_node_version("v0.3.8").unwrap(), ("0.3.8", NodeVersion { major: 0, minor: 3, patch: 8 }));
        assert_eq!(parse_node_version("v0.3.7").unwrap(), ("0.3.7", NodeVersion { major: 0, minor: 3, patch: 7 }));
        assert_eq!(parse_node_version("v0.3.6").unwrap(), ("0.3.6", NodeVersion { major: 0, minor: 3, patch: 6 }));
        assert_eq!(parse_node_version("v0.3.5").unwrap(), ("0.3.5", NodeVersion { major: 0, minor: 3, patch: 5 }));
        assert_eq!(parse_node_version("v0.3.4").unwrap(), ("0.3.4", NodeVersion { major: 0, minor: 3, patch: 4 }));
        assert_eq!(parse_node_version("v0.3.3").unwrap(), ("0.3.3", NodeVersion { major: 0, minor: 3, patch: 3 }));
        assert_eq!(parse_node_version("v0.3.2").unwrap(), ("0.3.2", NodeVersion { major: 0, minor: 3, patch: 2 }));
        assert_eq!(parse_node_version("v0.3.1").unwrap(), ("0.3.1", NodeVersion { major: 0, minor: 3, patch: 1 }));
        assert_eq!(parse_node_version("v0.3.0").unwrap(), ("0.3.0", NodeVersion { major: 0, minor: 3, patch: 0 }));
        assert_eq!(parse_node_version("v0.2.6").unwrap(), ("0.2.6", NodeVersion { major: 0, minor: 2, patch: 6 }));
        assert_eq!(parse_node_version("v0.2.5").unwrap(), ("0.2.5", NodeVersion { major: 0, minor: 2, patch: 5 }));
        assert_eq!(parse_node_version("v0.2.4").unwrap(), ("0.2.4", NodeVersion { major: 0, minor: 2, patch: 4 }));
        assert_eq!(parse_node_version("v0.2.3").unwrap(), ("0.2.3", NodeVersion { major: 0, minor: 2, patch: 3 }));
        assert_eq!(parse_node_version("v0.2.2").unwrap(), ("0.2.2", NodeVersion { major: 0, minor: 2, patch: 2 }));
        assert_eq!(parse_node_version("v0.2.1").unwrap(), ("0.2.1", NodeVersion { major: 0, minor: 2, patch: 1 }));
        assert_eq!(parse_node_version("v0.2.0").unwrap(), ("0.2.0", NodeVersion { major: 0, minor: 2, patch: 0 }));
        assert_eq!(parse_node_version("v0.1.104").unwrap(), ("0.1.104", NodeVersion { major: 0, minor: 1, patch: 104 }));
        assert_eq!(parse_node_version("v0.1.103").unwrap(), ("0.1.103", NodeVersion { major: 0, minor: 1, patch: 103 }));
        assert_eq!(parse_node_version("v0.1.102").unwrap(), ("0.1.102", NodeVersion { major: 0, minor: 1, patch: 102 }));
        assert_eq!(parse_node_version("v0.1.101").unwrap(), ("0.1.101", NodeVersion { major: 0, minor: 1, patch: 101 }));
        assert_eq!(parse_node_version("v0.1.100").unwrap(), ("0.1.100", NodeVersion { major: 0, minor: 1, patch: 100 }));
        assert_eq!(parse_node_version("v0.1.99").unwrap(), ("0.1.99", NodeVersion { major: 0, minor: 1, patch: 99 }));
        assert_eq!(parse_node_version("v0.1.98").unwrap(), ("0.1.98", NodeVersion { major: 0, minor: 1, patch: 98 }));
        assert_eq!(parse_node_version("v0.1.97").unwrap(), ("0.1.97", NodeVersion { major: 0, minor: 1, patch: 97 }));
        assert_eq!(parse_node_version("v0.1.96").unwrap(), ("0.1.96", NodeVersion { major: 0, minor: 1, patch: 96 }));
        assert_eq!(parse_node_version("v0.1.95").unwrap(), ("0.1.95", NodeVersion { major: 0, minor: 1, patch: 95 }));
        assert_eq!(parse_node_version("v0.1.94").unwrap(), ("0.1.94", NodeVersion { major: 0, minor: 1, patch: 94 }));
        assert_eq!(parse_node_version("v0.1.93").unwrap(), ("0.1.93", NodeVersion { major: 0, minor: 1, patch: 93 }));
        assert_eq!(parse_node_version("v0.1.92").unwrap(), ("0.1.92", NodeVersion { major: 0, minor: 1, patch: 92 }));
        assert_eq!(parse_node_version("v0.1.91").unwrap(), ("0.1.91", NodeVersion { major: 0, minor: 1, patch: 91 }));
        assert_eq!(parse_node_version("v0.1.90").unwrap(), ("0.1.90", NodeVersion { major: 0, minor: 1, patch: 90 }));
        assert_eq!(parse_node_version("v0.1.33").unwrap(), ("0.1.33", NodeVersion { major: 0, minor: 1, patch: 33 }));
        assert_eq!(parse_node_version("v0.1.32").unwrap(), ("0.1.32", NodeVersion { major: 0, minor: 1, patch: 32 }));
        assert_eq!(parse_node_version("v0.1.31").unwrap(), ("0.1.31", NodeVersion { major: 0, minor: 1, patch: 31 }));
        assert_eq!(parse_node_version("v0.1.30").unwrap(), ("0.1.30", NodeVersion { major: 0, minor: 1, patch: 30 }));
        assert_eq!(parse_node_version("v0.1.29").unwrap(), ("0.1.29", NodeVersion { major: 0, minor: 1, patch: 29 }));
        assert_eq!(parse_node_version("v0.1.28").unwrap(), ("0.1.28", NodeVersion { major: 0, minor: 1, patch: 28 }));
        assert_eq!(parse_node_version("v0.1.27").unwrap(), ("0.1.27", NodeVersion { major: 0, minor: 1, patch: 27 }));
        assert_eq!(parse_node_version("v0.1.26").unwrap(), ("0.1.26", NodeVersion { major: 0, minor: 1, patch: 26 }));
        assert_eq!(parse_node_version("v0.1.25").unwrap(), ("0.1.25", NodeVersion { major: 0, minor: 1, patch: 25 }));
        assert_eq!(parse_node_version("v0.1.24").unwrap(), ("0.1.24", NodeVersion { major: 0, minor: 1, patch: 24 }));
        assert_eq!(parse_node_version("v0.1.23").unwrap(), ("0.1.23", NodeVersion { major: 0, minor: 1, patch: 23 }));
        assert_eq!(parse_node_version("v0.1.22").unwrap(), ("0.1.22", NodeVersion { major: 0, minor: 1, patch: 22 }));
        assert_eq!(parse_node_version("v0.1.21").unwrap(), ("0.1.21", NodeVersion { major: 0, minor: 1, patch: 21 }));
        assert_eq!(parse_node_version("v0.1.20").unwrap(), ("0.1.20", NodeVersion { major: 0, minor: 1, patch: 20 }));
        assert_eq!(parse_node_version("v0.1.19").unwrap(), ("0.1.19", NodeVersion { major: 0, minor: 1, patch: 19 }));
        assert_eq!(parse_node_version("v0.1.18").unwrap(), ("0.1.18", NodeVersion { major: 0, minor: 1, patch: 18 }));
        assert_eq!(parse_node_version("v0.1.17").unwrap(), ("0.1.17", NodeVersion { major: 0, minor: 1, patch: 17 }));
        assert_eq!(parse_node_version("v0.1.16").unwrap(), ("0.1.16", NodeVersion { major: 0, minor: 1, patch: 16 }));
        assert_eq!(parse_node_version("v0.1.15").unwrap(), ("0.1.15", NodeVersion { major: 0, minor: 1, patch: 15 }));
        assert_eq!(parse_node_version("v0.1.14").unwrap(), ("0.1.14", NodeVersion { major: 0, minor: 1, patch: 14 }));
    }
}
