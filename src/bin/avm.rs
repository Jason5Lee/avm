mod avm_cli;

use any_version_manager::tool::general_tool::{go, liberica, node};
use any_version_manager::HttpClient;
use anyhow::Context;
use avm_cli::{load_config, rustup, AvmApp, LoadedConfig};
use log::LevelFilter;
use std::sync::Arc;

fn main() {
    stderrlog::new()
        .verbosity(LevelFilter::Trace)
        .init()
        .expect("Failed to initialize logger");

    let r = (|| -> anyhow::Result<()> {
        let LoadedConfig { mirror, paths } = load_config()?;
        ctrlc::set_handler(move || {
            any_version_manager::set_cancelled();
        })
        .context("Error setting Ctrl-C handler")?;

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let http_client = Arc::new(HttpClient::new(mirror));
        runtime
            .block_on(any_version_manager::CancellableFuture::new(
                AvmApp::new()
                    .add_tool(liberica::Tool::new(http_client.clone()))
                    .add_tool(go::Tool::new(http_client.clone()))
                    .add_tool(node::Tool::new(http_client.clone()))
                    .add_subcommand(rustup::new_subcommand())
                    .run(paths, http_client),
            ))
            .unwrap_or(Ok(()))
    })();

    if let Err(e) = r {
        log::error!("{e:?}");
    }
}
