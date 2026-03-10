use rustc_hash::FxHashMap;
use serde::Deserialize;
use smol_str::SmolStr;
use std::path::PathBuf;
use std::sync::Arc;

#[cfg(windows)]
use std::ffi::OsString;

use crate::tool::{ToolDownInfo, ToolInfo, Version, VersionFilter};
use crate::HttpClient;

pub struct Tool {
    client: Arc<HttpClient>,
    info: ToolInfo,
}

const REGISTRY_URL: &str = "https://registry.npmjs.org/pnpm";

impl crate::tool::GeneralTool for Tool {
    fn info(&self) -> &ToolInfo {
        &self.info
    }

    async fn fetch_versions(
        &self,
        _platform: Option<SmolStr>,
        _flavor: Option<SmolStr>,
        version_filter: VersionFilter,
    ) -> anyhow::Result<Vec<Version>> {
        let version_filter = ignore_lts_only(version_filter);
        let version_filter = PnpmVersionFilter::try_from(&version_filter)?;

        let registry = self.fetch_registry(&self.client).await?;
        let mut releases: Vec<(PnpmVersion, SmolStr)> = registry
            .versions
            .keys()
            .filter_map(|raw| {
                let version = parse_pnpm_version(raw)
                    .map_err(|e| log::error!("Failed to parse pnpm version '{}': {}", raw, e))
                    .ok()?;
                if !version_filter.matches(raw, &version) {
                    return None;
                }
                Some((version, raw.clone()))
            })
            .collect();

        releases.sort_by(|a, b| a.0.cmp(&b.0));
        let versions = releases
            .into_iter()
            .map(|(_, raw)| Version {
                version: raw,
                is_lts: false,
            })
            .collect();

        Ok(versions)
    }

    async fn get_down_info(
        &self,
        _platform: Option<SmolStr>,
        _flavor: Option<SmolStr>,
        version_filter: VersionFilter,
    ) -> anyhow::Result<ToolDownInfo> {
        let version_filter = ignore_lts_only(version_filter);
        let version_filter = PnpmVersionFilter::try_from(&version_filter)?;

        let registry = self.fetch_registry(&self.client).await?;
        let best = registry
            .versions
            .iter()
            .filter_map(|(raw, info)| {
                let version = parse_pnpm_version(raw)
                    .map_err(|e| log::error!("Failed to parse pnpm version '{}': {}", raw, e))
                    .ok()?;
                if !version_filter.matches(raw, &version) {
                    return None;
                }
                Some((version, raw.clone(), info))
            })
            .max_by(|a, b| a.0.cmp(&b.0));

        match best {
            Some((_, raw_version, info)) => Ok(ToolDownInfo {
                version: Version {
                    version: raw_version,
                    is_lts: false,
                },
                url: info.dist.tarball.clone(),
                hash: crate::FileHash {
                    sha1: Some(info.dist.shasum.clone()),
                    ..Default::default()
                },
            }),
            None => Err(anyhow::anyhow!("No download URL found.")),
        }
    }

    fn find_best_matching_local_tag<'a, I>(
        &self,
        tags_and_versions: I,
        version_filter: &VersionFilter,
    ) -> Option<SmolStr>
    where
        I: Iterator<Item = (&'a str, &'a Version)>,
    {
        let version_filter = ignore_lts_only(version_filter.clone());
        let version_filter = PnpmVersionFilter::try_from(&version_filter).ok()?;
        tags_and_versions
            .filter_map(|(tag, version_info)| {
                let raw_version = &*version_info.version;
                let version = parse_pnpm_version(raw_version).ok()?;
                if !version_filter.matches(raw_version, &version) {
                    return None;
                }
                Some((version, SmolStr::from(tag)))
            })
            .max_by(|a, b| a.0.cmp(&b.0))
            .map(|(_, tag)| tag)
    }

    fn entry_path(&self, tag_dir: PathBuf) -> anyhow::Result<PathBuf> {
        let mut p = tag_dir;
        p.push("bin");
        p.push("pnpm.cjs");
        Ok(p)
    }

    #[cfg(windows)]
    async fn run(&self, entry_path: PathBuf, args: Vec<OsString>) -> anyhow::Result<()> {
        crate::spawn_blocking(move || {
            let mut command = std::process::Command::new("node.exe");
            command.arg(entry_path);
            command.args(args);
            command.spawn()?.wait()?;
            Ok(())
        })
        .await
    }
}

impl Tool {
    pub fn new(client: Arc<HttpClient>) -> Self {
        Tool {
            client,
            info: ToolInfo {
                about: "Fast, disk space efficient package manager for Node.js".into(),
                after_long_help: None,
                all_platforms: None,
                default_platform: None,
                all_flavors: None,
                default_flavor: None,
            },
        }
    }

    async fn fetch_registry(&self, client: &HttpClient) -> anyhow::Result<RegistryDto> {
        client
            .get(REGISTRY_URL)
            .header("Accept", "application/vnd.npm.install-v1+json")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .map_err(Into::into)
    }
}

#[derive(Debug, Deserialize)]
struct RegistryDto {
    versions: FxHashMap<SmolStr, VersionDto>,
}

#[derive(Debug, Deserialize)]
struct VersionDto {
    dist: DistDto,
}

#[derive(Debug, Deserialize)]
struct DistDto {
    shasum: SmolStr,
    tarball: SmolStr,
}

/// Represents a parsed pnpm version.
/// Pre-release versions (e.g. 11.0.0-alpha.12) sort before their release counterpart.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct PnpmVersion {
    major: u32,
    minor: u32,
    patch: u32,
    pre: PreRelease,
}

/// Pre-release ordering: None (stable release) > any pre-release tag.
/// Pre-release tags are compared lexicographically by their raw string.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PreRelease {
    Some(String),
    None,
}

impl PartialOrd for PreRelease {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PreRelease {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (self, other) {
            (PreRelease::None, PreRelease::None) => std::cmp::Ordering::Equal,
            (PreRelease::None, PreRelease::Some(_)) => std::cmp::Ordering::Greater,
            (PreRelease::Some(_), PreRelease::None) => std::cmp::Ordering::Less,
            (PreRelease::Some(a), PreRelease::Some(b)) => a.cmp(b),
        }
    }
}

impl PartialOrd for PnpmVersion {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PnpmVersion {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.major
            .cmp(&other.major)
            .then(self.minor.cmp(&other.minor))
            .then(self.patch.cmp(&other.patch))
            .then(self.pre.cmp(&other.pre))
    }
}

struct PnpmVersionFilter {
    allow_prerelease: bool,
    version_prefix: Option<crate::tool::VersionPrefix>,
    exact_version: Option<SmolStr>,
}

impl PnpmVersionFilter {
    fn matches(&self, raw_version: &str, version: &PnpmVersion) -> bool {
        if !self.allow_prerelease && version.pre != PreRelease::None {
            return false;
        }
        if self
            .version_prefix
            .is_some_and(|p| !p.matches(version.major, version.minor, version.patch))
        {
            return false;
        }
        if self
            .exact_version
            .as_ref()
            .is_some_and(|ev| ev != raw_version)
        {
            return false;
        }
        true
    }
}

impl TryFrom<&VersionFilter> for PnpmVersionFilter {
    type Error = anyhow::Error;

    fn try_from(value: &VersionFilter) -> Result<Self, Self::Error> {
        Ok(Self {
            allow_prerelease: value.allow_prerelease,
            version_prefix: value.version_prefix,
            exact_version: value.exact_version.clone(),
        })
    }
}

fn ignore_lts_only(mut version_filter: VersionFilter) -> VersionFilter {
    if version_filter.lts_only {
        log::warn!(
            "`--lts-only` is ignored for `pnpm` because this tool does not define LTS releases."
        );
        version_filter.lts_only = false;
    }
    version_filter
}

/// Parses a pnpm version string (semver with optional pre-release).
/// Examples: "9.9.0", "11.0.0-alpha.12", "1.24.0-0"
pub fn parse_pnpm_version(s: &str) -> anyhow::Result<PnpmVersion> {
    let (main_part, pre) = match s.find('-') {
        Some(idx) => {
            let pre_str = &s[idx + 1..];
            if pre_str.is_empty() {
                anyhow::bail!("Empty pre-release tag in '{}'", s);
            }
            (&s[..idx], PreRelease::Some(pre_str.to_string()))
        }
        None => (s, PreRelease::None),
    };

    let parts: Vec<&str> = main_part.split('.').collect();
    if parts.len() != 3 {
        anyhow::bail!("Invalid version format '{}', expected major.minor.patch", s);
    }

    let major = parts[0]
        .parse::<u32>()
        .map_err(|e| anyhow::anyhow!("Invalid major version '{}' in '{}': {}", parts[0], s, e))?;
    let minor = parts[1]
        .parse::<u32>()
        .map_err(|e| anyhow::anyhow!("Invalid minor version '{}' in '{}': {}", parts[1], s, e))?;
    let patch = parts[2]
        .parse::<u32>()
        .map_err(|e| anyhow::anyhow!("Invalid patch version '{}' in '{}': {}", parts[2], s, e))?;

    Ok(PnpmVersion {
        major,
        minor,
        patch,
        pre,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::VersionFilter;

    #[test]
    fn test_parse_pnpm_version() {
        let v = parse_pnpm_version("9.9.0").unwrap();
        assert_eq!(
            v,
            PnpmVersion {
                major: 9,
                minor: 9,
                patch: 0,
                pre: PreRelease::None
            }
        );

        let v = parse_pnpm_version("11.0.0-alpha.12").unwrap();
        assert_eq!(
            v,
            PnpmVersion {
                major: 11,
                minor: 0,
                patch: 0,
                pre: PreRelease::Some("alpha.12".into())
            }
        );

        let v = parse_pnpm_version("1.24.0-0").unwrap();
        assert_eq!(
            v,
            PnpmVersion {
                major: 1,
                minor: 24,
                patch: 0,
                pre: PreRelease::Some("0".into())
            }
        );

        let v = parse_pnpm_version("0.69.0-beta.1").unwrap();
        assert_eq!(
            v,
            PnpmVersion {
                major: 0,
                minor: 69,
                patch: 0,
                pre: PreRelease::Some("beta.1".into())
            }
        );
    }

    #[test]
    fn test_version_ordering() {
        let v1 = parse_pnpm_version("9.9.0").unwrap();
        let v2 = parse_pnpm_version("9.10.0").unwrap();
        assert!(v1 < v2);

        // Stable release > pre-release with same major.minor.patch
        let stable = parse_pnpm_version("11.0.0").unwrap();
        let alpha = parse_pnpm_version("11.0.0-alpha.1").unwrap();
        assert!(stable > alpha);

        // Pre-release ordering within same version
        let a1 = parse_pnpm_version("11.0.0-alpha.1").unwrap();
        let a2 = parse_pnpm_version("11.0.0-alpha.2").unwrap();
        assert!(a1 < a2);
    }

    #[test]
    fn test_parse_invalid() {
        assert!(parse_pnpm_version("").is_err());
        assert!(parse_pnpm_version("9.9").is_err());
        assert!(parse_pnpm_version("abc").is_err());
        assert!(parse_pnpm_version("9.9.0-").is_err());
    }

    #[test]
    fn version_filter_ignores_lts_only() {
        let filter = PnpmVersionFilter::try_from(&VersionFilter {
            lts_only: true,
            allow_prerelease: false,
            version_prefix: None,
            exact_version: None,
        })
        .unwrap();
        let version = parse_pnpm_version("9.9.0").unwrap();

        assert!(filter.matches("9.9.0", &version));
    }
}
