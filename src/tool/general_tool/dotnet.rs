use rustc_hash::FxHashSet;
use serde::Deserialize;
use smol_str::SmolStr;
use std::cmp::Ordering;
use std::path::PathBuf;
use std::sync::Arc;

use crate::HttpClient;
use crate::{
    platform::{cpu, create_platform_string, current_cpu, current_os, os},
    tool::{ToolDownInfo, ToolInfo, Version, VersionFilter},
};

const RELEASES_INDEX_URL: &str =
    "https://builds.dotnet.microsoft.com/dotnet/release-metadata/releases-index.json";
const FLAVORS: &[&str] = &[
    "sdk",
    "runtime",
    "aspnetcore_runtime",
    "windowsdesktop_runtime",
];

pub struct Tool {
    client: Arc<HttpClient>,
    info: ToolInfo,
    corresponding_rids: Vec<&'static str>,
}

impl crate::tool::GeneralTool for Tool {
    fn info(&self) -> &ToolInfo {
        &self.info
    }

    fn describe_flavor(&self, flavor: &str) -> &'static str {
        match flavor {
            "sdk" => "The full .NET SDK including the CLI, compiler, and runtime.",
            "runtime" => "The base .NET runtime without ASP.NET Core or desktop components.",
            "aspnetcore_runtime" => {
                "The .NET runtime bundled with ASP.NET Core runtime components."
            }
            "windowsdesktop_runtime" => {
                "The Windows Desktop runtime for WinForms and WPF applications."
            }
            _ => "Tool-specific build flavor.",
        }
    }

    async fn fetch_versions(
        &self,
        platform: Option<SmolStr>,
        flavor: Option<SmolStr>,
        version_filter: VersionFilter,
    ) -> anyhow::Result<Vec<Version>> {
        let platform = platform.ok_or_else(|| anyhow::anyhow!("Platform is required"))?;
        let rid = self.get_rid(&platform)?;
        let flavor = Flavor::parse(flavor.as_deref())?;

        let mut releases = self
            .collect_matching_releases(rid, flavor, &version_filter)
            .await?;
        releases.sort_by(|a, b| a.version.cmp(&b.version));

        let mut versions = Vec::new();
        let mut version_set = FxHashSet::default();
        for release in releases {
            if version_set.insert(release.version_raw.clone()) {
                versions.push(Version {
                    version: release.version_raw,
                    is_lts: release.is_lts,
                });
            }
        }

        Ok(versions)
    }

    async fn get_down_info(
        &self,
        platform: Option<SmolStr>,
        flavor: Option<SmolStr>,
        version_filter: VersionFilter,
    ) -> anyhow::Result<ToolDownInfo> {
        let platform = platform.ok_or_else(|| anyhow::anyhow!("Platform is required"))?;
        let rid = self.get_rid(&platform)?;
        let flavor = Flavor::parse(flavor.as_deref())?;

        let release = self
            .find_latest_matching_release(rid, flavor, &version_filter)
            .await?;

        match release {
            Some(release) => Ok(ToolDownInfo {
                version: Version {
                    version: release.version_raw,
                    is_lts: release.is_lts,
                },
                url: release.url,
                hash: crate::FileHash {
                    sha512: Some(release.hash),
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
        tags_and_versions
            .filter_map(|(tag, version_info)| {
                let parsed = parse_dotnet_version(&version_info.version).ok()?;
                if !matches_version_filter(
                    &version_info.version,
                    &parsed,
                    version_info.is_lts,
                    version_filter,
                ) {
                    return None;
                }
                Some((parsed, SmolStr::from(tag)))
            })
            .max_by(|a, b| a.0.cmp(&b.0))
            .map(|(_, tag)| tag)
    }

    fn entry_path(&self, tag_dir: PathBuf) -> anyhow::Result<PathBuf> {
        let mut path = tag_dir;
        #[cfg(windows)]
        path.push("dotnet.exe");
        #[cfg(not(windows))]
        path.push("dotnet");
        Ok(path)
    }
}

impl Tool {
    pub fn new(client: Arc<HttpClient>, config_default_platform: Option<SmolStr>) -> Self {
        let (all_platforms, corresponding_rids) = Self::get_platforms_and_rids();

        let default_platform = config_default_platform
            .and_then(|p| all_platforms.iter().find(|&k| p == *k).cloned())
            .or_else(|| {
                current_cpu().and_then(|cpu| {
                    let os = current_os()?;
                    let platform = create_platform_string(cpu, os);
                    all_platforms.iter().find(|&k| platform == *k).cloned()
                })
            });

        Self {
            client,
            info: ToolInfo {
                about: ".NET SDK and runtimes".into(),
                after_long_help: Some(r#"### Flavors

- `sdk`: Full .NET SDK archive.
- `runtime`: Base .NET runtime archive.
- `aspnetcore_runtime`: Runtime archive with ASP.NET Core.
- `windowsdesktop_runtime`: Windows Desktop runtime archive.

The selected flavor controls which artifact family is queried from the official .NET release metadata."#.into()),
                all_platforms: Some(all_platforms),
                default_platform,
                all_flavors: Some(FLAVORS.iter().map(SmolStr::new).collect()),
                default_flavor: Some("sdk".into()),
            },
            corresponding_rids,
        }
    }

    fn get_platforms_and_rids() -> (Vec<SmolStr>, Vec<&'static str>) {
        let mut platforms = Vec::new();
        let mut rids = Vec::new();
        let mut add = |cpu: &str, os: &str, rid: &'static str| {
            platforms.push(create_platform_string(cpu, os));
            rids.push(rid);
        };

        add(cpu::ARM32, os::LINUX, "linux-arm");
        add(cpu::ARM64, os::LINUX, "linux-arm64");
        add(cpu::X64, os::LINUX, "linux-x64");

        add(cpu::ARM32, os::LINUX_MUSL, "linux-musl-arm");
        add(cpu::ARM64, os::LINUX_MUSL, "linux-musl-arm64");
        add(cpu::X64, os::LINUX_MUSL, "linux-musl-x64");

        add(cpu::ARM64, os::MAC, "osx-arm64");
        add(cpu::X64, os::MAC, "osx-x64");

        add(cpu::ARM64, os::WIN, "win-arm64");
        add(cpu::X64, os::WIN, "win-x64");
        add(cpu::X86, os::WIN, "win-x86");

        (platforms, rids)
    }

    fn get_rid(&self, platform: &SmolStr) -> anyhow::Result<&'static str> {
        let platforms = self.info.all_platforms.as_ref().ok_or_else(|| {
            anyhow::anyhow!("dotnet tool metadata is missing supported platforms")
        })?;
        let index = platforms
            .iter()
            .position(|p| p == platform)
            .ok_or_else(|| anyhow::anyhow!("Unsupported .NET platform: {platform}"))?;

        self.corresponding_rids
            .get(index)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("Missing RID mapping for .NET platform: {platform}"))
    }

    async fn collect_matching_releases(
        &self,
        rid: &str,
        flavor: Flavor,
        version_filter: &VersionFilter,
    ) -> anyhow::Result<Vec<DotnetReleaseAsset>> {
        let channels = self.matching_release_channels(version_filter).await?;
        let mut releases = Vec::new();

        for channel in channels {
            let channel_release = self.fetch_channel_release(&channel.releases_json).await?;
            releases.extend(select_release_assets(
                channel_release,
                rid,
                flavor,
                version_filter,
            ));
        }

        Ok(releases)
    }

    async fn find_latest_matching_release(
        &self,
        rid: &str,
        flavor: Flavor,
        version_filter: &VersionFilter,
    ) -> anyhow::Result<Option<DotnetReleaseAsset>> {
        let channels = self.matching_release_channels(version_filter).await?;

        for channel in channels {
            let channel_release = self.fetch_channel_release(&channel.releases_json).await?;
            if let Some(release) =
                select_latest_release_asset(channel_release, rid, flavor, version_filter)
            {
                return Ok(Some(release));
            }
        }

        Ok(None)
    }

    async fn matching_release_channels(
        &self,
        version_filter: &VersionFilter,
    ) -> anyhow::Result<Vec<ReleaseChannel>> {
        if let Some(channel) = direct_release_channel(version_filter) {
            return Ok(vec![channel]);
        }

        self.fetch_release_channels(version_filter).await
    }

    async fn fetch_release_channels(
        &self,
        version_filter: &VersionFilter,
    ) -> anyhow::Result<Vec<ReleaseChannel>> {
        let index = self
            .client
            .get(RELEASES_INDEX_URL)
            .send()
            .await?
            .error_for_status()?
            .json::<ReleaseIndexDto>()
            .await?;

        let mut channels = Vec::new();

        for channel in index.releases_index {
            if !channel_matches_filter(&channel, version_filter) {
                continue;
            }

            let Ok(channel_version) = parse_channel_version(&channel.channel_version) else {
                continue;
            };

            channels.push(ReleaseChannel {
                channel_version,
                releases_json: channel.releases_json,
            });
        }

        channels.sort_by(|a, b| b.channel_version.cmp(&a.channel_version));
        Ok(channels)
    }

    async fn fetch_channel_release(&self, url: &str) -> anyhow::Result<ChannelReleaseDto> {
        Ok(self
            .client
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .json::<ChannelReleaseDto>()
            .await?)
    }
}

#[derive(Clone, Copy)]
enum Flavor {
    Sdk,
    Runtime,
    AspNetCoreRuntime,
    WindowsDesktopRuntime,
}

impl Flavor {
    fn parse(raw: Option<&str>) -> anyhow::Result<Self> {
        match raw.unwrap_or("sdk") {
            "sdk" => Ok(Self::Sdk),
            "runtime" => Ok(Self::Runtime),
            "aspnetcore_runtime" => Ok(Self::AspNetCoreRuntime),
            "windowsdesktop_runtime" => Ok(Self::WindowsDesktopRuntime),
            other => anyhow::bail!("Invalid dotnet flavor: {other}"),
        }
    }
}

#[derive(Debug, Deserialize)]
struct ReleaseIndexDto {
    #[serde(rename = "releases-index")]
    releases_index: Vec<ReleaseChannelDto>,
}

#[derive(Debug, Deserialize)]
struct ReleaseChannelDto {
    #[serde(rename = "channel-version")]
    channel_version: SmolStr,
    #[serde(rename = "release-type")]
    release_type: SmolStr,
    #[serde(rename = "releases.json")]
    releases_json: SmolStr,
    #[serde(rename = "support-phase")]
    support_phase: Option<SmolStr>,
}

struct ReleaseChannel {
    channel_version: (u32, u32),
    releases_json: SmolStr,
}

#[derive(Debug, Deserialize)]
struct ChannelReleaseDto {
    #[serde(rename = "release-type")]
    release_type: SmolStr,
    releases: Vec<ReleaseEntryDto>,
}

#[derive(Debug, Deserialize)]
struct ReleaseEntryDto {
    sdk: Option<ProductReleaseDto>,
    sdks: Option<Vec<ProductReleaseDto>>,
    runtime: Option<ProductReleaseDto>,
    #[serde(rename = "aspnetcore-runtime")]
    aspnetcore_runtime: Option<ProductReleaseDto>,
    windowsdesktop: Option<ProductReleaseDto>,
}

#[derive(Debug, Deserialize)]
struct ProductReleaseDto {
    version: SmolStr,
    files: Vec<ProductFileDto>,
}

#[derive(Debug, Deserialize)]
struct ProductFileDto {
    name: SmolStr,
    rid: Option<SmolStr>,
    url: SmolStr,
    hash: SmolStr,
}

impl ProductFileDto {
    fn matches(&self, rid: &str) -> bool {
        self.rid.as_deref() == Some(rid) && is_supported_archive(&self.name)
    }
}

struct DotnetReleaseAsset {
    version_raw: SmolStr,
    version: DotnetVersion,
    is_lts: bool,
    url: SmolStr,
    hash: SmolStr,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct DotnetVersion {
    major: u32,
    minor: u32,
    patch: u32,
    pre_release: Option<Vec<PreReleaseIdentifier>>,
}

impl DotnetVersion {
    fn is_prerelease(&self) -> bool {
        self.pre_release.is_some()
    }
}

impl Ord for DotnetVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| match (&self.pre_release, &other.pre_release) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(left), Some(right)) => compare_pre_release(left, right),
            })
    }
}

impl PartialOrd for DotnetVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
enum PreReleaseIdentifier {
    Numeric(u32),
    Text(SmolStr),
}

impl Ord for PreReleaseIdentifier {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Numeric(left), Self::Numeric(right)) => left.cmp(right),
            (Self::Numeric(_), Self::Text(_)) => Ordering::Less,
            (Self::Text(_), Self::Numeric(_)) => Ordering::Greater,
            (Self::Text(left), Self::Text(right)) => left.cmp(right),
        }
    }
}

impl PartialOrd for PreReleaseIdentifier {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn channel_matches_filter(channel: &ReleaseChannelDto, version_filter: &VersionFilter) -> bool {
    if version_filter.lts_only && channel.release_type != "lts" {
        return false;
    }

    if !version_filter.allow_prerelease
        && channel.support_phase.as_deref() == Some("preview")
        && version_filter.exact_version.is_none()
    {
        return false;
    }

    let (major, minor) = match parse_channel_version(&channel.channel_version) {
        Ok(parts) => parts,
        Err(err) => {
            log::warn!(
                "Failed to parse .NET channel version '{}': {}",
                channel.channel_version,
                err
            );
            return false;
        }
    };

    if let Some(prefix) = version_filter.version_prefix {
        if prefix.major != major {
            return false;
        }
        if prefix
            .minor
            .is_some_and(|prefix_minor| prefix_minor != minor)
        {
            return false;
        }
    }

    if let Some(exact_version) = &version_filter.exact_version {
        match parse_dotnet_version(exact_version) {
            Ok(version) => {
                if version.major != major || version.minor != minor {
                    return false;
                }
            }
            Err(err) => {
                log::warn!(
                    "Failed to parse exact .NET version '{}': {}",
                    exact_version,
                    err
                );
            }
        }
    }

    true
}

fn parse_channel_version(raw: &str) -> anyhow::Result<(u32, u32)> {
    let mut parts = raw.split('.');
    let major = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("Missing major version"))?
        .parse()?;
    let minor = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("Missing minor version"))?
        .parse()?;
    Ok((major, minor))
}

fn direct_release_channel(version_filter: &VersionFilter) -> Option<ReleaseChannel> {
    let (major, minor) = if let Some(exact_version) = &version_filter.exact_version {
        let version = parse_dotnet_version(exact_version).ok()?;
        (version.major, version.minor)
    } else {
        let prefix = version_filter.version_prefix?;
        (prefix.major, prefix.minor?)
    };

    Some(ReleaseChannel {
        channel_version: (major, minor),
        releases_json: smol_str::format_smolstr!(
            "https://builds.dotnet.microsoft.com/dotnet/release-metadata/{}.{}/releases.json",
            major,
            minor
        ),
    })
}

fn select_release_assets(
    channel_release: ChannelReleaseDto,
    rid: &str,
    flavor: Flavor,
    version_filter: &VersionFilter,
) -> Vec<DotnetReleaseAsset> {
    let mut assets = Vec::new();
    let channel_is_lts = channel_release.release_type == "lts";

    for release in channel_release.releases {
        let products = release_products(&release, flavor);
        for product in products {
            let parsed_version = match parse_dotnet_version(&product.version) {
                Ok(version) => version,
                Err(err) => {
                    log::warn!(
                        "Failed to parse .NET product version '{}': {}",
                        product.version,
                        err
                    );
                    continue;
                }
            };

            let is_lts = channel_is_lts && !parsed_version.is_prerelease();

            if !matches_version_filter(&product.version, &parsed_version, is_lts, version_filter) {
                continue;
            }

            let Some(file) = product.files.iter().find(|file| file.matches(rid)) else {
                continue;
            };

            assets.push(DotnetReleaseAsset {
                version_raw: product.version.clone(),
                version: parsed_version,
                is_lts,
                url: file.url.clone(),
                hash: file.hash.clone(),
            });
        }
    }

    assets
}

fn select_latest_release_asset(
    channel_release: ChannelReleaseDto,
    rid: &str,
    flavor: Flavor,
    version_filter: &VersionFilter,
) -> Option<DotnetReleaseAsset> {
    select_release_assets(channel_release, rid, flavor, version_filter)
        .into_iter()
        .max_by(|a, b| a.version.cmp(&b.version))
}

fn release_products(release: &ReleaseEntryDto, flavor: Flavor) -> Vec<&ProductReleaseDto> {
    match flavor {
        Flavor::Sdk => {
            if let Some(sdks) = &release.sdks {
                sdks.iter().collect()
            } else {
                release.sdk.iter().collect()
            }
        }
        Flavor::Runtime => release.runtime.iter().collect(),
        Flavor::AspNetCoreRuntime => release.aspnetcore_runtime.iter().collect(),
        Flavor::WindowsDesktopRuntime => release.windowsdesktop.iter().collect(),
    }
}

fn matches_version_filter(
    raw_version: &str,
    version: &DotnetVersion,
    is_lts: bool,
    version_filter: &VersionFilter,
) -> bool {
    if version_filter.lts_only && !is_lts {
        return false;
    }
    if !version_filter.allow_prerelease && version.is_prerelease() {
        return false;
    }
    if version_filter
        .version_prefix
        .is_some_and(|prefix| !prefix.matches(version.major, version.minor, version.patch))
    {
        return false;
    }
    if version_filter
        .exact_version
        .as_deref()
        .is_some_and(|exact| exact != raw_version)
    {
        return false;
    }
    true
}

fn is_supported_archive(name: &str) -> bool {
    name.ends_with(".zip") || name.ends_with(".tar.gz")
}

fn parse_dotnet_version(raw: &str) -> anyhow::Result<DotnetVersion> {
    let raw = raw
        .split_once('+')
        .map(|(without_build, _)| without_build)
        .unwrap_or(raw);
    let (core, pre_release) = raw
        .split_once('-')
        .map(|(core, pre)| (core, Some(pre)))
        .unwrap_or((raw, None));

    let mut parts = core.split('.');
    let major = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("Missing major version in '{raw}'"))?
        .parse::<u32>()?;
    let minor = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("Missing minor version in '{raw}'"))?
        .parse::<u32>()?;
    let patch = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("Missing patch version in '{raw}'"))?
        .parse::<u32>()?;

    if parts.next().is_some() {
        anyhow::bail!("Unexpected extra numeric version parts in '{raw}'");
    }

    let pre_release = pre_release.map(parse_pre_release).transpose()?;
    Ok(DotnetVersion {
        major,
        minor,
        patch,
        pre_release,
    })
}

fn parse_pre_release(raw: &str) -> anyhow::Result<Vec<PreReleaseIdentifier>> {
    let mut identifiers = Vec::new();
    for part in raw.split('.') {
        if part.is_empty() {
            anyhow::bail!("Invalid empty pre-release identifier in '{raw}'");
        }
        if let Ok(value) = part.parse::<u32>() {
            identifiers.push(PreReleaseIdentifier::Numeric(value));
        } else {
            identifiers.push(PreReleaseIdentifier::Text(SmolStr::new(part)));
        }
    }
    Ok(identifiers)
}

fn compare_pre_release(left: &[PreReleaseIdentifier], right: &[PreReleaseIdentifier]) -> Ordering {
    for (left_id, right_id) in left.iter().zip(right.iter()) {
        let cmp = left_id.cmp(right_id);
        if cmp != Ordering::Equal {
            return cmp;
        }
    }
    left.len().cmp(&right.len())
}

#[cfg(test)]
mod tests {
    use super::{
        matches_version_filter, parse_dotnet_version, select_latest_release_asset,
        ChannelReleaseDto, DotnetVersion, Flavor, PreReleaseIdentifier, ProductFileDto,
        ProductReleaseDto, ReleaseChannel, ReleaseEntryDto,
    };
    use crate::tool::VersionFilter;

    #[test]
    fn parse_stable_dotnet_version() {
        assert_eq!(
            parse_dotnet_version("8.0.418").unwrap(),
            DotnetVersion {
                major: 8,
                minor: 0,
                patch: 418,
                pre_release: None,
            }
        );
    }

    #[test]
    fn parse_preview_dotnet_version() {
        assert_eq!(
            parse_dotnet_version("11.0.100-preview.1.26104.118").unwrap(),
            DotnetVersion {
                major: 11,
                minor: 0,
                patch: 100,
                pre_release: Some(vec![
                    PreReleaseIdentifier::Text("preview".into()),
                    PreReleaseIdentifier::Numeric(1),
                    PreReleaseIdentifier::Numeric(26104),
                    PreReleaseIdentifier::Numeric(118),
                ]),
            }
        );
    }

    #[test]
    fn stable_is_newer_than_preview() {
        let preview = parse_dotnet_version("10.0.100-preview.7.1234.5").unwrap();
        let stable = parse_dotnet_version("10.0.100").unwrap();
        assert!(stable > preview);
    }

    #[test]
    fn prerelease_from_lts_channel_is_not_treated_as_lts() {
        let preview = parse_dotnet_version("10.0.100-preview.7.1234.5").unwrap();
        let stable = parse_dotnet_version("10.0.100").unwrap();
        let filter = VersionFilter {
            lts_only: true,
            allow_prerelease: true,
            version_prefix: None,
            exact_version: None,
        };

        assert!(!matches_version_filter(
            "10.0.100-preview.7.1234.5",
            &preview,
            false,
            &filter,
        ));
        assert!(matches_version_filter("10.0.100", &stable, true, &filter));
    }

    #[test]
    fn release_channels_are_sorted_from_high_to_low() {
        let mut channels = [
            ReleaseChannel {
                channel_version: (8, 0),
                releases_json: "8.0".into(),
            },
            ReleaseChannel {
                channel_version: (10, 0),
                releases_json: "10.0".into(),
            },
            ReleaseChannel {
                channel_version: (9, 0),
                releases_json: "9.0".into(),
            },
        ];

        channels.sort_by(|a, b| b.channel_version.cmp(&a.channel_version));

        assert_eq!(channels[0].channel_version, (10, 0));
        assert_eq!(channels[1].channel_version, (9, 0));
        assert_eq!(channels[2].channel_version, (8, 0));
    }

    #[test]
    fn select_latest_release_asset_returns_highest_match_in_channel() {
        let filter = VersionFilter {
            lts_only: false,
            allow_prerelease: false,
            version_prefix: None,
            exact_version: None,
        };
        let channel_release = ChannelReleaseDto {
            release_type: "sts".into(),
            releases: vec![
                ReleaseEntryDto {
                    sdk: Some(ProductReleaseDto {
                        version: "9.0.100".into(),
                        files: vec![ProductFileDto {
                            name: "dotnet-sdk-win-x64.zip".into(),
                            rid: Some("win-x64".into()),
                            url: "https://example.invalid/9.0.100.zip".into(),
                            hash: "hash-100".into(),
                        }],
                    }),
                    sdks: None,
                    runtime: None,
                    aspnetcore_runtime: None,
                    windowsdesktop: None,
                },
                ReleaseEntryDto {
                    sdk: Some(ProductReleaseDto {
                        version: "9.0.101".into(),
                        files: vec![ProductFileDto {
                            name: "dotnet-sdk-win-x64.zip".into(),
                            rid: Some("win-x64".into()),
                            url: "https://example.invalid/9.0.101.zip".into(),
                            hash: "hash-101".into(),
                        }],
                    }),
                    sdks: None,
                    runtime: None,
                    aspnetcore_runtime: None,
                    windowsdesktop: None,
                },
            ],
        };

        let latest = select_latest_release_asset(channel_release, "win-x64", Flavor::Sdk, &filter)
            .expect("expected a matching release");

        assert_eq!(latest.version_raw, "9.0.101");
        assert_eq!(latest.url, "https://example.invalid/9.0.101.zip");
    }
}
