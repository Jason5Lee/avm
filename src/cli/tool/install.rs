use crate::tool::general_tool;
use crate::tool::GeneralTool;
use crate::tool::ToolInfo;
use crate::HttpClient;
use indicatif::ProgressBar;
use indicatif::ProgressStyle;
use smol_str::SmolStr;

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
        info.all_platforms.as_deref(),
        info.default_platform.as_ref(),
    );
    subcmd = add_flavor_arg(
        subcmd,
        info.all_flavors.as_deref(),
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
            clap::Arg::new("update")
                .long("update")
                .action(clap::ArgAction::SetTrue)
                .help("Update if the installed version is already installed"),
        );

    subcmd
}

pub async fn run(
    tool: &dyn GeneralTool,
    client: &HttpClient,
    tools_base: &std::path::Path,
    args: &clap::ArgMatches,
) -> anyhow::Result<()> {
    let platform = get_platform(args).map(|p| p.into());
    let flavor = get_flavor(args).map(|f| f.into());
    let update = args.get_flag("update");
    let default = args.get_flag("default");
    let install_version = get_install_version(args);

    let (target_tag, mut download_state) = general_tool::InstallArgs {
        tool,
        client,
        tools_base,
        platform,
        flavor,
        install_version,
        update,
        default,
    }
    .install()
    .await?;

    log::info!("\"{target_tag}\" will be installed");
    let mut prev_name: Option<SmolStr> = None;
    let mut pb: Option<ProgressBar> = None;

    #[allow(clippy::while_let_loop)] // It's more clear that every case is handled.
    loop {
        match download_state.status() {
            crate::Status::InProgress {
                name,
                progress_ratio,
            } => {
                if prev_name.as_ref() != Some(&name) {
                    if let Some(pb) = pb.take() {
                        pb.finish_with_message("Completed.");
                    }

                    log::info!("{name} ...");
                    prev_name = Some(name);
                }

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
