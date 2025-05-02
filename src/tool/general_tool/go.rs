use anyhow::Context;
use serde::Deserialize;
use smol_str::{SmolStr, ToSmolStr};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::{collections::HashSet, sync::Arc};

use crate::HttpClient;
use crate::{
    platform::{cpu, create_platform_string, current_cpu, current_os, os},
    tool::{InstallVersion, ToolDownInfo, ToolInfo, Version},
};

pub struct Tool {
    client: Arc<HttpClient>,
    info: ToolInfo,
    corresponding_dto_cpu_os: Vec<(&'static str, &'static str)>,
}

const BASE_URL: &str = "https://golang.org/dl/";

use async_trait::async_trait;

#[async_trait]
impl crate::tool::GeneralTool for Tool {
    fn info(&self) -> &ToolInfo {
        &self.info
    }

    async fn fetch_versions(
        &self,
        platform: Option<SmolStr>,
        _flavor: Option<SmolStr>,
        major_version: Option<SmolStr>,
    ) -> anyhow::Result<Vec<Version>> {
        let platform = platform.ok_or_else(|| anyhow::anyhow!("Platform is required"))?;
        let (cpu, os) = self.get_dto_cpu_os(&platform);

        let mut releases = self
            .fetch_go_releases(
                &self.client,
                cpu,
                os,
                major_version.map(|v| InstallVersion::Latest { major_version: v }),
            )
            .await?;

        releases.sort_by(|a, b| b.version.cmp(&a.version));
        let mut versions = Vec::new();
        let mut version_set = HashSet::new();
        for release in releases {
            let version_raw = release.version_raw.clone();
            if version_set.insert(version_raw.clone()) {
                versions.push(Version {
                    version: version_raw,
                    major_version: release.version.major.to_smolstr(),
                    is_lts: release.lts,
                });
            }
        }

        Ok(versions)
    }

    async fn get_down_info(
        &self,
        platform: Option<SmolStr>,
        _flavor: Option<SmolStr>,
        version: InstallVersion,
    ) -> anyhow::Result<ToolDownInfo> {
        let platform = platform.ok_or_else(|| anyhow::anyhow!("Platform is required"))?;
        let (cpu, os) = self.get_dto_cpu_os(&platform);

        let mut releases = self
            .fetch_go_releases(&self.client, cpu, os, Some(version))
            .await?;
        releases.sort_by(|a, b| b.version.cmp(&a.version));
        match releases.into_iter().next() {
            Some(item) => Ok(ToolDownInfo {
                version: item.version_raw,
                url: item.download_url.into(),
                hash: crate::FileHash {
                    sha256: Some(item.sha256.into()),
                    ..Default::default()
                },
            }),
            None => Err(anyhow::anyhow!("No download URL found.")),
        }
    }

    fn bin_path(&self, instance_dir: &Path) -> anyhow::Result<PathBuf> {
        let go_path = instance_dir.join("bin").join("go");
        Ok(go_path)
    }
}

impl Tool {
    pub fn new(client: Arc<HttpClient>) -> Self {
        let (all_platforms, corresponding_dto_cpu_os) =
            Self::get_platforms_and_corresponding_dto_cpu_os();

        let default_platform = current_cpu().and_then(|cpu| {
            let os = current_os()?;
            let p = create_platform_string(cpu, os);
            all_platforms.iter().find(|&k| p == *k).cloned()
        });

        Tool {
            client,
            info: ToolInfo {
                name: "go".into(),
                about: "Go programming language".into(),
                after_long_help: None,
                all_platforms: Some(all_platforms),
                default_platform,
                all_flavors: None,
                default_flavor: None,
                version_is_major: true,
            },
            corresponding_dto_cpu_os,
        }
    }

    fn get_platforms_and_corresponding_dto_cpu_os(
    ) -> (Vec<SmolStr>, Vec<(&'static str, &'static str)>) {
        let mut platforms = Vec::new();
        let mut dto_cpu_os = Vec::new();

        let mut add = |cpu: &str, os: &str, dto_cpu: &'static str, dto_os: &'static str| {
            platforms.push(create_platform_string(cpu, os));
            dto_cpu_os.push((dto_cpu, dto_os));
        };

        // --- Linux ---
        add(cpu::X86, os::LINUX, "386", "linux");
        add(cpu::X64, os::LINUX, "amd64", "linux");
        add(cpu::ARM64, os::LINUX, "arm64", "linux");
        add(cpu::ARMV6L, os::LINUX, "armv6l", "linux");
        add(cpu::LOONG64, os::LINUX, "loong64", "linux");
        add(cpu::MIPS32, os::LINUX, "mips", "linux");
        add(cpu::MIPS64, os::LINUX, "mips64", "linux");
        add(cpu::MIPS64LE, os::LINUX, "mips64le", "linux");
        add(cpu::MIPS32LE, os::LINUX, "mipsle", "linux");
        add(cpu::PPC64, os::LINUX, "ppc64", "linux");
        add(cpu::PPC64LE, os::LINUX, "ppc64le", "linux");
        add(cpu::RISCV64, os::LINUX, "riscv64", "linux");
        add(cpu::S390X, os::LINUX, "s390x", "linux");

        // --- Windows ---
        add(cpu::X86, os::WIN, "386", "windows");
        add(cpu::X64, os::WIN, "amd64", "windows");
        add(cpu::ARM32, os::WIN, "arm", "windows");
        add(cpu::ARM64, os::WIN, "arm64", "windows");
        add(cpu::ARMV6L, os::WIN, "armv6l", "windows");

        // --- macOS (Darwin) ---
        add(cpu::X86, os::MAC, "386", "darwin");
        add(cpu::X64, os::MAC, "amd64", "darwin");
        add(cpu::ARM64, os::MAC, "arm64", "darwin");

        // --- FreeBSD ---
        add(cpu::X86, os::FREEBSD, "386", "freebsd");
        add(cpu::X64, os::FREEBSD, "amd64", "freebsd");
        add(cpu::ARM32, os::FREEBSD, "arm", "freebsd");
        add(cpu::ARM64, os::FREEBSD, "arm64", "freebsd");
        add(cpu::ARMV6L, os::FREEBSD, "armv6l", "freebsd");
        add(cpu::RISCV64, os::FREEBSD, "riscv64", "freebsd");

        // --- AIX ---
        add(cpu::PPC64, os::AIX, "ppc64", "aix");

        // --- DragonflyBSD ---
        add(cpu::X64, os::DRAGONFLYBSD, "amd64", "dragonfly");

        // --- Illumos ---
        add(cpu::X64, os::ILLUMOS, "amd64", "illumos");

        // --- NetBSD ---
        add(cpu::X86, os::NETBSD, "386", "netbsd");
        add(cpu::X64, os::NETBSD, "amd64", "netbsd");
        add(cpu::ARM32, os::NETBSD, "arm", "netbsd");
        add(cpu::ARM64, os::NETBSD, "arm64", "netbsd");
        add(cpu::ARMV6L, os::NETBSD, "armv6l", "netbsd");

        // --- OpenBSD ---
        add(cpu::X86, os::OPENBSD, "386", "openbsd");
        add(cpu::X64, os::OPENBSD, "amd64", "openbsd");
        add(cpu::ARM32, os::OPENBSD, "arm", "openbsd");
        add(cpu::ARM64, os::OPENBSD, "arm64", "openbsd");
        add(cpu::ARMV6L, os::OPENBSD, "armv6l", "openbsd");
        add(cpu::PPC64, os::OPENBSD, "ppc64", "openbsd");
        add(cpu::RISCV64, os::OPENBSD, "riscv64", "openbsd");

        // --- Plan 9 ---
        add(cpu::X86, os::PLAN9, "386", "plan9");
        add(cpu::X64, os::PLAN9, "amd64", "plan9");
        add(cpu::ARM32, os::PLAN9, "arm", "plan9");
        add(cpu::ARMV6L, os::PLAN9, "armv6l", "plan9");

        // --- Solaris ---
        add(cpu::X64, os::SOLARIS, "amd64", "solaris");

        (platforms, dto_cpu_os)
    }

    fn get_dto_cpu_os(&self, platform: &SmolStr) -> (&'static str, &'static str) {
        let platform_index = self
            .info
            .all_platforms
            .as_ref()
            .unwrap()
            .iter()
            .position(|p| p == platform)
            .unwrap();
        self.corresponding_dto_cpu_os[platform_index]
    }

    #[allow(clippy::type_complexity)] // I don't get why `Box<dyn Fn(&str, &GoVersion) -> bool>` is considered too complex
    async fn fetch_go_releases(
        &self,
        client: &HttpClient,
        cpu: &str,
        os: &str,
        version: Option<InstallVersion>,
    ) -> anyhow::Result<Vec<ReleaseItem>> {
        let response: Vec<GoDlDto> = client
            .get(BASE_URL)
            .query(&[("mode", "json"), ("include", "all")])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let verify_version: Box<dyn Fn(&str, &GoVersion) -> bool> = match version {
            Some(InstallVersion::Latest { major_version }) => Box::new({
                let major_version = major_version
                    .parse::<u32>()
                    .context("Failed to parse major version")?;
                move |_, version| version.major == major_version
            }),
            Some(InstallVersion::Specific { version }) => {
                Box::new(move |version_raw, _| version_raw == version)
            }
            None => Box::new(|_, _| true),
        };

        let releases: Vec<ReleaseItem> = response
            .into_iter()
            .filter_map(|r| {
                let version = GoVersion::from_str(&r.version)
                    .map_err(|e| log::error!("Failed to parse Go version: {}", e))
                    .ok()?;
                let version_raw = &r.version[2..]; // strip "go" prefix
                if !verify_version(version_raw, &version) {
                    return None;
                }

                let file = r
                    .files
                    .iter()
                    .find(|f| f.os == os && f.arch == cpu && f.kind == "archive")?;

                Some(ReleaseItem {
                    download_url: format!("{}/{}", BASE_URL, file.filename),
                    sha256: file.sha256.clone(),
                    version_raw: version_raw.into(),
                    lts: version.pre_release == PreRelease::None,
                    version,
                })
            })
            .collect();

        Ok(releases)
    }
}

struct ReleaseItem {
    download_url: String,
    sha256: String,
    version_raw: SmolStr,
    version: GoVersion,
    lts: bool,
}

#[derive(Debug, Deserialize)]
struct GoDlDto {
    version: String,
    files: Vec<FileDto>,
}

#[derive(Debug, Deserialize)]
struct FileDto {
    filename: String,
    os: String,
    arch: String,
    sha256: String,
    kind: String,
}

/// Represents a Go version pre-release stage.
/// Note: The order of variants is important for the derived `Ord`.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
enum PreRelease {
    Beta(u32),
    Rc(u32),
    /// Represents a final release (no pre-release tag).
    /// This must be the last variant for correct ordering (Beta < Rc < None).
    None,
}

/// Represents a parsed Go version.
/// Derives comparison traits to order by major, minor, patch, and then pre-release status.
#[derive(PartialOrd, Ord, Debug, PartialEq, Eq, Clone)]
pub struct GoVersion {
    major: u32,
    minor: u32,
    patch: u32,
    pre_release: PreRelease,
}

// /// Implements ordering for GoVersion.
// /// Comparison happens field by field: major, minor, patch, pre_release.
// impl Ord for GoVersion {
//     fn cmp(&self, other: &Self) -> Ordering {
//         self.major
//             .cmp(&other.major)
//             .then_with(|| self.minor.cmp(&other.minor))
//             .then_with(|| self.patch.cmp(&other.patch))
//             .then_with(|| self.pre_release.cmp(&other.pre_release))
//     }
// }

// /// Implements partial ordering for GoVersion.
// impl PartialOrd for GoVersion {
//     fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
//         Some(self.cmp(other))
//     }
// }

/// Custom error type for parsing Go versions.
#[derive(Debug, PartialEq, Eq)]
pub struct ParseGoVersionError(String);

impl std::fmt::Display for ParseGoVersionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Failed to parse Go version: {}", self.0)
    }
}

impl std::error::Error for ParseGoVersionError {}

/// Implements the `FromStr` trait to allow parsing from string slices.
impl FromStr for GoVersion {
    type Err = ParseGoVersionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // 1. Check and remove the "go" prefix
        let version_part = s.strip_prefix("go").ok_or_else(|| {
            ParseGoVersionError(format!("Input string '{}' does not start with 'go'", s))
        })?;

        if version_part.is_empty() {
            return Err(ParseGoVersionError(format!(
                "Input string '{}' has no version part after 'go'",
                s
            )));
        }

        // 2. Identify and separate pre-release identifier (beta/rc)
        let (main_part, pre_release) = {
            if let Some(index) = version_part.find("beta") {
                let num_str = &version_part[index + 4..];
                if num_str.is_empty() {
                    return Err(ParseGoVersionError(format!(
                        "Missing number after 'beta' in '{}'",
                        s
                    )));
                }
                let num = num_str.parse::<u32>().map_err(|e| {
                    ParseGoVersionError(format!(
                        "Invalid beta number '{}' in '{}': {}",
                        num_str, s, e
                    ))
                })?;
                (&version_part[..index], PreRelease::Beta(num))
            } else if let Some(index) = version_part.find("rc") {
                let num_str = &version_part[index + 2..];
                if num_str.is_empty() {
                    return Err(ParseGoVersionError(format!(
                        "Missing number after 'rc' in '{}'",
                        s
                    )));
                }
                let num = num_str.parse::<u32>().map_err(|e| {
                    ParseGoVersionError(format!(
                        "Invalid rc number '{}' in '{}': {}",
                        num_str, s, e
                    ))
                })?;
                (&version_part[..index], PreRelease::Rc(num))
            } else {
                (version_part, PreRelease::None)
            }
        };

        // 3. Parse major, minor, patch numbers
        let mut parts = main_part.split('.');

        let major_str = parts.next().ok_or_else(|| {
            ParseGoVersionError(format!("Missing major version number in '{}'", s))
        })?;
        if major_str.is_empty() && main_part.starts_with('.') {
            // Handle cases like "go.1" which are invalid
            return Err(ParseGoVersionError(format!(
                "Major version part is empty in '{}'",
                s
            )));
        }
        let major = major_str.parse::<u32>().map_err(|e| {
            ParseGoVersionError(format!(
                "Invalid major version '{}' in '{}': {}",
                major_str, s, e
            ))
        })?;

        // Default minor and patch to 0 if not present
        let mut minor = 0;
        let mut patch = 0;

        if let Some(minor_str) = parts.next() {
            if minor_str.is_empty() {
                // Handle cases like "go1." or "go1..2"
                return Err(ParseGoVersionError(format!(
                    "Minor version part is empty in '{}'",
                    s
                )));
            }
            minor = minor_str.parse::<u32>().map_err(|e| {
                ParseGoVersionError(format!(
                    "Invalid minor version '{}' in '{}': {}",
                    minor_str, s, e
                ))
            })?;

            if let Some(patch_str) = parts.next() {
                if patch_str.is_empty() {
                    // Handle cases like "go1.2."
                    return Err(ParseGoVersionError(format!(
                        "Patch version part is empty in '{}'",
                        s
                    )));
                }
                patch = patch_str.parse::<u32>().map_err(|e| {
                    ParseGoVersionError(format!(
                        "Invalid patch version '{}' in '{}': {}",
                        patch_str, s, e
                    ))
                })?;
            }
        }

        // Check if there are too many parts (e.g., "go1.2.3.4")
        if parts.next().is_some() {
            return Err(ParseGoVersionError(format!(
                "Too many version parts (expected max 3) in '{}'",
                s
            )));
        }

        Ok(GoVersion {
            major,
            minor,
            patch,
            pre_release,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*; // Imports items from the parent module (GoVersion, PreRelease, etc.)

    #[test]
    fn test_parse_simple_versions() {
        assert_eq!(
            "go1".parse::<GoVersion>().unwrap(),
            GoVersion {
                major: 1,
                minor: 0,
                patch: 0,
                pre_release: PreRelease::None
            }
        );
        assert_eq!(
            "go1.10".parse::<GoVersion>().unwrap(),
            GoVersion {
                major: 1,
                minor: 10,
                patch: 0,
                pre_release: PreRelease::None
            }
        );
        assert_eq!(
            "go1.10.5".parse::<GoVersion>().unwrap(),
            GoVersion {
                major: 1,
                minor: 10,
                patch: 5,
                pre_release: PreRelease::None
            }
        );
        assert_eq!(
            "go1.2.2".parse::<GoVersion>().unwrap(),
            GoVersion {
                major: 1,
                minor: 2,
                patch: 2,
                pre_release: PreRelease::None
            }
        );
    }

    #[test]
    fn test_parse_prerelease_versions() {
        assert_eq!(
            "go1.10beta1".parse::<GoVersion>().unwrap(),
            GoVersion {
                major: 1,
                minor: 10,
                patch: 0,
                pre_release: PreRelease::Beta(1)
            }
        );
        assert_eq!(
            "go1.11beta3".parse::<GoVersion>().unwrap(),
            GoVersion {
                major: 1,
                minor: 11,
                patch: 0,
                pre_release: PreRelease::Beta(3)
            }
        );
        assert_eq!(
            "go1.10rc1".parse::<GoVersion>().unwrap(),
            GoVersion {
                major: 1,
                minor: 10,
                patch: 0,
                pre_release: PreRelease::Rc(1)
            }
        );
        assert_eq!(
            "go1.11rc2".parse::<GoVersion>().unwrap(),
            GoVersion {
                major: 1,
                minor: 11,
                patch: 0,
                pre_release: PreRelease::Rc(2)
            }
        );
        assert_eq!(
            "go1.9.2rc2".parse::<GoVersion>().unwrap(), // Pre-release can attach to patch version implicitly
            GoVersion {
                major: 1,
                minor: 9,
                patch: 2,
                pre_release: PreRelease::Rc(2)
            }
        );
        assert_eq!(
            "go1.23rc1".parse::<GoVersion>().unwrap(),
            GoVersion {
                major: 1,
                minor: 23,
                patch: 0,
                pre_release: PreRelease::Rc(1)
            }
        );
    }

    #[test]
    fn test_parse_errors() {
        assert!("go".parse::<GoVersion>().is_err()); // No version part
        assert!("go1.".parse::<GoVersion>().is_err()); // Empty minor part
        assert!("go1.10.".parse::<GoVersion>().is_err()); // Empty patch part
        assert!("go1.10.rc1".parse::<GoVersion>().is_err()); // rc attached to non-existent element
        assert!("go1.beta1".parse::<GoVersion>().is_err()); // beta attached to non-existent element
        assert!("go1.10beta".parse::<GoVersion>().is_err()); // Missing beta number
        assert!("go1.10rc".parse::<GoVersion>().is_err()); // Missing rc number
        assert!("go1.10.5rc".parse::<GoVersion>().is_err()); // Missing rc number after patch
        assert!("go1.10.5beta".parse::<GoVersion>().is_err()); // Missing beta number after patch
        assert!("go1.10.beta1".parse::<GoVersion>().is_err()); // beta attached to non-existent element after patch
        assert!("go1.a".parse::<GoVersion>().is_err()); // Invalid minor version
        assert!("go1.10.c".parse::<GoVersion>().is_err()); // Invalid patch version
        assert!("go1.10betaX".parse::<GoVersion>().is_err()); // Invalid beta number
        assert!("go1.10rcY".parse::<GoVersion>().is_err()); // Invalid rc number
        assert!("go1.10.5.6".parse::<GoVersion>().is_err()); // Too many parts
        assert!("og1.10".parse::<GoVersion>().is_err()); // Invalid prefix
        assert!("g1.10".parse::<GoVersion>().is_err()); // Invalid prefix
        assert!("go1..2".parse::<GoVersion>().is_err()); // Empty minor part
        assert!("go.1.2".parse::<GoVersion>().is_err()); // Empty major part
    }

    #[test]
    fn test_version_ordering() {
        // Patch comparison
        assert!(
            "go1.10.1".parse::<GoVersion>().unwrap() < "go1.10.2".parse::<GoVersion>().unwrap()
        );
        assert!(
            "go1.10.8".parse::<GoVersion>().unwrap() > "go1.10.7".parse::<GoVersion>().unwrap()
        );
        assert!(
            "go1.10.1".parse::<GoVersion>().unwrap() == "go1.10.1".parse::<GoVersion>().unwrap()
        );

        // Minor comparison
        assert!(
            "go1.10.8".parse::<GoVersion>().unwrap() < "go1.11.0".parse::<GoVersion>().unwrap()
        );
        assert!(
            "go1.11.1".parse::<GoVersion>().unwrap() > "go1.10.8".parse::<GoVersion>().unwrap()
        );
        assert!("go1.2.2".parse::<GoVersion>().unwrap() < "go1.3".parse::<GoVersion>().unwrap());

        // Major comparison (though all examples are 1)
        assert!("go1.20.0".parse::<GoVersion>().unwrap() == "go1.20".parse::<GoVersion>().unwrap());

        // Pre-release comparison
        assert!(
            "go1.10beta1".parse::<GoVersion>().unwrap()
                < "go1.10beta2".parse::<GoVersion>().unwrap()
        );
        assert!(
            "go1.10rc1".parse::<GoVersion>().unwrap() < "go1.10rc2".parse::<GoVersion>().unwrap()
        );
        assert!(
            "go1.10beta2".parse::<GoVersion>().unwrap() < "go1.10rc1".parse::<GoVersion>().unwrap()
        );
        assert!(
            "go1.10rc2".parse::<GoVersion>().unwrap() < "go1.10.0".parse::<GoVersion>().unwrap()
        );
        assert!("go1.10rc2".parse::<GoVersion>().unwrap() < "go1.10".parse::<GoVersion>().unwrap()); // same as previous
        assert!(
            "go1.11beta3".parse::<GoVersion>().unwrap() < "go1.11rc1".parse::<GoVersion>().unwrap()
        );
        assert!("go1.11rc2".parse::<GoVersion>().unwrap() < "go1.11".parse::<GoVersion>().unwrap());
        assert!(
            "go1.9.2rc2".parse::<GoVersion>().unwrap() < "go1.9.3".parse::<GoVersion>().unwrap()
        );
        assert!(
            "go1.9.2".parse::<GoVersion>().unwrap() > "go1.9.2rc2".parse::<GoVersion>().unwrap()
        );

        // Comparison across different minor versions with pre-releases
        assert!(
            "go1.10rc1".parse::<GoVersion>().unwrap() < "go1.11beta1".parse::<GoVersion>().unwrap()
        );
        assert!(
            "go1.11rc2".parse::<GoVersion>().unwrap() < "go1.12beta1".parse::<GoVersion>().unwrap()
        );
        assert!(
            "go1.11.13".parse::<GoVersion>().unwrap() > "go1.11rc2".parse::<GoVersion>().unwrap()
        );
    }

    #[test]
    fn test_sorting_full_list() {
        let version_strings = vec![
            "go1",
            "go1.10",
            "go1.10.1",
            "go1.10.2",
            "go1.10.3",
            "go1.10.4",
            "go1.10.5",
            "go1.10.6",
            "go1.10.7",
            "go1.10.8",
            "go1.10beta1",
            "go1.10beta2",
            "go1.10rc1",
            "go1.10rc2",
            "go1.11",
            "go1.11.1",
            "go1.11.10",
            "go1.11.11",
            "go1.11.12",
            "go1.11.13",
            "go1.11.2",
            "go1.11.3",
            "go1.11.4",
            "go1.11.5",
            "go1.11.6",
            "go1.11.7",
            "go1.11.8",
            "go1.11.9",
            "go1.11beta1",
            "go1.11beta2",
            "go1.11beta3",
            "go1.11rc1",
            "go1.11rc2",
            "go1.12",
            "go1.12.1",
            "go1.12.10",
            "go1.12.11",
            "go1.12.12",
            "go1.12.13",
            "go1.12.14",
            "go1.12.15",
            "go1.12.16",
            "go1.12.17",
            "go1.12.2",
            "go1.12.3",
            "go1.12.4",
            "go1.12.5",
            "go1.12.6",
            "go1.12.7",
            "go1.12.8",
            "go1.12.9",
            "go1.12beta1",
            "go1.12beta2",
            "go1.12rc1",
            "go1.13",
            "go1.13.1",
            "go1.13.10",
            "go1.13.11",
            "go1.13.12",
            "go1.13.13",
            "go1.13.14",
            "go1.13.15",
            "go1.13.2",
            "go1.13.3",
            "go1.13.4",
            "go1.13.5",
            "go1.13.6",
            "go1.13.7",
            "go1.13.8",
            "go1.13.9",
            "go1.13beta1",
            "go1.13rc1",
            "go1.13rc2",
            "go1.14",
            "go1.14.1",
            "go1.14.10",
            "go1.14.11",
            "go1.14.12",
            "go1.14.13",
            "go1.14.14",
            "go1.14.15",
            "go1.14.2",
            "go1.14.3",
            "go1.14.4",
            "go1.14.5",
            "go1.14.6",
            "go1.14.7",
            "go1.14.8",
            "go1.14.9",
            "go1.14beta1",
            "go1.14rc1",
            "go1.15",
            "go1.15.1",
            "go1.15.10",
            "go1.15.11",
            "go1.15.12",
            "go1.15.13",
            "go1.15.14",
            "go1.15.15",
            "go1.15.2",
            "go1.15.3",
            "go1.15.4",
            "go1.15.5",
            "go1.15.6",
            "go1.15.7",
            "go1.15.8",
            "go1.15.9",
            "go1.15beta1",
            "go1.15rc1",
            "go1.15rc2",
            "go1.16",
            "go1.16.1",
            "go1.16.10",
            "go1.16.11",
            "go1.16.12",
            "go1.16.13",
            "go1.16.14",
            "go1.16.15",
            "go1.16.2",
            "go1.16.3",
            "go1.16.4",
            "go1.16.5",
            "go1.16.6",
            "go1.16.7",
            "go1.16.8",
            "go1.16.9",
            "go1.16beta1",
            "go1.16rc1",
            "go1.17",
            "go1.17.1",
            "go1.17.10",
            "go1.17.11",
            "go1.17.12",
            "go1.17.13",
            "go1.17.2",
            "go1.17.3",
            "go1.17.4",
            "go1.17.5",
            "go1.17.6",
            "go1.17.7",
            "go1.17.8",
            "go1.17.9",
            "go1.17beta1",
            "go1.17rc1",
            "go1.17rc2",
            "go1.18",
            "go1.18.1",
            "go1.18.10",
            "go1.18.2",
            "go1.18.3",
            "go1.18.4",
            "go1.18.5",
            "go1.18.6",
            "go1.18.7",
            "go1.18.8",
            "go1.18.9",
            "go1.18beta1",
            "go1.18beta2",
            "go1.18rc1",
            "go1.19",
            "go1.19.1",
            "go1.19.10",
            "go1.19.11",
            "go1.19.12",
            "go1.19.13",
            "go1.19.2",
            "go1.19.3",
            "go1.19.4",
            "go1.19.5",
            "go1.19.6",
            "go1.19.7",
            "go1.19.8",
            "go1.19.9",
            "go1.19beta1",
            "go1.19rc1",
            "go1.19rc2",
            "go1.2.2",
            "go1.20",
            "go1.20.1",
            "go1.20.10",
            "go1.20.11",
            "go1.20.12",
            "go1.20.13",
            "go1.20.14",
            "go1.20.2",
            "go1.20.3",
            "go1.20.4",
            "go1.20.5",
            "go1.20.6",
            "go1.20.7",
            "go1.20.8",
            "go1.20.9",
            "go1.20rc1",
            "go1.20rc2",
            "go1.20rc3",
            "go1.21.0",
            "go1.21.1",
            "go1.21.10",
            "go1.21.11",
            "go1.21.12",
            "go1.21.13",
            "go1.21.2",
            "go1.21.3",
            "go1.21.4",
            "go1.21.5",
            "go1.21.6",
            "go1.21.7",
            "go1.21.8",
            "go1.21.9",
            "go1.21rc2",
            "go1.21rc3",
            "go1.21rc4",
            "go1.22.0",
            "go1.22.1",
            "go1.22.10",
            "go1.22.11",
            "go1.22.12",
            "go1.22.2",
            "go1.22.3",
            "go1.22.4",
            "go1.22.5",
            "go1.22.6",
            "go1.22.7",
            "go1.22.8",
            "go1.22.9",
            "go1.22rc1",
            "go1.22rc2",
            "go1.23.0",
            "go1.23.1",
            "go1.23.2",
            "go1.23.3",
            "go1.23.4",
            "go1.23.5",
            "go1.23.6",
            "go1.23.7",
            "go1.23.8",
            "go1.23rc1",
            "go1.23rc2",
            "go1.24.0",
            "go1.24.1",
            "go1.24.2",
            "go1.24rc1",
            "go1.24rc2",
            "go1.24rc3",
            "go1.3",
            "go1.3.1",
            "go1.3.2",
            "go1.3.3",
            "go1.3rc1",
            "go1.3rc2",
            "go1.4",
            "go1.4.1",
            "go1.4.2",
            "go1.4.3",
            "go1.4beta1",
            "go1.4rc1",
            "go1.4rc2",
            "go1.5",
            "go1.5.1",
            "go1.5.2",
            "go1.5.3",
            "go1.5.4",
            "go1.5beta1",
            "go1.5beta2",
            "go1.5beta3",
            "go1.5rc1",
            "go1.6",
            "go1.6.1",
            "go1.6.2",
            "go1.6.3",
            "go1.6.4",
            "go1.6beta1",
            "go1.6beta2",
            "go1.6rc1",
            "go1.6rc2",
            "go1.7",
            "go1.7.1",
            "go1.7.3",
            "go1.7.4",
            "go1.7.5",
            "go1.7.6",
            "go1.7beta1",
            "go1.7beta2",
            "go1.7rc1",
            "go1.7rc2",
            "go1.7rc3",
            "go1.7rc4",
            "go1.7rc5",
            "go1.7rc6",
            "go1.8",
            "go1.8.1",
            "go1.8.2",
            "go1.8.3",
            "go1.8.4",
            "go1.8.5",
            "go1.8.6",
            "go1.8.7",
            "go1.8beta1",
            "go1.8beta2",
            "go1.8rc1",
            "go1.8rc2",
            "go1.8rc3",
            "go1.9",
            "go1.9.1",
            "go1.9.2",
            "go1.9.2rc2",
            "go1.9.3",
            "go1.9.4",
            "go1.9.5",
            "go1.9.6",
            "go1.9.7",
            "go1.9beta1",
            "go1.9beta2",
            "go1.9rc1",
            "go1.9rc2",
        ];

        let mut parsed_versions: Vec<GoVersion> = version_strings
            .iter()
            .map(|s| s.parse::<GoVersion>())
            .collect::<Result<Vec<_>, _>>()
            .expect("Failed to parse one of the test strings");

        parsed_versions.sort();

        // Spot check a few critical ordering points after sort
        let v1_0_0 = GoVersion {
            major: 1,
            minor: 0,
            patch: 0,
            pre_release: PreRelease::None,
        }; // "go1"
        let v1_2_2 = GoVersion {
            major: 1,
            minor: 2,
            patch: 2,
            pre_release: PreRelease::None,
        };
        let v1_3_0 = GoVersion {
            major: 1,
            minor: 3,
            patch: 0,
            pre_release: PreRelease::None,
        };
        let v1_9_2 = GoVersion {
            major: 1,
            minor: 9,
            patch: 2,
            pre_release: PreRelease::None,
        };
        let v1_9_2rc2 = GoVersion {
            major: 1,
            minor: 9,
            patch: 2,
            pre_release: PreRelease::Rc(2),
        };
        let v1_9_3 = GoVersion {
            major: 1,
            minor: 9,
            patch: 3,
            pre_release: PreRelease::None,
        };
        let v1_10_beta1 = GoVersion {
            major: 1,
            minor: 10,
            patch: 0,
            pre_release: PreRelease::Beta(1),
        };
        let v1_10_beta2 = GoVersion {
            major: 1,
            minor: 10,
            patch: 0,
            pre_release: PreRelease::Beta(2),
        };
        let v1_10_rc1 = GoVersion {
            major: 1,
            minor: 10,
            patch: 0,
            pre_release: PreRelease::Rc(1),
        };
        let v1_10_rc2 = GoVersion {
            major: 1,
            minor: 10,
            patch: 0,
            pre_release: PreRelease::Rc(2),
        };
        let v1_10_0 = GoVersion {
            major: 1,
            minor: 10,
            patch: 0,
            pre_release: PreRelease::None,
        };
        let v1_10_1 = GoVersion {
            major: 1,
            minor: 10,
            patch: 1,
            pre_release: PreRelease::None,
        };
        let v1_24_2 = GoVersion {
            major: 1,
            minor: 24,
            patch: 2,
            pre_release: PreRelease::None,
        };
        let v1_24_rc3 = GoVersion {
            major: 1,
            minor: 24,
            patch: 0,
            pre_release: PreRelease::Rc(3),
        };

        // Find specific versions and assert their relative positions
        assert_eq!(parsed_versions[0], v1_0_0); // go1 should be first
        assert!(parsed_versions.contains(&v1_2_2));
        assert!(parsed_versions.contains(&v1_3_0));
        assert!(parsed_versions.contains(&v1_9_2rc2));
        assert!(parsed_versions.contains(&v1_9_2));
        assert!(parsed_versions.contains(&v1_9_3));
        assert!(parsed_versions.contains(&v1_10_beta1));
        assert!(parsed_versions.contains(&v1_10_beta2));
        assert!(parsed_versions.contains(&v1_10_rc1));
        assert!(parsed_versions.contains(&v1_10_rc2));
        assert!(parsed_versions.contains(&v1_10_0));
        assert!(parsed_versions.contains(&v1_10_1));
        assert!(parsed_versions.contains(&v1_24_rc3));
        assert!(parsed_versions.contains(&v1_24_2));
        assert_eq!(parsed_versions.last().unwrap(), &v1_24_2); // go1.24.2 should be last (as of list provided)

        // Verify the entire list is sorted by checking adjacent elements
        for i in 0..(parsed_versions.len() - 1) {
            assert!(
                parsed_versions[i] <= parsed_versions[i + 1],
                "Sort order violation: {:?} is not <= {:?}",
                parsed_versions[i],
                parsed_versions[i + 1]
            );
        }
    }
}
