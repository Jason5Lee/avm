use any_version_manager::tool::general_tool;
use any_version_manager::tool::GeneralTool;
use any_version_manager::tool::ToolInfo;

use super::lts_arg;
use super::version_arg;
use super::{
    add_flavor_arg, add_platform_arg, get_flavor, get_install_version_filter, get_platform,
    major_arg,
};

pub const CMD: &str = "get-downinfo";

pub fn command(info: &ToolInfo) -> clap::Command {
    let mut subcmd = clap::Command::new(CMD)
        .about("Get download info")
        .arg(version_arg())
        .arg(major_arg())
        .arg(lts_arg());
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

    subcmd
}

pub async fn run(tool: &impl GeneralTool, args: &clap::ArgMatches) -> anyhow::Result<()> {
    let platform = get_platform(args).map(|p| p.into());
    let flavor = get_flavor(args).map(|f| f.into());
    let install_version = get_install_version_filter(args);

    let downinfo = general_tool::get_downinfo(tool, platform, flavor, install_version).await?;
    println!("{}", serde_yaml_ng::to_string(&downinfo)?);
    Ok(())
}
