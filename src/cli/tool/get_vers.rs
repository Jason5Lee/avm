use crate::tool::general_tool;
use crate::tool::GeneralTool;
use crate::tool::ToolInfo;
use smol_str::ToSmolStr;

use super::{add_flavor_arg, add_platform_arg, get_flavor, get_platform};

pub const CMD: &str = "get-vers";

pub fn command(info: &ToolInfo) -> clap::Command {
    let mut subcmd = clap::Command::new(CMD).about("Get available versions");
    subcmd = add_platform_arg(
        subcmd,
        info.all_platforms.as_ref().map(|v| v.as_slice()),
        info.default_platform.as_ref(),
    );
    subcmd = add_flavor_arg(
        subcmd,
        info.all_flavors.as_ref().map(|v| v.as_slice()),
        info.default_flavor.as_ref(),
    );
    subcmd = subcmd.arg(
        clap::Arg::new("major")
            .short('m')
            .long("major")
            .help("Major version filter"),
    );
    subcmd
}

pub async fn run(tool: &dyn GeneralTool, args: &clap::ArgMatches) -> anyhow::Result<()> {
    let major = args.get_one::<String>("major").map(|m| m.to_smolstr());
    let platform = get_platform(args).map(|p| p.to_smolstr());
    let flavor = get_flavor(args).map(|f| f.to_smolstr());

    let vers = general_tool::get_vers(tool, platform, flavor, major).await?;
    for v in vers {
        println!(
            "{}: {}{}",
            v.major_version,
            v.version,
            if v.is_lts { " [LTS]" } else { "" }
        );
    }

    Ok(())
}
