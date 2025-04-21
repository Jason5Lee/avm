use crate::tool::general_tool;
use crate::tool::GeneralTool;
use crate::tool::ToolInfo;
use smol_str::ToSmolStr;

use super::{
    add_flavor_arg, add_platform_arg, get_flavor, get_install_version, get_platform, latest_arg,
};

pub const CMD: &str = "get-downurl";

pub fn command(info: &ToolInfo) -> clap::Command {
    let mut subcmd = clap::Command::new(CMD).about("Get download link").arg(
        clap::Arg::new("version")
            .required(true)
            .help("Version to get download link for"),
    );
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
    subcmd = subcmd.arg(latest_arg());

    subcmd
}

pub async fn run(tool: &dyn GeneralTool, args: &clap::ArgMatches) -> anyhow::Result<()> {
    let platform = get_platform(args).map(|p| p.to_smolstr());
    let flavor = get_flavor(args).map(|f| f.to_smolstr());
    let install_version = get_install_version(args);

    let downurl = general_tool::get_downurl(tool, platform, flavor, install_version).await?;
    println!("{}", downurl.url);
    Ok(())
}
