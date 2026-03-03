pub mod dirln;
pub mod general_tool;
pub mod global;

use any_version_manager::{DefaultPlatform, HttpClient, UrlMirror};
use clap::{Parser, Subcommand};
use directories::ProjectDirs;
use log::LevelFilter;
use std::path::PathBuf;
use std::sync::Arc;

pub const CONFIG_PATH_ENV: &str = "CONFIG_PATH";

#[derive(Debug, Parser)]
#[command(
    name = "avm",
    version,
    about = "A CLI tool designed to manage multiple versions of multiple development tools for multiple programming languages with shared workflows.",
    after_long_help = "Use `avm tool` to list supported tools and `avm tool <tool>` to see platform/flavor options and install examples.",
    subcommand_required = true,
    arg_required_else_help = true
)]
pub struct Cli {
    #[arg(long, global = true, action = clap::ArgAction::SetTrue, help = "Enable debug logs")]
    pub debug: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    #[command(about = "Get the path of the config file")]
    ConfigPath,

    #[command(about = "List tools, or show tool-specific install guidance")]
    Tool(global::ToolGuideArgs),

    #[command(about = "Install a specific tool")]
    Install(general_tool::InstallArgs),

    #[command(about = "Get available versions")]
    GetVers(general_tool::GetVersArgs),

    #[command(about = "Get download info")]
    GetDowninfo(general_tool::GetDowninfoArgs),

    #[command(about = "Install a specific tool from a local archive")]
    InstallLocal(general_tool::InstallLocalArgs),

    #[command(about = "List existing tags")]
    List(general_tool::ListArgs),

    #[command(about = "Get the tool path of a specific tag")]
    Path(general_tool::PathArgs),

    #[command(about = "Get the path of the executable program")]
    ExePath(general_tool::ExePathArgs),

    #[command(about = "Run by tag, selector, or default tag")]
    Run(general_tool::RunArgs),

    #[command(about = "Create a tag alias")]
    Alias(general_tool::AliasArgs),

    #[command(about = "Copy an existing tag to a new tag")]
    Copy(general_tool::CopyArgs),

    #[command(about = "Remove existing tags")]
    Remove(general_tool::RemoveArgs),

    #[command(about = "Clean temporary directories and dangling aliases")]
    Clean(general_tool::CleanArgs),

    #[command(
        about = "Create a directory symbolic link (equivalent ln -s for Unix, mklink /J for Windows)",
        long_about = "Creates a directory symbolic link. This is equivalent to 'ln -s' on Unix systems and 'mklink /J' on Windows. This command is a utility and not directly tied to core avm flows."
    )]
    Dirln(dirln::DirlnArgs),
}

pub struct LoadedConfig {
    pub mirrors: UrlMirror,
    pub paths: Paths,
    pub default_platform: DefaultPlatform,
}

#[allow(dead_code)]
pub struct Paths {
    pub config_file: PathBuf,
    pub data_dir: PathBuf,
    pub tool_dir: PathBuf,
}

pub async fn run(
    paths: Paths,
    client: Arc<HttpClient>,
    default_platform: DefaultPlatform,
) -> anyhow::Result<()> {
    let cli = Cli::parse();
    if !cli.debug {
        log::set_max_level(LevelFilter::Info);
    }

    let tools = general_tool::ToolSet::new(client.clone(), &default_platform);

    match cli.command {
        Command::ConfigPath => {
            println!("{}", paths.config_file.display());
            Ok(())
        }
        Command::Tool(args) => {
            global::run_tool_guide(args, &tools);
            Ok(())
        }
        Command::Install(args) => general_tool::run_install(args, &tools, &client, &paths).await,
        Command::GetVers(args) => general_tool::run_get_vers(args, &tools).await,
        Command::GetDowninfo(args) => general_tool::run_get_downinfo(args, &tools).await,
        Command::InstallLocal(args) => general_tool::run_install_local(args, &paths).await,
        Command::List(args) => general_tool::run_list(args, &paths).await,
        Command::Path(args) => general_tool::run_path(args, &paths),
        Command::ExePath(args) => general_tool::run_exe_path(args, &tools, &paths),
        Command::Run(args) => general_tool::run_run(args, &tools, &client, &paths).await,
        Command::Alias(args) => general_tool::run_alias(args, &paths).await,
        Command::Copy(args) => general_tool::run_copy(args, &paths).await,
        Command::Remove(args) => general_tool::run_remove(args, &paths).await,
        Command::Clean(args) => general_tool::run_clean(args, &paths).await,
        Command::Dirln(args) => dirln::run(args).await,
    }
}

pub fn load_config() -> anyhow::Result<LoadedConfig> {
    let dirs =
        ProjectDirs::from("", "", "avm").ok_or_else(|| anyhow::anyhow!("No home directory"))?;

    let config_path = match std::env::var_os(CONFIG_PATH_ENV) {
        Some(path) => path.into(),
        None => dirs.config_dir().join("config.toml"),
    };

    let config: any_version_manager::Config = match std::fs::read_to_string(&config_path) {
        Ok(config_str) => toml::from_str(&config_str)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            any_version_manager::Config::default()
        }
        Err(e) => return Err(e.into()),
    };

    let data_path = config
        .data_path
        .unwrap_or_else(|| dirs.data_local_dir().to_path_buf());
    let tool_path = data_path.join("tools");

    Ok(LoadedConfig {
        mirrors: config.mirrors.unwrap_or_default(),
        paths: Paths {
            config_file: config_path,
            data_dir: data_path,
            tool_dir: tool_path,
        },
        default_platform: config.default_platform.unwrap_or_default(),
    })
}
