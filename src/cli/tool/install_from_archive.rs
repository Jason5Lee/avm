use std::path::PathBuf;

use crate::tool::general_tool;
use crate::tool::GeneralTool;

pub const CMD: &str = "install-from-archive";

pub fn command() -> clap::Command {
    clap::Command::new(CMD)
        .about("Install a specific tool from a local archive")
        .arg(
            clap::Arg::new("archive")
                .value_name("archive")
                .help("The path to the archive file")
                .value_parser(clap::builder::PathBufValueParser::new())
                .required(true),
        )
        .arg(
            clap::Arg::new("target_tag")
                .value_name("target_tag")
                .help("The tag name to be installed")
                .required(true),
        )
        .arg(
            clap::Arg::new("hash")
                .value_name("hash")
                .long("hash")
                .help("The hash of the archive file")
                .required(false),
        )
        .arg(
            clap::Arg::new("update")
                .long("update")
                .action(clap::ArgAction::SetTrue)
                .help("Update if the tag is already installed"),
        )
        .arg(
            clap::Arg::new("default")
                .long("default")
                .action(clap::ArgAction::SetTrue)
                .help("Set the installed version as the default version"),
        )
}

pub async fn run(
    tool: &dyn GeneralTool,
    tools_base: &std::path::Path,
    args: &clap::ArgMatches,
) -> anyhow::Result<()> {
    let archive = args.get_one::<PathBuf>("archive").unwrap();
    let target_tag = args.get_one::<String>("target_tag").unwrap();
    let hash = args.get_one::<String>("hash");
    let update = args.get_flag("update");
    let default = args.get_flag("default");

    general_tool::install_from_archive(
        tool,
        tools_base,
        archive.clone(),
        target_tag,
        hash.map(|x| x.as_str()),
        update,
        default,
    )
    .await?;

    Ok(())
}
