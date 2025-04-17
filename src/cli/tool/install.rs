use crate::tool::general_tool;
use crate::tool::GeneralTool;
use crate::tool::ToolInfo;
use crate::HttpClient;
use indicatif::ProgressBar;
use indicatif::ProgressStyle;
use smol_str::ToSmolStr;

use super::{
    add_flavor_arg, add_platform_arg, get_flavor, get_install_version, get_platform, latest_arg,
    version_arg,
};

pub const CMD: &str = "install";

pub fn command(info: &ToolInfo) -> clap::Command {
    let mut subcmd = clap::Command::new(CMD)
        .about("Install a specific tool")
        .arg(version_arg());
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
    subcmd = subcmd
        .arg(latest_arg())
        .arg(
            clap::Arg::new("default")
                .long("default")
                .action(clap::ArgAction::SetTrue)
                .help("Set the installed version as the default version"),
        )
        .arg(
            clap::Arg::new("force")
                .long("force")
                .action(clap::ArgAction::SetTrue)
                .help("Force install even if the tag already exists"),
        );

    subcmd
}

pub async fn run(
    tool: &dyn GeneralTool,
    client: &HttpClient,
    tools_base: &std::path::Path,
    args: &clap::ArgMatches,
) -> anyhow::Result<()> {
    let platform = get_platform(args).map(|p| p.to_smolstr());
    let flavor = get_flavor(args).map(|f| f.to_smolstr());
    let force = args.get_flag("force");
    let default = args.get_flag("default");
    let install_version = get_install_version(args);

    let mut download_state = general_tool::install(
        tool,
        client,
        tools_base,
        platform,
        flavor,
        install_version,
        force,
        default,
    )
    .await?;

    let mut downloading_info_printed = false;
    let mut pb: Option<ProgressBar> = None;

    loop {
        match download_state.status() {
            crate::Status::InProgress {
                name,
                progress_ratio,
            } => {
                if name == "Downloading" {
                    if let Some(progress_ratio) = progress_ratio {
                        if let Some(pb) = &mut pb {
                            pb.set_position(progress_ratio.0);
                        } else {
                            let new_pb = ProgressBar::new(progress_ratio.1);
                            new_pb.set_style(ProgressStyle::default_bar()
                                .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")?
                                .progress_chars("#>-"));
                            new_pb.set_position(progress_ratio.0);
                            pb = Some(new_pb);
                        }
                    } else {
                        if !downloading_info_printed {
                            println!("Downloading...");
                            downloading_info_printed = true;
                        }
                    }
                } else if name == "Extracting" {
                    if let Some(pb) = &mut pb {
                        pb.finish_with_message("Download complete");
                    }

                    println!("Extracting...");
                } else {
                    unreachable!("Unknown status: {:?}", name);
                }
            }
            crate::Status::Stopped => {
                break;
            }
        }

        download_state = download_state.advance().await?;
    }

    Ok(())
}
