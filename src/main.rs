use anyhow::Context;
use avm::cli::{load_config, LoadedConfig};
use avm::tool::general_tool::go;
use avm::{cli::AvmApp, tool::general_tool::liberica, HttpClient};
use log::LevelFilter;
use std::sync::Arc;

fn main() {
    stderrlog::new()
        .module(module_path!())
        .verbosity(LevelFilter::Trace)
        .init()
        .expect("Failed to initialize logger");

    let r = (|| -> anyhow::Result<()> {
        let LoadedConfig { mirror, paths } = load_config()?;
        ctrlc::set_handler(move || {
            avm::set_cancelled();
        })
        .context("Error setting Ctrl-C handler")?;

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let http_client = Arc::new(HttpClient::new(mirror));
        runtime
            .block_on(avm::CancellableFuture::new(
                AvmApp::new()
                    .add_tool(liberica::Tool::new(http_client.clone()))
                    .add_tool(go::Tool::new(http_client.clone()))
                    .run(paths, &http_client),
            ))
            .unwrap_or(Ok(()))
    })();

    if let Err(e) = r {
        log::error!("{e}");
    }
}
