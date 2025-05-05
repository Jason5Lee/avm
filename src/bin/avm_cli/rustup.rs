use crate::HttpClient;

use super::{AvmSubcommand, Paths};
use async_trait::async_trait;
use clap::Arg;
use std::{path::PathBuf, sync::Arc};

pub fn new_subcommand() -> AvmSubcommand {
    AvmSubcommand {
        cmd: clap::Command::new("rustup")
            .about("Rustup (Rust Toolchain Manager) delegate")
            .arg(
                Arg::new("args")
                    .help("Arguments to pass to rustup")
                    .last(true)
                    .num_args(0..)
                    .value_parser(clap::value_parser!(std::ffi::OsString)),
            ),
        run_command: Box::new(RunRustupCommand {}),
    }
}

struct RunRustupCommand {}

#[async_trait]
impl super::RunSubcommand for RunRustupCommand {
    async fn run(
        &self,
        paths: Paths,
        _client: Arc<HttpClient>,
        args: &clap::ArgMatches,
    ) -> anyhow::Result<()> {
        let rustup_path = paths
            .rustup_path
            .unwrap_or_else(|| match std::env::var("RUSTUP_PATH") {
                Ok(path) => PathBuf::from(path),
                Err(_) => PathBuf::from("rustup"),
            });

        let mut command = std::process::Command::new(rustup_path);
        if let Some(args) = args.get_many::<std::ffi::OsString>("args") {
            command.args(args);
        }
        any_version_manager::spawn_blocking(move || {
            command.spawn()?.wait()?;
            Ok(())
        })
        .await
    }
}
