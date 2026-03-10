use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::avm_cli::Paths;
use crate::HttpClient;
use any_version_manager::tool::general_tool::{
    self, dotnet as dotnet_tool, go as go_tool, liberica as liberica_tool, node as node_tool,
    pnpm as pnpm_tool,
};
use any_version_manager::tool::{GeneralTool, ToolInfo, Version, VersionFilter, VersionPrefix};
use any_version_manager::DefaultPlatform;
use clap::{Args, ValueEnum};
use indicatif::{ProgressBar, ProgressStyle};
use smol_str::SmolStr;

#[derive(Debug, Clone, Copy, Eq, PartialEq, ValueEnum)]
pub enum ToolName {
    Dotnet,
    Liberica,
    Go,
    Node,
    Pnpm,
}

impl ToolName {
    pub fn command_name(self) -> String {
        self.to_possible_value()
            .expect("ToolName variants always map to clap values")
            .get_name()
            .to_owned()
    }
}

pub struct ToolSet {
    pub dotnet: dotnet_tool::Tool,
    pub liberica: liberica_tool::Tool,
    pub go: go_tool::Tool,
    pub node: node_tool::Tool,
    pub pnpm: pnpm_tool::Tool,
}

pub trait FnTool {
    type Output;

    fn invoke(&self, tool: &impl GeneralTool) -> Self::Output;
}

trait AsyncFnTool {
    type Output;

    async fn invoke(&self, tool: &impl GeneralTool) -> Self::Output;
}

fn invoke_tool<FT: FnTool>(tool_set: &ToolSet, tool_name: ToolName, fn_tool: &FT) -> FT::Output {
    match tool_name {
        ToolName::Dotnet => fn_tool.invoke(&tool_set.dotnet),
        ToolName::Liberica => fn_tool.invoke(&tool_set.liberica),
        ToolName::Go => fn_tool.invoke(&tool_set.go),
        ToolName::Node => fn_tool.invoke(&tool_set.node),
        ToolName::Pnpm => fn_tool.invoke(&tool_set.pnpm),
    }
}

async fn async_invoke_tool<FT: AsyncFnTool>(
    tool_set: &ToolSet,
    tool_name: ToolName,
    fn_tool: &FT,
) -> FT::Output {
    match tool_name {
        ToolName::Dotnet => fn_tool.invoke(&tool_set.dotnet).await,
        ToolName::Liberica => fn_tool.invoke(&tool_set.liberica).await,
        ToolName::Go => fn_tool.invoke(&tool_set.go).await,
        ToolName::Node => fn_tool.invoke(&tool_set.node).await,
        ToolName::Pnpm => fn_tool.invoke(&tool_set.pnpm).await,
    }
}

impl ToolSet {
    pub fn new(client: Arc<HttpClient>, default_platform: &DefaultPlatform) -> Self {
        let resolve = |tool_name: &str| -> Option<SmolStr> {
            default_platform
                .tools
                .get(tool_name)
                .or(default_platform.global.as_ref())
                .map(SmolStr::new)
        };
        Self {
            dotnet: dotnet_tool::Tool::new(client.clone(), resolve("dotnet")),
            liberica: liberica_tool::Tool::new(client.clone(), resolve("liberica")),
            go: go_tool::Tool::new(client.clone(), resolve("go")),
            node: node_tool::Tool::new(client.clone(), resolve("node")),
            pnpm: pnpm_tool::Tool::new(client),
        }
    }

    pub fn tool_info(&self, tool: ToolName) -> &ToolInfo {
        match tool {
            ToolName::Dotnet => self.dotnet.info(),
            ToolName::Liberica => self.liberica.info(),
            ToolName::Go => self.go.info(),
            ToolName::Node => self.node.info(),
            ToolName::Pnpm => self.pnpm.info(),
        }
    }

    pub fn all_infos(&self) -> [(String, &ToolInfo); 5] {
        [
            (ToolName::Go.command_name(), self.tool_info(ToolName::Go)),
            (
                ToolName::Liberica.command_name(),
                self.tool_info(ToolName::Liberica),
            ),
            (
                ToolName::Node.command_name(),
                self.tool_info(ToolName::Node),
            ),
            (
                ToolName::Pnpm.command_name(),
                self.tool_info(ToolName::Pnpm),
            ),
            (
                ToolName::Dotnet.command_name(),
                self.tool_info(ToolName::Dotnet),
            ),
        ]
    }

    pub fn describe_flavor(&self, tool: ToolName, flavor: &str) -> &'static str {
        invoke_tool(self, tool, &DescribeFlavorFn { flavor })
    }
}

struct DescribeFlavorFn<'a> {
    flavor: &'a str,
}

impl FnTool for DescribeFlavorFn<'_> {
    type Output = &'static str;

    fn invoke(&self, tool: &impl GeneralTool) -> Self::Output {
        tool.describe_flavor(self.flavor)
    }
}

#[derive(Debug, Clone, Args)]
pub struct SelectorArgs {
    #[arg(
        short = 'v',
        long = "version",
        help = "Exact version string, for example 22.13.1."
    )]
    pub version: Option<String>,
    #[arg(
        short = 'x',
        long = "verpfx",
        help = "Version prefix in strict x, x.y, or x.y.z format."
    )]
    pub version_prefix: Option<String>,
    #[arg(
        short = 'p',
        long,
        help = "Target platform identifier. Defaults to the avm binary's compile-target platform unless overridden by config."
    )]
    pub platform: Option<String>,
    #[arg(short = 'f', long, help = "Tool-specific flavor identifier.")]
    pub flavor: Option<String>,
    #[arg(long = "lts-only", help = "Only allow LTS releases.")]
    pub lts_only: bool,
    #[arg(long = "allow-prere", help = "Allow prerelease versions (beta/rc).")]
    pub allow_prerelease: bool,
}

impl SelectorArgs {
    fn is_empty(&self) -> bool {
        self.version.is_none()
            && self.version_prefix.is_none()
            && self.platform.is_none()
            && self.flavor.is_none()
            && !self.lts_only
            && !self.allow_prerelease
    }
}

fn resolve_selector_filters(
    tool: &impl GeneralTool,
    selector: &SelectorArgs,
) -> anyhow::Result<(Option<SmolStr>, Option<SmolStr>, VersionFilter)> {
    let (platform, flavor) = resolve_platform_flavor(tool, &selector.platform, &selector.flavor);
    let version_filter = to_version_filter(
        selector.version.as_deref(),
        selector.version_prefix.as_deref(),
        selector.lts_only,
        selector.allow_prerelease,
    )?;
    Ok((platform, flavor, version_filter))
}

#[derive(Debug, Clone, Args)]
pub struct InstallArgs {
    #[arg(
        value_enum,
        help = "Tool name. Use `avm tool <tool>` to inspect supported platform/flavor values."
    )]
    pub tool: ToolName,
    #[clap(flatten)]
    pub selector: SelectorArgs,
    #[arg(long, help = "Set installed version as the `default` alias.")]
    pub default: bool,
    #[arg(short = 'u', long, help = "Replace existing tag if already installed.")]
    pub update: bool,
}

#[derive(Debug, Clone, Args)]
pub struct GetVersArgs {
    #[arg(value_enum, help = "Tool name.")]
    pub tool: ToolName,
    #[clap(flatten)]
    pub selector: SelectorArgs,
}

#[derive(Debug, Clone, Args)]
pub struct GetDowninfoArgs {
    #[arg(value_enum, help = "Tool name.")]
    pub tool: ToolName,
    #[clap(flatten)]
    pub selector: SelectorArgs,
}

#[derive(Debug, Clone, Args)]
pub struct InstallLocalArgs {
    #[arg(value_enum, help = "Tool name.")]
    pub tool: ToolName,
    #[arg(value_name = "archive", help = "Path to the local archive file.")]
    pub archive: PathBuf,
    #[arg(value_name = "target_tag", help = "Tag to install as.")]
    pub target_tag: String,
    #[arg(long, value_name = "version", help = "Tool's version.")]
    pub version: String,
    #[arg(long, help = "If tool's version is LTS.")]
    pub lts: bool,
    #[arg(
        long,
        value_name = "hash",
        help = "Archive hash in TOML inline table format, for example `{ sha256 = \"...\" }`, `{ sha512 = \"...\" }`, or `{ sha1 = \"...\" }`."
    )]
    pub hash: Option<String>,
    #[arg(long, help = "Replace existing tag if already installed.")]
    pub update: bool,
    #[arg(long, help = "Set installed version as the `default` alias.")]
    pub default: bool,
}

#[derive(Debug, Clone, Args)]
pub struct ListArgs {
    #[arg(value_enum, help = "Tool name.")]
    pub tool: ToolName,
}

#[derive(Debug, Clone, Args)]
pub struct PathArgs {
    #[arg(value_enum, help = "Tool name.")]
    pub tool: ToolName,
    #[arg(
        help = "Tag to resolve. Defaults to `default`.",
        default_value = "default"
    )]
    pub tag: String,
}

#[derive(Debug, Clone, Args)]
pub struct EntryPathArgs {
    #[arg(value_enum, help = "Tool name.")]
    pub tool: ToolName,
    #[arg(
        help = "Tag to resolve. Defaults to `default`.",
        default_value = "default"
    )]
    pub tag: String,
}

#[derive(Debug, Clone, Args)]
pub struct RunArgs {
    #[arg(value_enum, help = "Tool name.")]
    pub tool: ToolName,
    #[arg(
        short = 't',
        long = "tag",
        help = "Tag to run. If set together with selector flags, selector filters are ignored."
    )]
    pub tag: Option<String>,
    #[clap(flatten)]
    pub selector: SelectorArgs,
    #[arg(
        help = "Arguments passed to the tool executable. Use `--` before these arguments.",
        last = true,
        allow_hyphen_values = true
    )]
    pub args: Vec<OsString>,
}

#[derive(Debug, Clone, Args)]
pub struct AliasArgs {
    #[arg(value_enum, help = "Tool name.")]
    pub tool: ToolName,
    #[arg(value_name = "src_tag", help = "Source tag.")]
    pub src_tag: String,
    #[arg(value_name = "alias_tag", help = "Alias tag to create.")]
    pub alias_tag: String,
}

#[derive(Debug, Clone, Args)]
pub struct CopyArgs {
    #[arg(value_enum, help = "Tool name.")]
    pub tool: ToolName,
    #[arg(value_name = "src_tag", help = "Source tag.")]
    pub src_tag: String,
    #[arg(value_name = "target_tag", help = "Target tag.")]
    pub target_tag: String,
}

#[derive(Debug, Clone, Args)]
pub struct RemoveArgs {
    #[arg(value_enum, help = "Tool name.")]
    pub tool: ToolName,
    #[arg(value_name = "tag", required = true, num_args = 1.., help = "Tag(s) to remove.")]
    pub tags: Vec<String>,
    #[arg(
        long,
        help = "Allow deleting an alias target and leaving dangling aliases."
    )]
    pub allow_dangling: bool,
}

#[derive(Debug, Clone, Args)]
pub struct CleanArgs {
    #[arg(value_enum, help = "Tool name.")]
    pub tool: ToolName,
}

struct RunInstallFn<'a> {
    tool_name: &'a str,
    client: &'a HttpClient,
    tools_base: &'a Path,
    args: &'a InstallArgs,
}

impl AsyncFnTool for RunInstallFn<'_> {
    type Output = anyhow::Result<()>;

    async fn invoke(&self, tool: &impl GeneralTool) -> Self::Output {
        let tool_name = self.tool_name;
        let client = self.client;
        let tools_base = self.tools_base;
        let args = self.args;

        let (platform, flavor, install_version) = resolve_selector_filters(tool, &args.selector)?;

        let (target_tag, download_url, download_state) = general_tool::InstallArgs {
            tool_name,
            tool,
            client,
            tools_base,
            platform,
            flavor,
            install_version,
            update: args.update,
            default: args.default,
        }
        .install()
        .await?;

        drive_download_state(target_tag, download_url, download_state).await?;

        Ok(())
    }
}

struct RunGetVersFn<'a> {
    args: &'a GetVersArgs,
}

impl AsyncFnTool for RunGetVersFn<'_> {
    type Output = anyhow::Result<()>;

    async fn invoke(&self, tool: &impl GeneralTool) -> Self::Output {
        let args = self.args;
        let (platform, flavor, version_filter) = resolve_selector_filters(tool, &args.selector)?;

        let vers = general_tool::get_vers(tool, platform, flavor, version_filter).await?;
        for v in vers {
            println!("{}{}", v.version, if v.is_lts { " [LTS]" } else { "" });
        }

        Ok(())
    }
}

struct RunGetDowninfoFn<'a> {
    args: &'a GetDowninfoArgs,
}

impl AsyncFnTool for RunGetDowninfoFn<'_> {
    type Output = anyhow::Result<()>;

    async fn invoke(&self, tool: &impl GeneralTool) -> Self::Output {
        let args = self.args;
        let (platform, flavor, install_version) = resolve_selector_filters(tool, &args.selector)?;

        let downinfo = general_tool::get_downinfo(tool, platform, flavor, install_version).await?;
        println!("{}", toml::to_string(&downinfo)?);
        Ok(())
    }
}

struct RunEntryPathFn<'a> {
    tool_name: &'a str,
    tools_base: &'a Path,
    args: &'a EntryPathArgs,
}

impl FnTool for RunEntryPathFn<'_> {
    type Output = anyhow::Result<()>;

    fn invoke(&self, tool: &impl GeneralTool) -> Self::Output {
        let path =
            general_tool::get_entry_path(self.tool_name, tool, self.tools_base, &self.args.tag)?;
        println!("{}", path.display());
        Ok(())
    }
}

struct RunRunFn<'a> {
    tool_name: &'a str,
    client: &'a HttpClient,
    tools_base: &'a Path,
    args: &'a RunArgs,
}

impl AsyncFnTool for RunRunFn<'_> {
    type Output = anyhow::Result<()>;

    async fn invoke(&self, tool: &impl GeneralTool) -> Self::Output {
        let tool_name = self.tool_name;
        let client = self.client;
        let tools_base = self.tools_base;
        let args = self.args;

        let tag = if let Some(tag) = args.tag.as_ref() {
            if !args.selector.is_empty() {
                log::warn!("Selector flags are ignored because `--tag` is provided.");
            }
            SmolStr::from(tag.as_str())
        } else if !args.selector.is_empty() {
            let (platform, flavor, version_filter) =
                resolve_selector_filters(tool, &args.selector)?;

            if let Some(local_tag) = general_tool::find_matching_local_tag(
                tool_name,
                tool,
                tools_base,
                platform.clone(),
                flavor.clone(),
                version_filter.clone(),
            )
            .await?
            {
                local_tag
            } else {
                let (target_tag, download_url, download_state) = general_tool::InstallArgs {
                    tool_name,
                    tool,
                    client,
                    tools_base,
                    platform,
                    flavor,
                    install_version: version_filter,
                    update: false,
                    default: false,
                }
                .install()
                .await?;
                drive_download_state(target_tag.clone(), download_url, download_state).await?;
                target_tag
            }
        } else {
            SmolStr::new("default")
        };

        let entry_path = general_tool::get_entry_path(tool_name, tool, tools_base, &tag)?;
        tool.run(entry_path, args.args.clone()).await
    }
}

pub async fn run_install(
    args: InstallArgs,
    tools: &ToolSet,
    client: &HttpClient,
    paths: &Paths,
) -> anyhow::Result<()> {
    let tool_name = args.tool.command_name();
    let fn_tool = RunInstallFn {
        tool_name: &tool_name,
        client,
        tools_base: &paths.tool_dir,
        args: &args,
    };
    async_invoke_tool(tools, args.tool, &fn_tool).await
}

pub async fn run_get_vers(args: GetVersArgs, tools: &ToolSet) -> anyhow::Result<()> {
    let fn_tool = RunGetVersFn { args: &args };
    async_invoke_tool(tools, args.tool, &fn_tool).await
}

pub async fn run_get_downinfo(args: GetDowninfoArgs, tools: &ToolSet) -> anyhow::Result<()> {
    let fn_tool = RunGetDowninfoFn { args: &args };
    async_invoke_tool(tools, args.tool, &fn_tool).await
}

pub async fn run_install_local(args: InstallLocalArgs, paths: &Paths) -> anyhow::Result<()> {
    let tool_name = args.tool.command_name();
    general_tool::LocalInstaller {
        tool_name: &tool_name,
        tools_base: &paths.tool_dir,
        archive: args.archive,
        target_tag: &args.target_tag,
        version: Version {
            version: args.version.into(),
            is_lts: args.lts,
        },
        hash: args.hash.as_deref(),
        update: args.update,
        default: args.default,
    }
    .install()
    .await
}

pub async fn run_list(args: ListArgs, paths: &Paths) -> anyhow::Result<()> {
    let tool_name = args.tool.command_name();
    for (tag, target) in general_tool::list_tags(&tool_name, &paths.tool_dir).await? {
        print!("{}", tag);
        if let Some(target) = target {
            print!(" -> {}", target);
        }
        println!();
    }
    Ok(())
}

pub fn run_path(args: PathArgs, paths: &Paths) -> anyhow::Result<()> {
    let tool_name = args.tool.command_name();
    let path = general_tool::get_tag_path(&tool_name, &paths.tool_dir, &args.tag)?;
    println!("{}", path.display());
    Ok(())
}

pub fn run_entry_path(args: EntryPathArgs, tools: &ToolSet, paths: &Paths) -> anyhow::Result<()> {
    let tool_name = args.tool.command_name();
    let fn_tool = RunEntryPathFn {
        tool_name: &tool_name,
        tools_base: &paths.tool_dir,
        args: &args,
    };
    invoke_tool(tools, args.tool, &fn_tool)
}

pub async fn run_run(
    args: RunArgs,
    tools: &ToolSet,
    client: &HttpClient,
    paths: &Paths,
) -> anyhow::Result<()> {
    let tool_name = args.tool.command_name();
    let fn_tool = RunRunFn {
        tool_name: &tool_name,
        client,
        tools_base: &paths.tool_dir,
        args: &args,
    };
    async_invoke_tool(tools, args.tool, &fn_tool).await
}

pub async fn run_alias(args: AliasArgs, paths: &Paths) -> anyhow::Result<()> {
    let tool_name = args.tool.command_name();
    general_tool::create_alias_tag(
        &tool_name,
        &paths.tool_dir,
        args.src_tag.into(),
        args.alias_tag.into(),
    )
    .await
}

pub async fn run_copy(args: CopyArgs, paths: &Paths) -> anyhow::Result<()> {
    let tool_name = args.tool.command_name();
    general_tool::copy_tag(
        &tool_name,
        &paths.tool_dir,
        args.src_tag.into(),
        args.target_tag.into(),
    )
    .await
}

pub async fn run_remove(args: RemoveArgs, paths: &Paths) -> anyhow::Result<()> {
    let tool_name = args.tool.command_name();
    let tags_to_remove = args.tags.into_iter().map(SmolStr::from).collect::<Vec<_>>();
    general_tool::remove_tag(
        &tool_name,
        &paths.tool_dir,
        tags_to_remove,
        args.allow_dangling,
    )
    .await
}

pub async fn run_clean(args: CleanArgs, paths: &Paths) -> anyhow::Result<()> {
    let tool_name = args.tool.command_name();
    general_tool::clean(&tool_name, &paths.tool_dir).await
}

pub fn to_version_filter(
    version: Option<&str>,
    version_prefix: Option<&str>,
    lts: bool,
    allow_prerelease: bool,
) -> anyhow::Result<VersionFilter> {
    Ok(VersionFilter {
        exact_version: version.map(SmolStr::from),
        version_prefix: version_prefix.map(VersionPrefix::parse).transpose()?,
        lts_only: lts,
        allow_prerelease,
    })
}

async fn drive_download_state(
    target_tag: SmolStr,
    download_url: SmolStr,
    mut download_state: any_version_manager::io::DownloadExtractState,
) -> anyhow::Result<()> {
    log::info!("Will download from {download_url}");
    log::info!("\"{target_tag}\" will be installed");
    let mut prev_name: Option<SmolStr> = None;
    let mut pb: Option<ProgressBar> = None;

    #[allow(clippy::while_let_loop)]
    loop {
        match download_state.status() {
            any_version_manager::Status::InProgress {
                name,
                progress_ratio,
            } => {
                if prev_name.as_ref() != Some(&name) {
                    if let Some(pb) = pb.take() {
                        pb.finish_with_message("Completed.");
                    }

                    log::info!("{name} ...");
                    prev_name = Some(name);
                }

                if let Some(progress_ratio) = progress_ratio {
                    if let Some(pb) = &mut pb {
                        pb.set_position(progress_ratio.0);
                    } else {
                        let new_pb = ProgressBar::new(progress_ratio.1);
                        new_pb.set_style(ProgressStyle::default_bar().template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")?.progress_chars("#>-"));
                        new_pb.set_position(progress_ratio.0);
                        pb = Some(new_pb);
                    }
                }
            }
            any_version_manager::Status::Stopped => {
                break;
            }
        }

        download_state = download_state.advance().await?;
    }

    Ok(())
}

pub fn option_to_smol_str(value: &Option<String>) -> Option<SmolStr> {
    value.as_deref().map(SmolStr::from)
}

pub fn resolve_platform_flavor(
    tool: &impl GeneralTool,
    platform: &Option<String>,
    flavor: &Option<String>,
) -> (Option<SmolStr>, Option<SmolStr>) {
    let info = tool.info();

    let platform = option_to_smol_str(platform).or_else(|| info.default_platform.clone());
    let flavor = option_to_smol_str(flavor).or_else(|| info.default_flavor.clone());

    (platform, flavor)
}
