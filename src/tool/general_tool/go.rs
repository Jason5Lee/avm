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
    corresponding_dto_cpu_os: Vec<(&'static str, &'static str)>,
}

const BASE_URL: &str = "https://golang.org/dl/";

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
        let (cpu, os) = self.get_dto_cpu_os(&platform);
        let version_filter = GoVersionFilter::try_from(version_filter)?;

        let mut releases = self
            .fetch_go_releases(&self.client)
            .await?
            .into_iter()
            .filter_map(|r| {
                if !r.files.iter().any(|f| f.matches(cpu, os)) {
                    return None;
                }
                let (raw_version, version) = parse_go_version(&r.version)
                    .map_err(|e| log::error!("Failed to parse Go version: {}", e))
                    .ok()?;
                if !version_filter.matches(raw_version, &version) {
                    None
                } else {
                    Some((version, SmolStr::from(raw_version)))
                }
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
                    is_lts: release.0.is_lts(),
                });
            }
        }

        Ok(versions)
    }

    async fn get_down_info(
        &self,
        platform: Option<SmolStr>,
        _flavor: Option<SmolStr>,
        version_filter: VersionFilter,
    ) -> anyhow::Result<ToolDownInfo> {
        let platform = platform.ok_or_else(|| anyhow::anyhow!("Platform is required"))?;
        let (cpu, os) = self.get_dto_cpu_os(&platform);

        let version_filter = GoVersionFilter::try_from(version_filter)?;

        let release = self
            .fetch_go_releases(&self.client)
            .await?
            .into_iter()
            .filter_map(|r| {
                let item = r.files.into_iter().find(|f| f.matches(cpu, os))?;
                let (raw_version, version) = parse_go_version(&r.version)
                    .map_err(|e| log::error!("Failed to parse Go version: {}", e))
                    .ok()?;
                if !version_filter.matches(raw_version, &version) {
                    None
                } else {
                    Some((version, SmolStr::from(raw_version), item))
                }
            })
            .max_by(|a, b| a.0.cmp(&b.0));
        if let Some((_, raw_version, item)) = release {
            Ok(ToolDownInfo {
                version: raw_version,
                url: smol_str::format_smolstr!("{}/{}", BASE_URL, item.filename),
                hash: crate::FileHash {
                    sha256: Some(item.sha256.into()),
                    ..Default::default()
                },
            })
        } else {
            Err(anyhow::anyhow!("No download URL found."))
        }
    }

    fn exe_path(&self, tag_dir: PathBuf) -> anyhow::Result<PathBuf> {
        let mut p = tag_dir;
        p.push("bin");
        #[cfg(windows)]
        p.push("go.exe");
        #[cfg(not(windows))]
        p.push("go");
        Ok(p)
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

    async fn fetch_go_releases(&self, client: &HttpClient) -> reqwest::Result<Vec<ReleaseDto>> {
        client
            .get(BASE_URL)
            .query(&[("mode", "json"), ("include", "all")])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
    }
}

#[derive(Debug, Deserialize)]
struct ReleaseDto {
    version: SmolStr,
    files: Vec<ReleaseFileDto>,
}

#[derive(Debug, Deserialize)]
struct ReleaseFileDto {
    filename: String,
    os: SmolStr,
    arch: SmolStr,
    sha256: String,
    kind: String,
}

impl ReleaseFileDto {
    fn matches(&self, cpu: &str, os: &str) -> bool {
        self.os == os && self.arch == cpu && self.kind == "archive"
    }
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

impl GoVersion {
    fn is_lts(&self) -> bool {
        self.pre_release == PreRelease::None
    }
}

struct GoVersionFilter {
    lts_only: bool,
    major_version: Option<u32>,
    exact_version: Option<SmolStr>,
}

impl GoVersionFilter {
    fn matches(&self, raw_version: &str, version: &GoVersion) -> bool {
        let Self {
            lts_only,
            major_version,
            exact_version,
        } = self;
        if *lts_only && !version.is_lts() {
            return false;
        }
        if let Some(major_version) = major_version {
            if version.major != *major_version {
                return false;
            }
        }
        if let Some(exact_version) = exact_version {
            if exact_version != raw_version {
                return false;
            }
        }
        true
    }
}

impl TryFrom<VersionFilter> for GoVersionFilter {
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

/// Parses a Go version string and returns the trimmed version string and parsed GoVersion.
pub fn parse_go_version(s: &str) -> anyhow::Result<(&str, GoVersion)> {
    // 1. Check and remove the "go" prefix
    let raw_version = s
        .strip_prefix("go")
        .ok_or_else(|| anyhow::anyhow!("Input string '{}' does not start with 'go'", s))?;

    if raw_version.is_empty() {
        return Err(anyhow::anyhow!(
            "Input string '{}' has no version part after 'go'",
            s
        ));
    }

    // 2. Identify and separate pre-release identifier (beta/rc)
    let (main_part, pre_release) = {
        if let Some(index) = raw_version.find("beta") {
            let num_str = &raw_version[index + 4..];
            if num_str.is_empty() {
                return Err(anyhow::anyhow!("Missing number after 'beta' in '{}'", s));
            }
            let num = num_str.parse::<u32>().map_err(|e| {
                anyhow::anyhow!("Invalid beta number '{}' in '{}': {}", num_str, s, e)
            })?;
            (&raw_version[..index], PreRelease::Beta(num))
        } else if let Some(index) = raw_version.find("rc") {
            let num_str = &raw_version[index + 2..];
            if num_str.is_empty() {
                return Err(anyhow::anyhow!("Missing number after 'rc' in '{}'", s));
            }
            let num = num_str.parse::<u32>().map_err(|e| {
                anyhow::anyhow!("Invalid rc number '{}' in '{}': {}", num_str, s, e)
            })?;
            (&raw_version[..index], PreRelease::Rc(num))
        } else {
            (raw_version, PreRelease::None)
        }
    };

    // 3. Parse major, minor, patch numbers
    let mut parts = main_part.split('.');

    let major_str = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("Missing major version number in '{}'", s))?;
    if major_str.is_empty() && main_part.starts_with('.') {
        return Err(anyhow::anyhow!("Major version part is empty in '{}'", s));
    }
    let major = major_str
        .parse::<u32>()
        .map_err(|e| anyhow::anyhow!("Invalid major version '{}' in '{}': {}", major_str, s, e))?;

    // Default minor and patch to 0 if not present
    let mut minor = 0;
    let mut patch = 0;

    if let Some(minor_str) = parts.next() {
        if minor_str.is_empty() {
            return Err(anyhow::anyhow!("Minor version part is empty in '{}'", s));
        }
        minor = minor_str.parse::<u32>().map_err(|e| {
            anyhow::anyhow!("Invalid minor version '{}' in '{}': {}", minor_str, s, e)
        })?;

        if let Some(patch_str) = parts.next() {
            if patch_str.is_empty() {
                return Err(anyhow::anyhow!("Patch version part is empty in '{}'", s));
            }
            patch = patch_str.parse::<u32>().map_err(|e| {
                anyhow::anyhow!("Invalid patch version '{}' in '{}': {}", patch_str, s, e)
            })?;
        }
    }

    // Check if there are too many parts (e.g., "go1.2.3.4")
    if parts.next().is_some() {
        return Err(anyhow::anyhow!(
            "Too many version parts (expected max 3) in '{}'",
            s
        ));
    }

    Ok((
        raw_version,
        GoVersion {
            major,
            minor,
            patch,
            pre_release,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[rustfmt::skip]
    #[test]
    fn test_correct_versions() {
        assert_eq!(parse_go_version("go1.24.2").unwrap(), ("1.24.2", GoVersion { major: 1, minor: 24, patch: 2, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.24.1").unwrap(), ("1.24.1", GoVersion { major: 1, minor: 24, patch: 1, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.24.0").unwrap(), ("1.24.0", GoVersion { major: 1, minor: 24, patch: 0, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.24rc3").unwrap(), ("1.24rc3", GoVersion { major: 1, minor: 24, patch: 0, pre_release: PreRelease::Rc(3) }));
        assert_eq!(parse_go_version("go1.24rc2").unwrap(), ("1.24rc2", GoVersion { major: 1, minor: 24, patch: 0, pre_release: PreRelease::Rc(2) }));
        assert_eq!(parse_go_version("go1.24rc1").unwrap(), ("1.24rc1", GoVersion { major: 1, minor: 24, patch: 0, pre_release: PreRelease::Rc(1) }));
        assert_eq!(parse_go_version("go1.23.8").unwrap(), ("1.23.8", GoVersion { major: 1, minor: 23, patch: 8, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.23.7").unwrap(), ("1.23.7", GoVersion { major: 1, minor: 23, patch: 7, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.23.6").unwrap(), ("1.23.6", GoVersion { major: 1, minor: 23, patch: 6, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.23.5").unwrap(), ("1.23.5", GoVersion { major: 1, minor: 23, patch: 5, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.23.4").unwrap(), ("1.23.4", GoVersion { major: 1, minor: 23, patch: 4, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.23.3").unwrap(), ("1.23.3", GoVersion { major: 1, minor: 23, patch: 3, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.23.2").unwrap(), ("1.23.2", GoVersion { major: 1, minor: 23, patch: 2, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.23.1").unwrap(), ("1.23.1", GoVersion { major: 1, minor: 23, patch: 1, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.23.0").unwrap(), ("1.23.0", GoVersion { major: 1, minor: 23, patch: 0, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.23rc2").unwrap(), ("1.23rc2", GoVersion { major: 1, minor: 23, patch: 0, pre_release: PreRelease::Rc(2) }));
        assert_eq!(parse_go_version("go1.23rc1").unwrap(), ("1.23rc1", GoVersion { major: 1, minor: 23, patch: 0, pre_release: PreRelease::Rc(1) }));
        assert_eq!(parse_go_version("go1.22.12").unwrap(), ("1.22.12", GoVersion { major: 1, minor: 22, patch: 12, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.22.11").unwrap(), ("1.22.11", GoVersion { major: 1, minor: 22, patch: 11, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.22.10").unwrap(), ("1.22.10", GoVersion { major: 1, minor: 22, patch: 10, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.22.9").unwrap(), ("1.22.9", GoVersion { major: 1, minor: 22, patch: 9, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.22.8").unwrap(), ("1.22.8", GoVersion { major: 1, minor: 22, patch: 8, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.22.7").unwrap(), ("1.22.7", GoVersion { major: 1, minor: 22, patch: 7, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.22.6").unwrap(), ("1.22.6", GoVersion { major: 1, minor: 22, patch: 6, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.22.5").unwrap(), ("1.22.5", GoVersion { major: 1, minor: 22, patch: 5, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.22.4").unwrap(), ("1.22.4", GoVersion { major: 1, minor: 22, patch: 4, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.22.3").unwrap(), ("1.22.3", GoVersion { major: 1, minor: 22, patch: 3, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.22.2").unwrap(), ("1.22.2", GoVersion { major: 1, minor: 22, patch: 2, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.22.1").unwrap(), ("1.22.1", GoVersion { major: 1, minor: 22, patch: 1, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.22.0").unwrap(), ("1.22.0", GoVersion { major: 1, minor: 22, patch: 0, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.22rc2").unwrap(), ("1.22rc2", GoVersion { major: 1, minor: 22, patch: 0, pre_release: PreRelease::Rc(2) }));
        assert_eq!(parse_go_version("go1.22rc1").unwrap(), ("1.22rc1", GoVersion { major: 1, minor: 22, patch: 0, pre_release: PreRelease::Rc(1) }));
        assert_eq!(parse_go_version("go1.21.13").unwrap(), ("1.21.13", GoVersion { major: 1, minor: 21, patch: 13, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.21.12").unwrap(), ("1.21.12", GoVersion { major: 1, minor: 21, patch: 12, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.21.11").unwrap(), ("1.21.11", GoVersion { major: 1, minor: 21, patch: 11, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.21.10").unwrap(), ("1.21.10", GoVersion { major: 1, minor: 21, patch: 10, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.21.9").unwrap(), ("1.21.9", GoVersion { major: 1, minor: 21, patch: 9, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.21.8").unwrap(), ("1.21.8", GoVersion { major: 1, minor: 21, patch: 8, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.21.7").unwrap(), ("1.21.7", GoVersion { major: 1, minor: 21, patch: 7, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.21.6").unwrap(), ("1.21.6", GoVersion { major: 1, minor: 21, patch: 6, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.21.5").unwrap(), ("1.21.5", GoVersion { major: 1, minor: 21, patch: 5, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.21.4").unwrap(), ("1.21.4", GoVersion { major: 1, minor: 21, patch: 4, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.21.3").unwrap(), ("1.21.3", GoVersion { major: 1, minor: 21, patch: 3, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.21.2").unwrap(), ("1.21.2", GoVersion { major: 1, minor: 21, patch: 2, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.21.1").unwrap(), ("1.21.1", GoVersion { major: 1, minor: 21, patch: 1, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.21.0").unwrap(), ("1.21.0", GoVersion { major: 1, minor: 21, patch: 0, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.21rc4").unwrap(), ("1.21rc4", GoVersion { major: 1, minor: 21, patch: 0, pre_release: PreRelease::Rc(4) }));
        assert_eq!(parse_go_version("go1.21rc3").unwrap(), ("1.21rc3", GoVersion { major: 1, minor: 21, patch: 0, pre_release: PreRelease::Rc(3) }));
        assert_eq!(parse_go_version("go1.21rc2").unwrap(), ("1.21rc2", GoVersion { major: 1, minor: 21, patch: 0, pre_release: PreRelease::Rc(2) }));
        assert_eq!(parse_go_version("go1.20.14").unwrap(), ("1.20.14", GoVersion { major: 1, minor: 20, patch: 14, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.20.13").unwrap(), ("1.20.13", GoVersion { major: 1, minor: 20, patch: 13, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.20.12").unwrap(), ("1.20.12", GoVersion { major: 1, minor: 20, patch: 12, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.20.11").unwrap(), ("1.20.11", GoVersion { major: 1, minor: 20, patch: 11, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.20.10").unwrap(), ("1.20.10", GoVersion { major: 1, minor: 20, patch: 10, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.20.9").unwrap(), ("1.20.9", GoVersion { major: 1, minor: 20, patch: 9, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.20.8").unwrap(), ("1.20.8", GoVersion { major: 1, minor: 20, patch: 8, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.20.7").unwrap(), ("1.20.7", GoVersion { major: 1, minor: 20, patch: 7, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.20.6").unwrap(), ("1.20.6", GoVersion { major: 1, minor: 20, patch: 6, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.20.5").unwrap(), ("1.20.5", GoVersion { major: 1, minor: 20, patch: 5, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.20.4").unwrap(), ("1.20.4", GoVersion { major: 1, minor: 20, patch: 4, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.20.3").unwrap(), ("1.20.3", GoVersion { major: 1, minor: 20, patch: 3, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.20.2").unwrap(), ("1.20.2", GoVersion { major: 1, minor: 20, patch: 2, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.20.1").unwrap(), ("1.20.1", GoVersion { major: 1, minor: 20, patch: 1, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.20").unwrap(), ("1.20", GoVersion { major: 1, minor: 20, patch: 0, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.20rc3").unwrap(), ("1.20rc3", GoVersion { major: 1, minor: 20, patch: 0, pre_release: PreRelease::Rc(3) }));
        assert_eq!(parse_go_version("go1.20rc2").unwrap(), ("1.20rc2", GoVersion { major: 1, minor: 20, patch: 0, pre_release: PreRelease::Rc(2) }));
        assert_eq!(parse_go_version("go1.20rc1").unwrap(), ("1.20rc1", GoVersion { major: 1, minor: 20, patch: 0, pre_release: PreRelease::Rc(1) }));
        assert_eq!(parse_go_version("go1.19.13").unwrap(), ("1.19.13", GoVersion { major: 1, minor: 19, patch: 13, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.19.12").unwrap(), ("1.19.12", GoVersion { major: 1, minor: 19, patch: 12, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.19.11").unwrap(), ("1.19.11", GoVersion { major: 1, minor: 19, patch: 11, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.19.10").unwrap(), ("1.19.10", GoVersion { major: 1, minor: 19, patch: 10, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.19.9").unwrap(), ("1.19.9", GoVersion { major: 1, minor: 19, patch: 9, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.19.8").unwrap(), ("1.19.8", GoVersion { major: 1, minor: 19, patch: 8, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.19.7").unwrap(), ("1.19.7", GoVersion { major: 1, minor: 19, patch: 7, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.19.6").unwrap(), ("1.19.6", GoVersion { major: 1, minor: 19, patch: 6, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.19.5").unwrap(), ("1.19.5", GoVersion { major: 1, minor: 19, patch: 5, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.19.4").unwrap(), ("1.19.4", GoVersion { major: 1, minor: 19, patch: 4, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.19.3").unwrap(), ("1.19.3", GoVersion { major: 1, minor: 19, patch: 3, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.19.2").unwrap(), ("1.19.2", GoVersion { major: 1, minor: 19, patch: 2, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.19.1").unwrap(), ("1.19.1", GoVersion { major: 1, minor: 19, patch: 1, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.19").unwrap(), ("1.19", GoVersion { major: 1, minor: 19, patch: 0, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.19rc2").unwrap(), ("1.19rc2", GoVersion { major: 1, minor: 19, patch: 0, pre_release: PreRelease::Rc(2) }));
        assert_eq!(parse_go_version("go1.19rc1").unwrap(), ("1.19rc1", GoVersion { major: 1, minor: 19, patch: 0, pre_release: PreRelease::Rc(1) }));
        assert_eq!(parse_go_version("go1.19beta1").unwrap(), ("1.19beta1", GoVersion { major: 1, minor: 19, patch: 0, pre_release: PreRelease::Beta(1) }));
        assert_eq!(parse_go_version("go1.18.10").unwrap(), ("1.18.10", GoVersion { major: 1, minor: 18, patch: 10, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.18.9").unwrap(), ("1.18.9", GoVersion { major: 1, minor: 18, patch: 9, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.18.8").unwrap(), ("1.18.8", GoVersion { major: 1, minor: 18, patch: 8, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.18.7").unwrap(), ("1.18.7", GoVersion { major: 1, minor: 18, patch: 7, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.18.6").unwrap(), ("1.18.6", GoVersion { major: 1, minor: 18, patch: 6, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.18.5").unwrap(), ("1.18.5", GoVersion { major: 1, minor: 18, patch: 5, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.18.4").unwrap(), ("1.18.4", GoVersion { major: 1, minor: 18, patch: 4, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.18.3").unwrap(), ("1.18.3", GoVersion { major: 1, minor: 18, patch: 3, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.18.2").unwrap(), ("1.18.2", GoVersion { major: 1, minor: 18, patch: 2, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.18.1").unwrap(), ("1.18.1", GoVersion { major: 1, minor: 18, patch: 1, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.18").unwrap(), ("1.18", GoVersion { major: 1, minor: 18, patch: 0, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.18rc1").unwrap(), ("1.18rc1", GoVersion { major: 1, minor: 18, patch: 0, pre_release: PreRelease::Rc(1) }));
        assert_eq!(parse_go_version("go1.18beta2").unwrap(), ("1.18beta2", GoVersion { major: 1, minor: 18, patch: 0, pre_release: PreRelease::Beta(2) }));
        assert_eq!(parse_go_version("go1.18beta1").unwrap(), ("1.18beta1", GoVersion { major: 1, minor: 18, patch: 0, pre_release: PreRelease::Beta(1) }));
        assert_eq!(parse_go_version("go1.17.13").unwrap(), ("1.17.13", GoVersion { major: 1, minor: 17, patch: 13, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.17.12").unwrap(), ("1.17.12", GoVersion { major: 1, minor: 17, patch: 12, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.17.11").unwrap(), ("1.17.11", GoVersion { major: 1, minor: 17, patch: 11, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.17.10").unwrap(), ("1.17.10", GoVersion { major: 1, minor: 17, patch: 10, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.17.9").unwrap(), ("1.17.9", GoVersion { major: 1, minor: 17, patch: 9, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.17.8").unwrap(), ("1.17.8", GoVersion { major: 1, minor: 17, patch: 8, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.17.7").unwrap(), ("1.17.7", GoVersion { major: 1, minor: 17, patch: 7, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.17.6").unwrap(), ("1.17.6", GoVersion { major: 1, minor: 17, patch: 6, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.17.5").unwrap(), ("1.17.5", GoVersion { major: 1, minor: 17, patch: 5, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.17.4").unwrap(), ("1.17.4", GoVersion { major: 1, minor: 17, patch: 4, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.17.3").unwrap(), ("1.17.3", GoVersion { major: 1, minor: 17, patch: 3, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.17.2").unwrap(), ("1.17.2", GoVersion { major: 1, minor: 17, patch: 2, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.17.1").unwrap(), ("1.17.1", GoVersion { major: 1, minor: 17, patch: 1, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.17").unwrap(), ("1.17", GoVersion { major: 1, minor: 17, patch: 0, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.17rc2").unwrap(), ("1.17rc2", GoVersion { major: 1, minor: 17, patch: 0, pre_release: PreRelease::Rc(2) }));
        assert_eq!(parse_go_version("go1.17rc1").unwrap(), ("1.17rc1", GoVersion { major: 1, minor: 17, patch: 0, pre_release: PreRelease::Rc(1) }));
        assert_eq!(parse_go_version("go1.17beta1").unwrap(), ("1.17beta1", GoVersion { major: 1, minor: 17, patch: 0, pre_release: PreRelease::Beta(1) }));
        assert_eq!(parse_go_version("go1.16.15").unwrap(), ("1.16.15", GoVersion { major: 1, minor: 16, patch: 15, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.16.14").unwrap(), ("1.16.14", GoVersion { major: 1, minor: 16, patch: 14, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.16.13").unwrap(), ("1.16.13", GoVersion { major: 1, minor: 16, patch: 13, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.16.12").unwrap(), ("1.16.12", GoVersion { major: 1, minor: 16, patch: 12, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.16.11").unwrap(), ("1.16.11", GoVersion { major: 1, minor: 16, patch: 11, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.16.10").unwrap(), ("1.16.10", GoVersion { major: 1, minor: 16, patch: 10, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.16.9").unwrap(), ("1.16.9", GoVersion { major: 1, minor: 16, patch: 9, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.16.8").unwrap(), ("1.16.8", GoVersion { major: 1, minor: 16, patch: 8, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.16.7").unwrap(), ("1.16.7", GoVersion { major: 1, minor: 16, patch: 7, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.16.6").unwrap(), ("1.16.6", GoVersion { major: 1, minor: 16, patch: 6, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.16.5").unwrap(), ("1.16.5", GoVersion { major: 1, minor: 16, patch: 5, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.16.4").unwrap(), ("1.16.4", GoVersion { major: 1, minor: 16, patch: 4, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.16.3").unwrap(), ("1.16.3", GoVersion { major: 1, minor: 16, patch: 3, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.16.2").unwrap(), ("1.16.2", GoVersion { major: 1, minor: 16, patch: 2, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.16.1").unwrap(), ("1.16.1", GoVersion { major: 1, minor: 16, patch: 1, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.16").unwrap(), ("1.16", GoVersion { major: 1, minor: 16, patch: 0, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.16rc1").unwrap(), ("1.16rc1", GoVersion { major: 1, minor: 16, patch: 0, pre_release: PreRelease::Rc(1) }));
        assert_eq!(parse_go_version("go1.16beta1").unwrap(), ("1.16beta1", GoVersion { major: 1, minor: 16, patch: 0, pre_release: PreRelease::Beta(1) }));
        assert_eq!(parse_go_version("go1.15.15").unwrap(), ("1.15.15", GoVersion { major: 1, minor: 15, patch: 15, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.15.14").unwrap(), ("1.15.14", GoVersion { major: 1, minor: 15, patch: 14, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.15.13").unwrap(), ("1.15.13", GoVersion { major: 1, minor: 15, patch: 13, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.15.12").unwrap(), ("1.15.12", GoVersion { major: 1, minor: 15, patch: 12, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.15.11").unwrap(), ("1.15.11", GoVersion { major: 1, minor: 15, patch: 11, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.15.10").unwrap(), ("1.15.10", GoVersion { major: 1, minor: 15, patch: 10, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.15.9").unwrap(), ("1.15.9", GoVersion { major: 1, minor: 15, patch: 9, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.15.8").unwrap(), ("1.15.8", GoVersion { major: 1, minor: 15, patch: 8, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.15.7").unwrap(), ("1.15.7", GoVersion { major: 1, minor: 15, patch: 7, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.15.6").unwrap(), ("1.15.6", GoVersion { major: 1, minor: 15, patch: 6, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.15.5").unwrap(), ("1.15.5", GoVersion { major: 1, minor: 15, patch: 5, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.15.4").unwrap(), ("1.15.4", GoVersion { major: 1, minor: 15, patch: 4, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.15.3").unwrap(), ("1.15.3", GoVersion { major: 1, minor: 15, patch: 3, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.15.2").unwrap(), ("1.15.2", GoVersion { major: 1, minor: 15, patch: 2, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.15.1").unwrap(), ("1.15.1", GoVersion { major: 1, minor: 15, patch: 1, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.15").unwrap(), ("1.15", GoVersion { major: 1, minor: 15, patch: 0, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.15rc2").unwrap(), ("1.15rc2", GoVersion { major: 1, minor: 15, patch: 0, pre_release: PreRelease::Rc(2) }));
        assert_eq!(parse_go_version("go1.15rc1").unwrap(), ("1.15rc1", GoVersion { major: 1, minor: 15, patch: 0, pre_release: PreRelease::Rc(1) }));
        assert_eq!(parse_go_version("go1.15beta1").unwrap(), ("1.15beta1", GoVersion { major: 1, minor: 15, patch: 0, pre_release: PreRelease::Beta(1) }));
        assert_eq!(parse_go_version("go1.14.15").unwrap(), ("1.14.15", GoVersion { major: 1, minor: 14, patch: 15, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.14.14").unwrap(), ("1.14.14", GoVersion { major: 1, minor: 14, patch: 14, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.14.13").unwrap(), ("1.14.13", GoVersion { major: 1, minor: 14, patch: 13, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.14.12").unwrap(), ("1.14.12", GoVersion { major: 1, minor: 14, patch: 12, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.14.11").unwrap(), ("1.14.11", GoVersion { major: 1, minor: 14, patch: 11, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.14.10").unwrap(), ("1.14.10", GoVersion { major: 1, minor: 14, patch: 10, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.14.9").unwrap(), ("1.14.9", GoVersion { major: 1, minor: 14, patch: 9, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.14.8").unwrap(), ("1.14.8", GoVersion { major: 1, minor: 14, patch: 8, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.14.7").unwrap(), ("1.14.7", GoVersion { major: 1, minor: 14, patch: 7, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.14.6").unwrap(), ("1.14.6", GoVersion { major: 1, minor: 14, patch: 6, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.14.5").unwrap(), ("1.14.5", GoVersion { major: 1, minor: 14, patch: 5, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.14.4").unwrap(), ("1.14.4", GoVersion { major: 1, minor: 14, patch: 4, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.14.3").unwrap(), ("1.14.3", GoVersion { major: 1, minor: 14, patch: 3, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.14.2").unwrap(), ("1.14.2", GoVersion { major: 1, minor: 14, patch: 2, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.14.1").unwrap(), ("1.14.1", GoVersion { major: 1, minor: 14, patch: 1, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.14").unwrap(), ("1.14", GoVersion { major: 1, minor: 14, patch: 0, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.14rc1").unwrap(), ("1.14rc1", GoVersion { major: 1, minor: 14, patch: 0, pre_release: PreRelease::Rc(1) }));
        assert_eq!(parse_go_version("go1.14beta1").unwrap(), ("1.14beta1", GoVersion { major: 1, minor: 14, patch: 0, pre_release: PreRelease::Beta(1) }));
        assert_eq!(parse_go_version("go1.13.15").unwrap(), ("1.13.15", GoVersion { major: 1, minor: 13, patch: 15, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.13.14").unwrap(), ("1.13.14", GoVersion { major: 1, minor: 13, patch: 14, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.13.13").unwrap(), ("1.13.13", GoVersion { major: 1, minor: 13, patch: 13, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.13.12").unwrap(), ("1.13.12", GoVersion { major: 1, minor: 13, patch: 12, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.13.11").unwrap(), ("1.13.11", GoVersion { major: 1, minor: 13, patch: 11, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.13.10").unwrap(), ("1.13.10", GoVersion { major: 1, minor: 13, patch: 10, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.13.9").unwrap(), ("1.13.9", GoVersion { major: 1, minor: 13, patch: 9, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.13.8").unwrap(), ("1.13.8", GoVersion { major: 1, minor: 13, patch: 8, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.13.7").unwrap(), ("1.13.7", GoVersion { major: 1, minor: 13, patch: 7, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.13.6").unwrap(), ("1.13.6", GoVersion { major: 1, minor: 13, patch: 6, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.13.5").unwrap(), ("1.13.5", GoVersion { major: 1, minor: 13, patch: 5, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.13.4").unwrap(), ("1.13.4", GoVersion { major: 1, minor: 13, patch: 4, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.13.3").unwrap(), ("1.13.3", GoVersion { major: 1, minor: 13, patch: 3, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.13.2").unwrap(), ("1.13.2", GoVersion { major: 1, minor: 13, patch: 2, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.13.1").unwrap(), ("1.13.1", GoVersion { major: 1, minor: 13, patch: 1, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.13").unwrap(), ("1.13", GoVersion { major: 1, minor: 13, patch: 0, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.13rc2").unwrap(), ("1.13rc2", GoVersion { major: 1, minor: 13, patch: 0, pre_release: PreRelease::Rc(2) }));
        assert_eq!(parse_go_version("go1.13rc1").unwrap(), ("1.13rc1", GoVersion { major: 1, minor: 13, patch: 0, pre_release: PreRelease::Rc(1) }));
        assert_eq!(parse_go_version("go1.13beta1").unwrap(), ("1.13beta1", GoVersion { major: 1, minor: 13, patch: 0, pre_release: PreRelease::Beta(1) }));
        assert_eq!(parse_go_version("go1.12.17").unwrap(), ("1.12.17", GoVersion { major: 1, minor: 12, patch: 17, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.12.16").unwrap(), ("1.12.16", GoVersion { major: 1, minor: 12, patch: 16, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.12.15").unwrap(), ("1.12.15", GoVersion { major: 1, minor: 12, patch: 15, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.12.14").unwrap(), ("1.12.14", GoVersion { major: 1, minor: 12, patch: 14, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.12.13").unwrap(), ("1.12.13", GoVersion { major: 1, minor: 12, patch: 13, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.12.12").unwrap(), ("1.12.12", GoVersion { major: 1, minor: 12, patch: 12, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.12.11").unwrap(), ("1.12.11", GoVersion { major: 1, minor: 12, patch: 11, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.12.10").unwrap(), ("1.12.10", GoVersion { major: 1, minor: 12, patch: 10, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.12.9").unwrap(), ("1.12.9", GoVersion { major: 1, minor: 12, patch: 9, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.12.8").unwrap(), ("1.12.8", GoVersion { major: 1, minor: 12, patch: 8, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.12.7").unwrap(), ("1.12.7", GoVersion { major: 1, minor: 12, patch: 7, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.12.6").unwrap(), ("1.12.6", GoVersion { major: 1, minor: 12, patch: 6, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.12.5").unwrap(), ("1.12.5", GoVersion { major: 1, minor: 12, patch: 5, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.12.4").unwrap(), ("1.12.4", GoVersion { major: 1, minor: 12, patch: 4, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.12.3").unwrap(), ("1.12.3", GoVersion { major: 1, minor: 12, patch: 3, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.12.2").unwrap(), ("1.12.2", GoVersion { major: 1, minor: 12, patch: 2, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.12.1").unwrap(), ("1.12.1", GoVersion { major: 1, minor: 12, patch: 1, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.12").unwrap(), ("1.12", GoVersion { major: 1, minor: 12, patch: 0, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.12rc1").unwrap(), ("1.12rc1", GoVersion { major: 1, minor: 12, patch: 0, pre_release: PreRelease::Rc(1) }));
        assert_eq!(parse_go_version("go1.12beta2").unwrap(), ("1.12beta2", GoVersion { major: 1, minor: 12, patch: 0, pre_release: PreRelease::Beta(2) }));
        assert_eq!(parse_go_version("go1.12beta1").unwrap(), ("1.12beta1", GoVersion { major: 1, minor: 12, patch: 0, pre_release: PreRelease::Beta(1) }));
        assert_eq!(parse_go_version("go1.11.13").unwrap(), ("1.11.13", GoVersion { major: 1, minor: 11, patch: 13, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.11.12").unwrap(), ("1.11.12", GoVersion { major: 1, minor: 11, patch: 12, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.11.11").unwrap(), ("1.11.11", GoVersion { major: 1, minor: 11, patch: 11, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.11.10").unwrap(), ("1.11.10", GoVersion { major: 1, minor: 11, patch: 10, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.11.9").unwrap(), ("1.11.9", GoVersion { major: 1, minor: 11, patch: 9, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.11.8").unwrap(), ("1.11.8", GoVersion { major: 1, minor: 11, patch: 8, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.11.7").unwrap(), ("1.11.7", GoVersion { major: 1, minor: 11, patch: 7, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.11.6").unwrap(), ("1.11.6", GoVersion { major: 1, minor: 11, patch: 6, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.11.5").unwrap(), ("1.11.5", GoVersion { major: 1, minor: 11, patch: 5, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.11.4").unwrap(), ("1.11.4", GoVersion { major: 1, minor: 11, patch: 4, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.11.3").unwrap(), ("1.11.3", GoVersion { major: 1, minor: 11, patch: 3, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.11.2").unwrap(), ("1.11.2", GoVersion { major: 1, minor: 11, patch: 2, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.11.1").unwrap(), ("1.11.1", GoVersion { major: 1, minor: 11, patch: 1, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.11").unwrap(), ("1.11", GoVersion { major: 1, minor: 11, patch: 0, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.11rc2").unwrap(), ("1.11rc2", GoVersion { major: 1, minor: 11, patch: 0, pre_release: PreRelease::Rc(2) }));
        assert_eq!(parse_go_version("go1.11rc1").unwrap(), ("1.11rc1", GoVersion { major: 1, minor: 11, patch: 0, pre_release: PreRelease::Rc(1) }));
        assert_eq!(parse_go_version("go1.11beta3").unwrap(), ("1.11beta3", GoVersion { major: 1, minor: 11, patch: 0, pre_release: PreRelease::Beta(3) }));
        assert_eq!(parse_go_version("go1.11beta2").unwrap(), ("1.11beta2", GoVersion { major: 1, minor: 11, patch: 0, pre_release: PreRelease::Beta(2) }));
        assert_eq!(parse_go_version("go1.11beta1").unwrap(), ("1.11beta1", GoVersion { major: 1, minor: 11, patch: 0, pre_release: PreRelease::Beta(1) }));
        assert_eq!(parse_go_version("go1.10.8").unwrap(), ("1.10.8", GoVersion { major: 1, minor: 10, patch: 8, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.10.7").unwrap(), ("1.10.7", GoVersion { major: 1, minor: 10, patch: 7, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.10.6").unwrap(), ("1.10.6", GoVersion { major: 1, minor: 10, patch: 6, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.10.5").unwrap(), ("1.10.5", GoVersion { major: 1, minor: 10, patch: 5, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.10.4").unwrap(), ("1.10.4", GoVersion { major: 1, minor: 10, patch: 4, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.10.3").unwrap(), ("1.10.3", GoVersion { major: 1, minor: 10, patch: 3, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.10.2").unwrap(), ("1.10.2", GoVersion { major: 1, minor: 10, patch: 2, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.10.1").unwrap(), ("1.10.1", GoVersion { major: 1, minor: 10, patch: 1, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.10").unwrap(), ("1.10", GoVersion { major: 1, minor: 10, patch: 0, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.10rc2").unwrap(), ("1.10rc2", GoVersion { major: 1, minor: 10, patch: 0, pre_release: PreRelease::Rc(2) }));
        assert_eq!(parse_go_version("go1.10rc1").unwrap(), ("1.10rc1", GoVersion { major: 1, minor: 10, patch: 0, pre_release: PreRelease::Rc(1) }));
        assert_eq!(parse_go_version("go1.10beta2").unwrap(), ("1.10beta2", GoVersion { major: 1, minor: 10, patch: 0, pre_release: PreRelease::Beta(2) }));
        assert_eq!(parse_go_version("go1.10beta1").unwrap(), ("1.10beta1", GoVersion { major: 1, minor: 10, patch: 0, pre_release: PreRelease::Beta(1) }));
        assert_eq!(parse_go_version("go1.9.7").unwrap(), ("1.9.7", GoVersion { major: 1, minor: 9, patch: 7, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.9.6").unwrap(), ("1.9.6", GoVersion { major: 1, minor: 9, patch: 6, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.9.5").unwrap(), ("1.9.5", GoVersion { major: 1, minor: 9, patch: 5, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.9.4").unwrap(), ("1.9.4", GoVersion { major: 1, minor: 9, patch: 4, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.9.3").unwrap(), ("1.9.3", GoVersion { major: 1, minor: 9, patch: 3, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.9.2").unwrap(), ("1.9.2", GoVersion { major: 1, minor: 9, patch: 2, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.9.2rc2").unwrap(), ("1.9.2rc2", GoVersion { major: 1, minor: 9, patch: 2, pre_release: PreRelease::Rc(2) }));
        assert_eq!(parse_go_version("go1.9.1").unwrap(), ("1.9.1", GoVersion { major: 1, minor: 9, patch: 1, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.9").unwrap(), ("1.9", GoVersion { major: 1, minor: 9, patch: 0, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.9rc2").unwrap(), ("1.9rc2", GoVersion { major: 1, minor: 9, patch: 0, pre_release: PreRelease::Rc(2) }));
        assert_eq!(parse_go_version("go1.9rc1").unwrap(), ("1.9rc1", GoVersion { major: 1, minor: 9, patch: 0, pre_release: PreRelease::Rc(1) }));
        assert_eq!(parse_go_version("go1.9beta2").unwrap(), ("1.9beta2", GoVersion { major: 1, minor: 9, patch: 0, pre_release: PreRelease::Beta(2) }));
        assert_eq!(parse_go_version("go1.9beta1").unwrap(), ("1.9beta1", GoVersion { major: 1, minor: 9, patch: 0, pre_release: PreRelease::Beta(1) }));
        assert_eq!(parse_go_version("go1.8.7").unwrap(), ("1.8.7", GoVersion { major: 1, minor: 8, patch: 7, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.8.6").unwrap(), ("1.8.6", GoVersion { major: 1, minor: 8, patch: 6, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.8.5").unwrap(), ("1.8.5", GoVersion { major: 1, minor: 8, patch: 5, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.8.4").unwrap(), ("1.8.4", GoVersion { major: 1, minor: 8, patch: 4, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.8.3").unwrap(), ("1.8.3", GoVersion { major: 1, minor: 8, patch: 3, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.8.2").unwrap(), ("1.8.2", GoVersion { major: 1, minor: 8, patch: 2, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.8.1").unwrap(), ("1.8.1", GoVersion { major: 1, minor: 8, patch: 1, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.8").unwrap(), ("1.8", GoVersion { major: 1, minor: 8, patch: 0, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.8rc3").unwrap(), ("1.8rc3", GoVersion { major: 1, minor: 8, patch: 0, pre_release: PreRelease::Rc(3) }));
        assert_eq!(parse_go_version("go1.8rc2").unwrap(), ("1.8rc2", GoVersion { major: 1, minor: 8, patch: 0, pre_release: PreRelease::Rc(2) }));
        assert_eq!(parse_go_version("go1.8rc1").unwrap(), ("1.8rc1", GoVersion { major: 1, minor: 8, patch: 0, pre_release: PreRelease::Rc(1) }));
        assert_eq!(parse_go_version("go1.8beta2").unwrap(), ("1.8beta2", GoVersion { major: 1, minor: 8, patch: 0, pre_release: PreRelease::Beta(2) }));
        assert_eq!(parse_go_version("go1.8beta1").unwrap(), ("1.8beta1", GoVersion { major: 1, minor: 8, patch: 0, pre_release: PreRelease::Beta(1) }));
        assert_eq!(parse_go_version("go1.7.6").unwrap(), ("1.7.6", GoVersion { major: 1, minor: 7, patch: 6, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.7.5").unwrap(), ("1.7.5", GoVersion { major: 1, minor: 7, patch: 5, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.7.4").unwrap(), ("1.7.4", GoVersion { major: 1, minor: 7, patch: 4, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.7.3").unwrap(), ("1.7.3", GoVersion { major: 1, minor: 7, patch: 3, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.7.1").unwrap(), ("1.7.1", GoVersion { major: 1, minor: 7, patch: 1, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.7").unwrap(), ("1.7", GoVersion { major: 1, minor: 7, patch: 0, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.7rc6").unwrap(), ("1.7rc6", GoVersion { major: 1, minor: 7, patch: 0, pre_release: PreRelease::Rc(6) }));
        assert_eq!(parse_go_version("go1.7rc5").unwrap(), ("1.7rc5", GoVersion { major: 1, minor: 7, patch: 0, pre_release: PreRelease::Rc(5) }));
        assert_eq!(parse_go_version("go1.7rc4").unwrap(), ("1.7rc4", GoVersion { major: 1, minor: 7, patch: 0, pre_release: PreRelease::Rc(4) }));
        assert_eq!(parse_go_version("go1.7rc3").unwrap(), ("1.7rc3", GoVersion { major: 1, minor: 7, patch: 0, pre_release: PreRelease::Rc(3) }));
        assert_eq!(parse_go_version("go1.7rc2").unwrap(), ("1.7rc2", GoVersion { major: 1, minor: 7, patch: 0, pre_release: PreRelease::Rc(2) }));
        assert_eq!(parse_go_version("go1.7rc1").unwrap(), ("1.7rc1", GoVersion { major: 1, minor: 7, patch: 0, pre_release: PreRelease::Rc(1) }));
        assert_eq!(parse_go_version("go1.7beta2").unwrap(), ("1.7beta2", GoVersion { major: 1, minor: 7, patch: 0, pre_release: PreRelease::Beta(2) }));
        assert_eq!(parse_go_version("go1.7beta1").unwrap(), ("1.7beta1", GoVersion { major: 1, minor: 7, patch: 0, pre_release: PreRelease::Beta(1) }));
        assert_eq!(parse_go_version("go1.6.4").unwrap(), ("1.6.4", GoVersion { major: 1, minor: 6, patch: 4, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.6.3").unwrap(), ("1.6.3", GoVersion { major: 1, minor: 6, patch: 3, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.6.2").unwrap(), ("1.6.2", GoVersion { major: 1, minor: 6, patch: 2, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.6.1").unwrap(), ("1.6.1", GoVersion { major: 1, minor: 6, patch: 1, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.6").unwrap(), ("1.6", GoVersion { major: 1, minor: 6, patch: 0, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.6rc2").unwrap(), ("1.6rc2", GoVersion { major: 1, minor: 6, patch: 0, pre_release: PreRelease::Rc(2) }));
        assert_eq!(parse_go_version("go1.6rc1").unwrap(), ("1.6rc1", GoVersion { major: 1, minor: 6, patch: 0, pre_release: PreRelease::Rc(1) }));
        assert_eq!(parse_go_version("go1.6beta2").unwrap(), ("1.6beta2", GoVersion { major: 1, minor: 6, patch: 0, pre_release: PreRelease::Beta(2) }));
        assert_eq!(parse_go_version("go1.6beta1").unwrap(), ("1.6beta1", GoVersion { major: 1, minor: 6, patch: 0, pre_release: PreRelease::Beta(1) }));
        assert_eq!(parse_go_version("go1.5.4").unwrap(), ("1.5.4", GoVersion { major: 1, minor: 5, patch: 4, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.5.3").unwrap(), ("1.5.3", GoVersion { major: 1, minor: 5, patch: 3, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.5.2").unwrap(), ("1.5.2", GoVersion { major: 1, minor: 5, patch: 2, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.5.1").unwrap(), ("1.5.1", GoVersion { major: 1, minor: 5, patch: 1, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.5").unwrap(), ("1.5", GoVersion { major: 1, minor: 5, patch: 0, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.5rc1").unwrap(), ("1.5rc1", GoVersion { major: 1, minor: 5, patch: 0, pre_release: PreRelease::Rc(1) }));
        assert_eq!(parse_go_version("go1.5beta3").unwrap(), ("1.5beta3", GoVersion { major: 1, minor: 5, patch: 0, pre_release: PreRelease::Beta(3) }));
        assert_eq!(parse_go_version("go1.5beta2").unwrap(), ("1.5beta2", GoVersion { major: 1, minor: 5, patch: 0, pre_release: PreRelease::Beta(2) }));
        assert_eq!(parse_go_version("go1.5beta1").unwrap(), ("1.5beta1", GoVersion { major: 1, minor: 5, patch: 0, pre_release: PreRelease::Beta(1) }));
        assert_eq!(parse_go_version("go1.4.3").unwrap(), ("1.4.3", GoVersion { major: 1, minor: 4, patch: 3, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.4.2").unwrap(), ("1.4.2", GoVersion { major: 1, minor: 4, patch: 2, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.4.1").unwrap(), ("1.4.1", GoVersion { major: 1, minor: 4, patch: 1, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.4").unwrap(), ("1.4", GoVersion { major: 1, minor: 4, patch: 0, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.4rc2").unwrap(), ("1.4rc2", GoVersion { major: 1, minor: 4, patch: 0, pre_release: PreRelease::Rc(2) }));
        assert_eq!(parse_go_version("go1.4rc1").unwrap(), ("1.4rc1", GoVersion { major: 1, minor: 4, patch: 0, pre_release: PreRelease::Rc(1) }));
        assert_eq!(parse_go_version("go1.4beta1").unwrap(), ("1.4beta1", GoVersion { major: 1, minor: 4, patch: 0, pre_release: PreRelease::Beta(1) }));
        assert_eq!(parse_go_version("go1.3.3").unwrap(), ("1.3.3", GoVersion { major: 1, minor: 3, patch: 3, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.3.2").unwrap(), ("1.3.2", GoVersion { major: 1, minor: 3, patch: 2, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.3.1").unwrap(), ("1.3.1", GoVersion { major: 1, minor: 3, patch: 1, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.3").unwrap(), ("1.3", GoVersion { major: 1, minor: 3, patch: 0, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1.3rc2").unwrap(), ("1.3rc2", GoVersion { major: 1, minor: 3, patch: 0, pre_release: PreRelease::Rc(2) }));
        assert_eq!(parse_go_version("go1.3rc1").unwrap(), ("1.3rc1", GoVersion { major: 1, minor: 3, patch: 0, pre_release: PreRelease::Rc(1) }));
        assert_eq!(parse_go_version("go1.2.2").unwrap(), ("1.2.2", GoVersion { major: 1, minor: 2, patch: 2, pre_release: PreRelease::None }));
        assert_eq!(parse_go_version("go1").unwrap(), ("1", GoVersion { major: 1, minor: 0, patch: 0, pre_release: PreRelease::None }));
    }

    #[test]
    fn test_parse_errors() {
        assert!(parse_go_version("go").is_err()); // No version part
        assert!(parse_go_version("go1.").is_err()); // Empty minor part
        assert!(parse_go_version("go1.10.").is_err()); // Empty patch part
        assert!(parse_go_version("go1.10.rc1").is_err()); // rc attached to non-existent element
        assert!(parse_go_version("go1.beta1").is_err()); // beta attached to non-existent element
        assert!(parse_go_version("go1.10beta").is_err()); // Missing beta number
        assert!(parse_go_version("go1.10rc").is_err()); // Missing rc number
        assert!(parse_go_version("go1.10.5rc").is_err()); // Missing rc number after patch
        assert!(parse_go_version("go1.10.5beta").is_err()); // Missing beta number after patch
        assert!(parse_go_version("go1.10.beta1").is_err()); // beta attached to non-existent element after patch
        assert!(parse_go_version("go1.a").is_err()); // Invalid minor version
        assert!(parse_go_version("go1.10.c").is_err()); // Invalid patch version
        assert!(parse_go_version("go1.10betaX").is_err()); // Invalid beta number
        assert!(parse_go_version("go1.10rcY").is_err()); // Invalid rc number
        assert!(parse_go_version("go1.10.5.6").is_err()); // Too many parts
        assert!(parse_go_version("og1.10").is_err()); // Invalid prefix
        assert!(parse_go_version("g1.10").is_err()); // Invalid prefix
        assert!(parse_go_version("go1..2").is_err()); // Empty minor part
        assert!(parse_go_version("go.1.2").is_err()); // Empty major part
    }
}
