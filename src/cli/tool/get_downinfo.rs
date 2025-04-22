use crate::tool::general_tool;
use crate::tool::GeneralTool;
use crate::tool::ToolInfo;

use super::{
    add_flavor_arg, add_platform_arg, get_flavor, get_install_version, get_platform, latest_arg,
};

pub const CMD: &str = "get-downinfo";

pub fn command(info: &ToolInfo) -> clap::Command {
    let mut subcmd = clap::Command::new(CMD).about("Get download info").arg(
        clap::Arg::new("version")
            .required(true)
            .help("Version to get download link for"),
    );
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
    subcmd = subcmd.arg(latest_arg());

    subcmd
}

pub async fn run(tool: &dyn GeneralTool, args: &clap::ArgMatches) -> anyhow::Result<()> {
    let platform = get_platform(args).map(|p| p.into());
    let flavor = get_flavor(args).map(|f| f.into());
    let install_version = get_install_version(args);

    let downinfo = general_tool::get_downinfo(tool, platform, flavor, install_version).await?;
    println!("{}", serde_yaml_ng::to_string(&downinfo)?);
    Ok(())
}
