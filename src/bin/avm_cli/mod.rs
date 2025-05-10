pub mod general_tool;
pub mod rustup;

use any_version_manager::tool::GeneralTool;
use any_version_manager::{HttpClient, UrlMirror};
use async_trait::async_trait;
use directories::ProjectDirs;
use fxhash::FxHashMap;
use log::LevelFilter;
use smol_str::SmolStr;
use std::fs::File;
use std::path::PathBuf;
use std::sync::Arc;

pub const CONFIG_PATH_ENV: &str = "CONFIG_PATH";

pub struct AvmSubcommand {
    cmd: clap::Command,
    run_command: Box<dyn RunSubcommand>,
}

// Cannot use `Fn` trait due to lifetime issues.
#[async_trait]
trait RunSubcommand: Send + Sync {
    async fn run(
        &self,
        paths: Paths,
        client: Arc<HttpClient>,
        args: &clap::ArgMatches,
    ) -> anyhow::Result<()>;
}

struct RunConfigPathSubcommand;

#[async_trait]
impl RunSubcommand for RunConfigPathSubcommand {
    async fn run(
        &self,
        paths: Paths,
        _client: Arc<HttpClient>,
        _args: &clap::ArgMatches,
    ) -> anyhow::Result<()> {
        println!("{}", paths.config_file.display());
        Ok(())
    }
}

pub struct AvmApp {
    cmd: clap::Command,
    run_commands: FxHashMap<SmolStr, Box<dyn RunSubcommand>>,
}

pub struct LoadedConfig {
    pub mirror: UrlMirror,
    pub paths: Paths,
}

#[allow(dead_code)] // some fields may be useful in the future.
pub struct Paths {
    pub config_file: PathBuf,
    pub data_dir: PathBuf,
    pub tool_dir: PathBuf,
    pub rustup_path: Option<PathBuf>,
}

impl AvmApp {
    pub const CONFIG_PATH_CMD: &str = "config-path";

    pub fn new() -> Self {
        let mut run_commands: FxHashMap<SmolStr, Box<dyn RunSubcommand>> = FxHashMap::default();
        run_commands.insert(
            Self::CONFIG_PATH_CMD.into(),
            Box::new(RunConfigPathSubcommand),
        );
        Self {
            cmd: clap::Command::new("avm")
                .about("(Potentially) Any language Version Manager, a Command-Line Interface tool designed to manage multiple versions of development tools for potentially any programming language, maximizing code reuse.")
                .version("0.0.2")
                .subcommand_required(true)
                .arg_required_else_help(true)
                .arg(clap::Arg::new("debug").long("debug").action(clap::ArgAction::SetTrue))
                .subcommand(clap::Command::new(Self::CONFIG_PATH_CMD)
                    .about("Get the path of the config file")),
            run_commands,
        }
    }

    pub fn add_subcommand(self, subcmd: AvmSubcommand) -> Self {
        let Self {
            mut cmd,
            mut run_commands,
        } = self;
        let name = subcmd.cmd.get_name().into();
        cmd = cmd.subcommand(subcmd.cmd);
        run_commands.insert(name, subcmd.run_command);
        Self { cmd, run_commands }
    }

    pub fn add_tool<T: GeneralTool + 'static>(self, tool: T) -> Self {
        self.add_subcommand(general_tool::new_avm_subcommand(tool))
    }

    pub async fn run(self, paths: Paths, client: Arc<HttpClient>) -> anyhow::Result<()> {
        let matches = self.cmd.get_matches();
        if !matches.get_flag("debug") {
            log::set_max_level(LevelFilter::Info);
        }

        let (subcmd, args) = matches.subcommand().expect("Subcommand is required");
        self.run_commands
            .get(subcmd)
            .expect("Subcommand should be present")
            .run(paths, client, args)
            .await
    }
}

impl Default for AvmApp {
    fn default() -> Self {
        Self::new()
    }
}

pub fn load_config() -> anyhow::Result<LoadedConfig> {
    let dirs =
        ProjectDirs::from("", "", "avm").ok_or_else(|| anyhow::anyhow!("No home directory"))?;

    let config_path = match std::env::var_os(CONFIG_PATH_ENV) {
        Some(path) => path.into(),
        None => dirs.config_dir().join("config.yaml"),
    };

    let config: any_version_manager::Config = match File::open(&config_path) {
        Ok(file) => serde_yaml_ng::from_reader(file)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Use default config when file is not found
            any_version_manager::Config::default()
        }
        Err(e) => return Err(e.into()),
    };

    let data_path = config
        .data_path
        .unwrap_or_else(|| dirs.data_local_dir().to_path_buf());
    let tool_path = data_path.join("tools");

    Ok(LoadedConfig {
        mirror: config.mirror.unwrap_or_default(),
        paths: Paths {
            config_file: config_path,
            data_dir: data_path,
            tool_dir: tool_path,
            rustup_path: config.rustup.and_then(|c| c.path),
        },
    })
}
