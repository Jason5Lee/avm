use any_version_manager::tool::general_tool;
use any_version_manager::tool::GeneralTool;
use any_version_manager::tool::ToolInfo;
use any_version_manager::tool::VersionFilter;

use super::lts_arg;
use super::major_arg;
use super::{add_flavor_arg, add_platform_arg, get_flavor, get_platform};

pub const CMD: &str = "get-vers";

pub fn command(info: &ToolInfo) -> clap::Command {
    let mut subcmd = clap::Command::new(CMD).about("Get available versions");
    subcmd = add_platform_arg(
        subcmd,
        info.all_platforms.as_deref(),
        info.default_platform.as_ref(),
    );
    subcmd = add_flavor_arg(
        subcmd,
        info.all_flavors.as_deref(),
        info.default_flavor.as_ref(),
    );
    subcmd.arg(major_arg()).arg(lts_arg())
}

pub async fn run(tool: &impl GeneralTool, args: &clap::ArgMatches) -> anyhow::Result<()> {
    let platform = get_platform(args).map(|p| p.into());
    let flavor = get_flavor(args).map(|f| f.into());
    let major = args.get_one::<String>("major").map(|m| m.into());
    let lts = args.get_flag("lts");

    let version_filter = VersionFilter {
        lts_only: lts,
        major_version: major,
        exact_version: None,
    };

    let vers = general_tool::get_vers(tool, platform, flavor, version_filter).await?;
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
