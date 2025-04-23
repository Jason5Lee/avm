use crate::tool::GeneralTool;
use crate::{HttpClient, UrlMirror};
use directories::ProjectDirs;
use log::LevelFilter;
use smol_str::SmolStr;
use std::collections::HashMap;
use std::fs::File;
use std::path::PathBuf;

pub const CONFIG_PATH_ENV: &str = "CONFIG_PATH";

pub struct AvmApp {
    cmd: clap::Command,
    tools: HashMap<SmolStr, Box<dyn GeneralTool + Send + Sync + 'static>>,
}

pub struct LoadedConfig {
    pub mirror: UrlMirror,
    pub paths: Paths,
}

pub struct Paths {
    pub config_file: PathBuf,
    pub data_dir: PathBuf,
    pub tool_dir: PathBuf,
}

impl AvmApp {
    pub const CONFIG_PATH_CMD: &str = "config-path";
    pub const DEFAULT_TAG: &str = "default";

    pub fn new() -> Self {
        Self {
            cmd: clap::Command::new("avm")
                .about("Any Version Manager - manage several versions of the development tools for potentially any programming language")
                .version("0.1.0")
                .subcommand_required(true)
                .arg_required_else_help(true)
                .arg(clap::Arg::new("debug").long("debug").action(clap::ArgAction::SetTrue))
                .subcommand(clap::Command::new(Self::CONFIG_PATH_CMD)
                    .about("Get the path of the config file")),
            tools: HashMap::new(),
        }
    }

    pub fn add_tool<T: GeneralTool + Send + Sync + 'static>(self, tool: T) -> Self {
        let Self { mut cmd, mut tools } = self;
        let info = tool.info();
        cmd = cmd.subcommand(tool::command(info));
        tools.insert(info.name.clone(), Box::new(tool));
        Self { cmd, tools }
    }

    pub async fn run(self, paths: Paths, client: &HttpClient) -> anyhow::Result<()> {
        let matches = self.cmd.get_matches();
        if !matches.get_flag("debug") {
            log::set_max_level(LevelFilter::Info);
        }

        if let Some((subcmd, args)) = matches.subcommand() {
            if subcmd == Self::CONFIG_PATH_CMD {
                println!("{}", paths.config_file.display());
            } else {
                let tool = self
                    .tools
                    .get(subcmd)
                    .ok_or_else(|| anyhow::anyhow!("Unknown tool {}", subcmd))?;
                tool::run(tool.as_ref(), client, &paths, args).await?;
            }
        }

        Ok(())
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

    let config: crate::Config = match File::open(&config_path) {
        Ok(file) => serde_yaml_ng::from_reader(file)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Use default config when file is not found
            crate::Config::default()
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
        },
    })
}

pub mod tool;
