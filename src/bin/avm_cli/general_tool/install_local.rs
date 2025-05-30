use std::path::PathBuf;

use any_version_manager::tool::general_tool;
use any_version_manager::tool::GeneralTool;

pub const CMD: &str = "install-local";

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
                .long_help("The hash of the archive file, in the format of yaml like `sha256: <hash hex>`, supports sha1 and sha256")
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
    tool: &impl GeneralTool,
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
