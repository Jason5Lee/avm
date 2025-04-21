use anyhow::Context;
use serde::Deserialize;
use smol_str::{SmolStr, ToSmolStr};
use std::path::{Path, PathBuf};
use std::{collections::HashSet, sync::Arc};

use crate::HttpClient;
use crate::{
    platform::{cpu, create_platform_string, current_cpu, current_os, os},
    tool::{DownUrl, InstallVersion, ToolInfo, Version},
};

pub struct Tool {
    client: Arc<HttpClient>,
    info: ToolInfo,
    corresponding_dto_os_arch_bitness: Vec<(&'static str, &'static str, u32)>,
}

const FLAVOR: &[&str] = &[
    "jdk",
    "jdk_full",
    "jdk_lite",
    "jre",
    "jre_full",
    "nik_core",
    "nik_standard",
    "nik_full",
];
const BASE_URL: &str = "https://api.bell-sw.com/v1/";

use async_trait::async_trait;

#[async_trait]
impl crate::tool::GeneralTool for Tool {
    fn info(&self) -> &ToolInfo {
        &self.info
    }

    async fn fetch_versions(
        &self,
        platform: Option<SmolStr>,
        flavor: Option<SmolStr>,
        major_version: Option<SmolStr>,
    ) -> anyhow::Result<Vec<Version>> {
        let platform = platform.ok_or_else(|| anyhow::anyhow!("Platform is required"))?;
        let (cpu, os, bitness) = self.get_dto_os_arch_bitness(&platform);
        let flavor = Flavor::parse(flavor.as_deref())?;

        let mut releases = if flavor.is_nik {
            self.fetch_nik_releases(&self.client, cpu, os, bitness, &flavor, major_version, None)
                .await?
        } else {
            self.fetch_liberica_releases(
                &self.client,
                cpu,
                os,
                bitness,
                &flavor,
                major_version,
                None,
            )
            .await?
        };

        releases.sort_by(|a, b| b.version.cmp(&a.version));
        let mut versions = Vec::new();
        let mut version_set = HashSet::new();
        for release in releases {
            let version_raw = release.version_raw.to_smolstr();
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

    async fn get_down_url(
        &self,
        platform: Option<SmolStr>,
        flavor: Option<SmolStr>,
        version: InstallVersion,
    ) -> anyhow::Result<DownUrl> {
        let platform = platform.ok_or_else(|| anyhow::anyhow!("Platform is required"))?;
        let (cpu, os, bitness) = self.get_dto_os_arch_bitness(&platform);
        let flavor = Flavor::parse(flavor.as_deref())?;

        let (major_version, version) = match version {
            InstallVersion::Latest { major_version } => (Some(major_version), None),
            InstallVersion::Specific { version } => (None, Some(version)),
        };
        let mut releases = if flavor.is_nik {
            self.fetch_nik_releases(
                &self.client,
                cpu,
                os,
                bitness,
                &flavor,
                major_version,
                version,
            )
            .await?
        } else {
            self.fetch_liberica_releases(
                &self.client,
                cpu,
                os,
                bitness,
                &flavor,
                major_version,
                version,
            )
            .await?
        };

        releases.sort_by(|a, b| b.version.cmp(&a.version));
        match releases.into_iter().next() {
            Some(item) => Ok(DownUrl {
                version: item.version_raw.to_smolstr(),
                url: item.download_url.to_smolstr(),
                hash: crate::FileHash {
                    hex: item.sha1.to_smolstr(),
                    algo: crate::FileHashAlgo::Sha1,
                },
            }),
            None => Err(anyhow::anyhow!("No download URL found.")),
        }
    }

    fn bin_path(&self, instance_dir: &Path) -> anyhow::Result<PathBuf> {
        let java_path = instance_dir.join("bin").join("java");
        Ok(java_path)
    }
}

impl Tool {
    pub fn new(client: Arc<HttpClient>) -> Self {
        let (all_platforms, corresponding_dto_os_arch_bitness) =
            Self::get_platforms_and_corresponding_dto_os_arch_bitness();
        let all_flavors = FLAVOR.iter().map(|f| f.to_smolstr()).collect::<Vec<_>>();

        let default_platform = current_cpu().and_then(|cpu| {
            let os = current_os()?;
            let p = create_platform_string(cpu, os);
            all_platforms.iter().find(|&k| p == *k).cloned()
        });

        Tool {
            client,
            info: ToolInfo {
                name: "liberica".to_smolstr(),
                about: "Liberica Java JDK/JRE".to_smolstr(),
                after_long_help: Some(r#"### Flavors

The flavor parameter allows you to specify which type of Liberica JDK or NIK to use, based on your application's needs. The available options include both Java SE Development Kit (JDK) and Java SE Runtime Environment (JRE) distributions, as well as the Liberica Native Image Kit (NIK), which enables Java bytecode to be compiled into native executables.

#### **JDK and JRE Options**

These distributions are tailored for running, compiling, and debugging Java applications or for lightweight runtime environments:
- **`jdk` (Standard version):** A full Java SE Development Kit optimized for server and desktop deployments without additional components.
- **`jdk_full` (Full version):** Includes LibericaFX (based on OpenJFX) and Minimal VM, providing a more feature-complete development environment.
- **`jdk_lite` (Lite version):** Optimized for size, making it ideal for cloud deployments.
- **`jre` (Standard version):** A lightweight Java Runtime Environment for running simple Java applications.
- **`jre_full` (Full version):** Includes LibericaFX and Minimal VM for a richer runtime experience.

#### **NIK (Native Image Kit) Options**

These distributions are designed for building native executables from Java bytecode for improved performance and startup time:
- **`nik_core` (Core version):** A minimal distribution with Liberica VM and native image (based on GraalVM), suitable for Java development.
- **`nik_standard` (Standard version):** Adds support for plugins to enable the use of non-Java programming languages.
- **`nik_full` (Full version):** A comprehensive build that includes LibericaFX for GUI-based applications."#.to_smolstr()),
                all_platforms: Some(all_platforms),
                default_platform,
                all_flavors: Some(all_flavors),
                default_flavor: Some("jdk".to_smolstr()),
                version_is_major: false,
            },
            corresponding_dto_os_arch_bitness,
        }
    }

    fn get_platforms_and_corresponding_dto_os_arch_bitness(
    ) -> (Vec<SmolStr>, Vec<(&'static str, &'static str, u32)>) {
        let mut platforms = Vec::new();
        let mut corresponding_dto_os_arch_bitness = Vec::new();
        let mut add = |cpu: &str,
                       os: &str,
                       dto_os: &'static str,
                       dto_arch: &'static str,
                       dto_bitness: u32| {
            platforms.push(create_platform_string(cpu, os));
            corresponding_dto_os_arch_bitness.push((dto_arch, dto_os, dto_bitness));
        };

        add(cpu::X86, os::LINUX, "linux", "x86", 32);
        add(cpu::X64, os::LINUX, "linux", "x86", 64);
        add(cpu::ARM32, os::LINUX, "linux", "arm", 32);
        add(cpu::ARM64, os::LINUX, "linux", "arm", 64);
        add(cpu::PPC64, os::LINUX, "linux", "ppc", 64);
        add(cpu::RISCV64, os::LINUX, "linux", "riscv", 64);

        add(cpu::ARM64, os::WIN, "windows", "arm", 64);
        add(cpu::X86, os::WIN, "windows", "x86", 32);
        add(cpu::X64, os::WIN, "windows", "x86", 64);

        add(cpu::X64, os::LINUX_MUSL, "linux-musl", "x86", 64);
        add(cpu::ARM64, os::LINUX_MUSL, "linux-musl", "arm", 64);

        add(cpu::X64, os::MAC, "macos", "x86", 64);
        add(cpu::ARM64, os::MAC, "macos", "arm", 64);

        add(cpu::SPARC64, os::SOLARIS, "solaris", "sparc", 64);
        add(cpu::X64, os::SOLARIS, "solaris", "x86", 64);

        (platforms, corresponding_dto_os_arch_bitness)
    }

    fn get_dto_os_arch_bitness(&self, platform: &str) -> (&'static str, &'static str, u32) {
        let index = self
            .info
            .all_platforms
            .as_ref()
            .unwrap()
            .iter()
            .position(|p| p == platform)
            .unwrap();
        self.corresponding_dto_os_arch_bitness[index]
    }

    async fn fetch_liberica_releases(
        &self,
        client: &HttpClient,
        cpu: &str,
        os: &str,
        bitness: u32,
        flavor: &Flavor,
        major_version: Option<SmolStr>,
        version: Option<SmolStr>,
    ) -> anyhow::Result<Vec<ReleaseItem>> {
        let url = format!("{}liberica/releases", BASE_URL);
        let mut request_builder =
            self.build_parameters(client.get(&url), cpu, os, bitness, &flavor.bundle_type)?;

        if let Some(major_version) = major_version {
            request_builder = request_builder.query(&[("version-feature", &major_version)]);
        }

        if let Some(ver) = version {
            request_builder = request_builder.query(&[("version", ver.as_str())]);
        }

        let response: Vec<ReleaseItemDto> = request_builder
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        Ok(response.into_iter().map(ReleaseItem::from).collect())
    }

    async fn fetch_nik_releases(
        &self,
        client: &HttpClient,
        cpu: &str,
        os: &str,
        bitness: u32,
        flavor: &Flavor,
        major_version: Option<SmolStr>,
        version: Option<SmolStr>,
    ) -> anyhow::Result<Vec<ReleaseItem>> {
        let url = format!("{}nik/releases", BASE_URL);
        let mut request_builder =
            self.build_parameters(client.get(&url), cpu, os, bitness, &flavor.bundle_type)?;

        if let Some(version) = version {
            request_builder = request_builder.query(&[("version", format!("liberica@{version}"))]);
        }

        let response = request_builder
            .send()
            .await?
            .error_for_status()?
            .json::<Vec<NikReleaseItemDto>>()
            .await?;

        let releases: Vec<ReleaseItem> = response
            .into_iter()
            .map(ReleaseItem::try_from)
            .collect::<Result<_, _>>()?;

        if let Some(major_version) = major_version {
            let major_version = major_version
                .parse::<i32>()
                .context("Invalid major version")?;
            Ok(releases
                .into_iter()
                .filter(|r| r.version.major == major_version)
                .collect())
        } else {
            Ok(releases)
        }
    }

    fn build_parameters(
        &self,
        request_builder: reqwest::RequestBuilder,
        arch: &str,
        os: &str,
        bitness: u32,
        bundle_type: &str,
    ) -> anyhow::Result<reqwest::RequestBuilder> {
        Ok(request_builder.query(&[
            ("arch", arch),
            ("os", os),
            ("installationType", "archive"),
            ("bitness", &bitness.to_string()),
            ("bundle-type", bundle_type),
        ]))
    }
}

#[derive(Debug)]
struct Flavor {
    is_nik: bool,
    bundle_type: SmolStr,
}

impl Flavor {
    fn parse(s: Option<&str>) -> anyhow::Result<Flavor> {
        let s = s.unwrap_or("jdk");
        let is_nik = s.starts_with("nik");
        let bundle_type = s.strip_prefix("nik_").unwrap_or(s).to_smolstr();

        if is_nik && !["core", "standard", "full"].contains(&bundle_type.as_str()) {
            anyhow::bail!("Invalid nik flavor: {}", s);
        }
        if !is_nik
            && !["jdk", "jdk_full", "jdk_lite", "jre", "jre_full"].contains(&bundle_type.as_str())
        {
            anyhow::bail!("Invalid jdk/jre flavor: {}", s);
        }

        Ok(Flavor {
            is_nik,
            bundle_type,
        })
    }
}

#[derive(Debug)]
struct ReleaseItem {
    download_url: String,
    sha1: String,
    version_raw: String,
    version: JdkVersion,
    lts: bool,
}

impl From<ReleaseItemDto> for ReleaseItem {
    fn from(value: ReleaseItemDto) -> Self {
        Self {
            download_url: value.download_url,
            sha1: value.sha1,
            version: JdkVersion::parse(&value.version),
            version_raw: value.version,
            lts: value.lts,
        }
    }
}

impl TryFrom<NikReleaseItemDto> for ReleaseItem {
    type Error = anyhow::Error;

    fn try_from(value: NikReleaseItemDto) -> Result<Self, Self::Error> {
        let java_component = value
            .components
            .iter()
            .find(|c| c.component == "liberica")
            .context("No liberica component found in NIK release")?;
        Ok(Self {
            download_url: value.download_url,
            sha1: value.sha1,
            version: JdkVersion::parse(&java_component.version),
            version_raw: java_component.version.clone(),
            lts: value.lts,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReleaseItemDto {
    download_url: String,
    sha1: String,
    version: String,
    #[serde(rename = "LTS")]
    lts: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NikReleaseItemDto {
    download_url: String,
    sha1: String,
    components: Vec<NikComponentDto>,
    // version: String,
    #[serde(rename = "LTS")]
    lts: bool,
}

#[derive(Debug, Deserialize)]
struct NikComponentDto {
    version: String,
    component: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct JdkVersion {
    pub major: i32,
    pub minor: i32,
    pub security: i32,
    pub patch: i32,
    pub build: i32,
}

impl JdkVersion {
    pub(crate) fn parse(version: &str) -> Self {
        let mut major = 0;
        let mut minor = 0;
        let mut security = 0;
        let mut patch = 0;
        let mut build = 0;

        if version.to_lowercase().starts_with("8u") {
            major = 8;
            minor = 0;
            let rest = &version[2..]; // Remove '8u'
            let mut parts = rest.split('+');
            if let Some(security_part) = parts.next() {
                security = security_part.parse().unwrap_or(0);
            }
            if let Some(build_part) = parts.next() {
                build = build_part.parse().unwrap_or(0);
            }
        } else {
            let mut parts = version.split('+');
            let version_part = parts.next().unwrap_or("");
            if let Some(build_part) = parts.next() {
                build = build_part.parse().unwrap_or(0);
            }

            let version_numbers: Vec<&str> = version_part.split('.').collect();
            if let Some(&v) = version_numbers.get(0) {
                major = v.parse().unwrap_or(0);
            }
            if let Some(&v) = version_numbers.get(1) {
                minor = v.parse().unwrap_or(0);
            }
            if let Some(&v) = version_numbers.get(2) {
                security = v.parse().unwrap_or(0);
            }
            if let Some(&v) = version_numbers.get(3) {
                patch = v.parse().unwrap_or(0);
            }
        }

        Self {
            major,
            minor,
            security,
            patch,
            build,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::JdkVersion;

    #[test]
    #[rustfmt::skip]
    fn test_parse_jdk_version_8_to_23() {
        assert_eq!(JdkVersion::parse("23.0.1+13"), JdkVersion { major: 23, minor: 0, security: 1, patch: 0, build: 13 });
        assert_eq!(JdkVersion::parse("23+38"), JdkVersion { major: 23, minor: 0, security: 0, patch: 0, build: 38 });
        assert_eq!(JdkVersion::parse("22.0.2+11"), JdkVersion { major: 22, minor: 0, security: 2, patch: 0, build: 11 });
        assert_eq!(JdkVersion::parse("22.0.1+12"), JdkVersion { major: 22, minor: 0, security: 1, patch: 0, build: 12 });
        assert_eq!(JdkVersion::parse("22.0.1+10"), JdkVersion { major: 22, minor: 0, security: 1, patch: 0, build: 10 });
        assert_eq!(JdkVersion::parse("22+37"), JdkVersion { major: 22, minor: 0, security: 0, patch: 0, build: 37 });
        assert_eq!(JdkVersion::parse("21.0.5+11"), JdkVersion { major: 21, minor: 0, security: 5, patch: 0, build: 11 });
        assert_eq!(JdkVersion::parse("21.0.4+9"), JdkVersion { major: 21, minor: 0, security: 4, patch: 0, build: 9 });
        assert_eq!(JdkVersion::parse("21.0.3+12"), JdkVersion { major: 21, minor: 0, security: 3, patch: 0, build: 12 });
        assert_eq!(JdkVersion::parse("21.0.3+10"), JdkVersion { major: 21, minor: 0, security: 3, patch: 0, build: 10 });
        assert_eq!(JdkVersion::parse("21.0.2+14"), JdkVersion { major: 21, minor: 0, security: 2, patch: 0, build: 14 });
        assert_eq!(JdkVersion::parse("21.0.1+12"), JdkVersion { major: 21, minor: 0, security: 1, patch: 0, build: 12 });
        assert_eq!(JdkVersion::parse("21+37"), JdkVersion { major: 21, minor: 0, security: 0, patch: 0, build: 37 });
        assert_eq!(JdkVersion::parse("20.0.2+10"), JdkVersion { major: 20, minor: 0, security: 2, patch: 0, build: 10 });
        assert_eq!(JdkVersion::parse("20.0.1+10"), JdkVersion { major: 20, minor: 0, security: 1, patch: 0, build: 10 });
        assert_eq!(JdkVersion::parse("20+37"), JdkVersion { major: 20, minor: 0, security: 0, patch: 0, build: 37 });
        assert_eq!(JdkVersion::parse("19.0.2+9"), JdkVersion { major: 19, minor: 0, security: 2, patch: 0, build: 9 });
        assert_eq!(JdkVersion::parse("19.0.1+11"), JdkVersion { major: 19, minor: 0, security: 1, patch: 0, build: 11 });
        assert_eq!(JdkVersion::parse("19+37"), JdkVersion { major: 19, minor: 0, security: 0, patch: 0, build: 37 });
        assert_eq!(JdkVersion::parse("18.0.2+10"), JdkVersion { major: 18, minor: 0, security: 2, patch: 0, build: 10 });
        assert_eq!(JdkVersion::parse("18.0.2.1+1"), JdkVersion { major: 18, minor: 0, security: 2, patch: 1, build: 1 });
        assert_eq!(JdkVersion::parse("18.0.1+12"), JdkVersion { major: 18, minor: 0, security: 1, patch: 0, build: 12 });
        assert_eq!(JdkVersion::parse("18.0.1.1+2"), JdkVersion { major: 18, minor: 0, security: 1, patch: 1, build: 2 });
        assert_eq!(JdkVersion::parse("18+37"), JdkVersion { major: 18, minor: 0, security: 0, patch: 0, build: 37 });
        assert_eq!(JdkVersion::parse("17.0.13+12"), JdkVersion { major: 17, minor: 0, security: 13, patch: 0, build: 12 });
        assert_eq!(JdkVersion::parse("17.0.12+10"), JdkVersion { major: 17, minor: 0, security: 12, patch: 0, build: 10 });
        assert_eq!(JdkVersion::parse("17.0.11+12"), JdkVersion { major: 17, minor: 0, security: 11, patch: 0, build: 12 });
        assert_eq!(JdkVersion::parse("17.0.11+10"), JdkVersion { major: 17, minor: 0, security: 11, patch: 0, build: 10 });
        assert_eq!(JdkVersion::parse("17.0.10+13"), JdkVersion { major: 17, minor: 0, security: 10, patch: 0, build: 13 });
        assert_eq!(JdkVersion::parse("17.0.9+11"), JdkVersion { major: 17, minor: 0, security: 9, patch: 0, build: 11 });
        assert_eq!(JdkVersion::parse("17.0.8+7"), JdkVersion { major: 17, minor: 0, security: 8, patch: 0, build: 7 });
        assert_eq!(JdkVersion::parse("17.0.8.1+1"), JdkVersion { major: 17, minor: 0, security: 8, patch: 1, build: 1 });
        assert_eq!(JdkVersion::parse("17.0.7+7"), JdkVersion { major: 17, minor: 0, security: 7, patch: 0, build: 7 });
        assert_eq!(JdkVersion::parse("17.0.6+10"), JdkVersion { major: 17, minor: 0, security: 6, patch: 0, build: 10 });
        assert_eq!(JdkVersion::parse("17.0.5+8"), JdkVersion { major: 17, minor: 0, security: 5, patch: 0, build: 8 });
        assert_eq!(JdkVersion::parse("17.0.4+8"), JdkVersion { major: 17, minor: 0, security: 4, patch: 0, build: 8 });
        assert_eq!(JdkVersion::parse("17.0.4.1+1"), JdkVersion { major: 17, minor: 0, security: 4, patch: 1, build: 1 });
        assert_eq!(JdkVersion::parse("17.0.3+7"), JdkVersion { major: 17, minor: 0, security: 3, patch: 0, build: 7 });
        assert_eq!(JdkVersion::parse("17.0.3.1+2"), JdkVersion { major: 17, minor: 0, security: 3, patch: 1, build: 2 });
        assert_eq!(JdkVersion::parse("17.0.2+9"), JdkVersion { major: 17, minor: 0, security: 2, patch: 0, build: 9 });
        assert_eq!(JdkVersion::parse("17.0.1+12"), JdkVersion { major: 17, minor: 0, security: 1, patch: 0, build: 12 });
        assert_eq!(JdkVersion::parse("17+35"), JdkVersion { major: 17, minor: 0, security: 0, patch: 0, build: 35 });
        assert_eq!(JdkVersion::parse("16.0.2+7"), JdkVersion { major: 16, minor: 0, security: 2, patch: 0, build: 7 });
        assert_eq!(JdkVersion::parse("16.0.1+9"), JdkVersion { major: 16, minor: 0, security: 1, patch: 0, build: 9 });
        assert_eq!(JdkVersion::parse("16+36"), JdkVersion { major: 16, minor: 0, security: 0, patch: 0, build: 36 });
        assert_eq!(JdkVersion::parse("15.0.2+10"), JdkVersion { major: 15, minor: 0, security: 2, patch: 0, build: 10 });
        assert_eq!(JdkVersion::parse("15.0.2+8"), JdkVersion { major: 15, minor: 0, security: 2, patch: 0, build: 8 });
        assert_eq!(JdkVersion::parse("15.0.1+9"), JdkVersion { major: 15, minor: 0, security: 1, patch: 0, build: 9 });
        assert_eq!(JdkVersion::parse("15+36"), JdkVersion { major: 15, minor: 0, security: 0, patch: 0, build: 36 });
        assert_eq!(JdkVersion::parse("14.0.2+13"), JdkVersion { major: 14, minor: 0, security: 2, patch: 0, build: 13 });
        assert_eq!(JdkVersion::parse("14.0.1+8"), JdkVersion { major: 14, minor: 0, security: 1, patch: 0, build: 8 });
        assert_eq!(JdkVersion::parse("14+36"), JdkVersion { major: 14, minor: 0, security: 0, patch: 0, build: 36 });
        assert_eq!(JdkVersion::parse("13.0.2+9"), JdkVersion { major: 13, minor: 0, security: 2, patch: 0, build: 9 });
        assert_eq!(JdkVersion::parse("13.0.1+10"), JdkVersion { major: 13, minor: 0, security: 1, patch: 0, build: 10 });
        assert_eq!(JdkVersion::parse("13.0.1+9"), JdkVersion { major: 13, minor: 0, security: 1, patch: 0, build: 9 });
        assert_eq!(JdkVersion::parse("13+33"), JdkVersion { major: 13, minor: 0, security: 0, patch: 0, build: 33 });
        assert_eq!(JdkVersion::parse("12.0.2+10"), JdkVersion { major: 12, minor: 0, security: 2, patch: 0, build: 10 });
        assert_eq!(JdkVersion::parse("12.0.1+12"), JdkVersion { major: 12, minor: 0, security: 1, patch: 0, build: 12 });
        assert_eq!(JdkVersion::parse("12+33"), JdkVersion { major: 12, minor: 0, security: 0, patch: 0, build: 33 });
        assert_eq!(JdkVersion::parse("11.0.25+11"), JdkVersion { major: 11, minor: 0, security: 25, patch: 0, build: 11 });
        assert_eq!(JdkVersion::parse("11.0.24+9"), JdkVersion { major: 11, minor: 0, security: 24, patch: 0, build: 9 });
        assert_eq!(JdkVersion::parse("11.0.23+12"), JdkVersion { major: 11, minor: 0, security: 23, patch: 0, build: 12 });
        assert_eq!(JdkVersion::parse("11.0.23+10"), JdkVersion { major: 11, minor: 0, security: 23, patch: 0, build: 10 });
        assert_eq!(JdkVersion::parse("11.0.22+12"), JdkVersion { major: 11, minor: 0, security: 22, patch: 0, build: 12 });
        assert_eq!(JdkVersion::parse("11.0.21+10"), JdkVersion { major: 11, minor: 0, security: 21, patch: 0, build: 10 });
        assert_eq!(JdkVersion::parse("11.0.20+8"), JdkVersion { major: 11, minor: 0, security: 20, patch: 0, build: 8 });
        assert_eq!(JdkVersion::parse("11.0.20.1+1"), JdkVersion { major: 11, minor: 0, security: 20, patch: 1, build: 1 });
        assert_eq!(JdkVersion::parse("11.0.19+7"), JdkVersion { major: 11, minor: 0, security: 19, patch: 0, build: 7 });
        assert_eq!(JdkVersion::parse("11.0.18+10"), JdkVersion { major: 11, minor: 0, security: 18, patch: 0, build: 10 });
        assert_eq!(JdkVersion::parse("11.0.17+7"), JdkVersion { major: 11, minor: 0, security: 17, patch: 0, build: 7 });
        assert_eq!(JdkVersion::parse("11.0.16+8"), JdkVersion { major: 11, minor: 0, security: 16, patch: 0, build: 8 });
        assert_eq!(JdkVersion::parse("11.0.16.1+1"), JdkVersion { major: 11, minor: 0, security: 16, patch: 1, build: 1 });
        assert_eq!(JdkVersion::parse("11.0.15+10"), JdkVersion { major: 11, minor: 0, security: 15, patch: 0, build: 10 });
        assert_eq!(JdkVersion::parse("11.0.15.1+2"), JdkVersion { major: 11, minor: 0, security: 15, patch: 1, build: 2 });
        assert_eq!(JdkVersion::parse("11.0.14+9"), JdkVersion { major: 11, minor: 0, security: 14, patch: 0, build: 9 });
        assert_eq!(JdkVersion::parse("11.0.14.1+1"), JdkVersion { major: 11, minor: 0, security: 14, patch: 1, build: 1 });
        assert_eq!(JdkVersion::parse("11.0.13+8"), JdkVersion { major: 11, minor: 0, security: 13, patch: 0, build: 8 });
        assert_eq!(JdkVersion::parse("11.0.12+7"), JdkVersion { major: 11, minor: 0, security: 12, patch: 0, build: 7 });
        assert_eq!(JdkVersion::parse("11.0.11+9"), JdkVersion { major: 11, minor: 0, security: 11, patch: 0, build: 9 });
        assert_eq!(JdkVersion::parse("11.0.10+9"), JdkVersion { major: 11, minor: 0, security: 10, patch: 0, build: 9 });
        assert_eq!(JdkVersion::parse("11.0.9+12"), JdkVersion { major: 11, minor: 0, security: 9, patch: 0, build: 12 });
        assert_eq!(JdkVersion::parse("11.0.9+11"), JdkVersion { major: 11, minor: 0, security: 9, patch: 0, build: 11 });
        assert_eq!(JdkVersion::parse("11.0.9.1+1"), JdkVersion { major: 11, minor: 0, security: 9, patch: 1, build: 1 });
        assert_eq!(JdkVersion::parse("11.0.8+10"), JdkVersion { major: 11, minor: 0, security: 8, patch: 0, build: 10 });
        assert_eq!(JdkVersion::parse("11.0.7+10"), JdkVersion { major: 11, minor: 0, security: 7, patch: 0, build: 10 });
        assert_eq!(JdkVersion::parse("11.0.6+10"), JdkVersion { major: 11, minor: 0, security: 6, patch: 0, build: 10 });
        assert_eq!(JdkVersion::parse("11.0.5+11"), JdkVersion { major: 11, minor: 0, security: 5, patch: 0, build: 11 });
        assert_eq!(JdkVersion::parse("11.0.5+10"), JdkVersion { major: 11, minor: 0, security: 5, patch: 0, build: 10 });
        assert_eq!(JdkVersion::parse("11.0.4+10"), JdkVersion { major: 11, minor: 0, security: 4, patch: 0, build: 10 });
        assert_eq!(JdkVersion::parse("11.0.3+12"), JdkVersion { major: 11, minor: 0, security: 3, patch: 0, build: 12 });
        assert_eq!(JdkVersion::parse("11.0.2+7"), JdkVersion { major: 11, minor: 0, security: 2, patch: 0, build: 7 });
        assert_eq!(JdkVersion::parse("11.0.1"), JdkVersion { major: 11, minor: 0, security: 1, patch: 0, build: 0 });
        assert_eq!(JdkVersion::parse("11"), JdkVersion { major: 11, minor: 0, security: 0, patch: 0, build: 0 });
        assert_eq!(JdkVersion::parse("8u432+7"), JdkVersion { major: 8, minor: 0, security: 432, patch: 0, build: 7 });
        assert_eq!(JdkVersion::parse("8u422+6"), JdkVersion { major: 8, minor: 0, security: 422, patch: 0, build: 6 });
        assert_eq!(JdkVersion::parse("8u412+9"), JdkVersion { major: 8, minor: 0, security: 412, patch: 0, build: 9 });
        assert_eq!(JdkVersion::parse("8u402+7"), JdkVersion { major: 8, minor: 0, security: 402, patch: 0, build: 7 });
        assert_eq!(JdkVersion::parse("8u392+9"), JdkVersion { major: 8, minor: 0, security: 392, patch: 0, build: 9 });
        assert_eq!(JdkVersion::parse("8u382+6"), JdkVersion { major: 8, minor: 0, security: 382, patch: 0, build: 6 });
        assert_eq!(JdkVersion::parse("8u372+7"), JdkVersion { major: 8, minor: 0, security: 372, patch: 0, build: 7 });
        assert_eq!(JdkVersion::parse("8u362+9"), JdkVersion { major: 8, minor: 0, security: 362, patch: 0, build: 9 });
        assert_eq!(JdkVersion::parse("8u352+8"), JdkVersion { major: 8, minor: 0, security: 352, patch: 0, build: 8 });
        assert_eq!(JdkVersion::parse("8u345+1"), JdkVersion { major: 8, minor: 0, security: 345, patch: 0, build: 1 });
        assert_eq!(JdkVersion::parse("8u342+7"), JdkVersion { major: 8, minor: 0, security: 342, patch: 0, build: 7 });
        assert_eq!(JdkVersion::parse("8u333+2"), JdkVersion { major: 8, minor: 0, security: 333, patch: 0, build: 2 });
        assert_eq!(JdkVersion::parse("8u332+9"), JdkVersion { major: 8, minor: 0, security: 332, patch: 0, build: 9 });
        assert_eq!(JdkVersion::parse("8u322+6"), JdkVersion { major: 8, minor: 0, security: 322, patch: 0, build: 6 });
        assert_eq!(JdkVersion::parse("8u312+7"), JdkVersion { major: 8, minor: 0, security: 312, patch: 0, build: 7 });
        assert_eq!(JdkVersion::parse("8u302+8"), JdkVersion { major: 8, minor: 0, security: 302, patch: 0, build: 8 });
        assert_eq!(JdkVersion::parse("8u292+10"), JdkVersion { major: 8, minor: 0, security: 292, patch: 0, build: 10 });
        assert_eq!(JdkVersion::parse("8u282+8"), JdkVersion { major: 8, minor: 0, security: 282, patch: 0, build: 8 });
        assert_eq!(JdkVersion::parse("8u275+1"), JdkVersion { major: 8, minor: 0, security: 275, patch: 0, build: 1 });
        assert_eq!(JdkVersion::parse("8u272+10"), JdkVersion { major: 8, minor: 0, security: 272, patch: 0, build: 10 });
        assert_eq!(JdkVersion::parse("8u265+1"), JdkVersion { major: 8, minor: 0, security: 265, patch: 0, build: 1 });
        assert_eq!(JdkVersion::parse("8u262+10"), JdkVersion { major: 8, minor: 0, security: 262, patch: 0, build: 10 });
        assert_eq!(JdkVersion::parse("8u252+9"), JdkVersion { major: 8, minor: 0, security: 252, patch: 0, build: 9 });
        assert_eq!(JdkVersion::parse("8u242+7"), JdkVersion { major: 8, minor: 0, security: 242, patch: 0, build: 7 });
        assert_eq!(JdkVersion::parse("8u232+10"), JdkVersion { major: 8, minor: 0, security: 232, patch: 0, build: 10 });
        assert_eq!(JdkVersion::parse("8u232+9"), JdkVersion { major: 8, minor: 0, security: 232, patch: 0, build: 9 });
        assert_eq!(JdkVersion::parse("8u222+11"), JdkVersion { major: 8, minor: 0, security: 222, patch: 0, build: 11 });
        assert_eq!(JdkVersion::parse("8u212+12"), JdkVersion { major: 8, minor: 0, security: 212, patch: 0, build: 12 });
        assert_eq!(JdkVersion::parse("8u202+8"), JdkVersion { major: 8, minor: 0, security: 202, patch: 0, build: 8 });
    }
}
