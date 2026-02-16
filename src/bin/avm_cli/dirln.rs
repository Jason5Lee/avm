use clap::Args;
use std::path::PathBuf;

#[derive(Debug, Clone, Args)]
pub struct DirlnArgs {
    #[arg(help = "Source directory path")]
    pub source: PathBuf,
    #[arg(help = "Target directory path where the link will be created")]
    pub target: PathBuf,
}

pub async fn run(args: DirlnArgs) -> anyhow::Result<()> {
    any_version_manager::spawn_blocking(move || {
        any_version_manager::io::blocking::create_link(&args.source, &args.target)?;
        Ok(())
    })
    .await
}
