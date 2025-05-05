use std::sync::Arc;

use crate::HttpClient;
use any_version_manager::tool::GeneralTool;
use any_version_manager::tool::VersionFilter;
use async_trait::async_trait;
use clap::builder::PossibleValuesParser;
use smol_str::SmolStr;

use crate::avm_cli::Paths;

mod alias;
mod clean;
mod copy;
mod delete;
mod exe_path;
mod get_downinfo;
mod get_vers;
mod install;
mod install_local;
mod list;
mod path;
mod run;

pub(super) fn new_avm_subcommand(tool: impl GeneralTool + 'static) -> super::AvmSubcommand {
    let info = tool.info();
    let mut cmd = clap::Command::new(info.name.to_string())
        .about(info.about.to_string())
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(alias::command(info))
        .subcommand(copy::command(info))
        .subcommand(delete::command(info))
        .subcommand(get_downinfo::command(info))
        .subcommand(get_vers::command(info))
        .subcommand(install::command(info))
        .subcommand(install_local::command())
        .subcommand(list::command())
        .subcommand(path::command())
        .subcommand(exe_path::command())
        .subcommand(run::command())
        .subcommand(clean::command());

    if let Some(after_long_help) = &info.after_long_help {
        cmd = cmd.after_long_help(after_long_help.to_string());
    }
    super::AvmSubcommand {
        cmd,
        run_command: Box::new(RunToolCommand { tool }),
    }
}

struct RunToolCommand<T: GeneralTool> {
    tool: T,
}

#[async_trait]
impl<T: GeneralTool> super::RunSubcommand for RunToolCommand<T> {
    async fn run(
        &self,
        paths: Paths,
        client: Arc<HttpClient>,
        args: &clap::ArgMatches,
    ) -> anyhow::Result<()> {
        match args.subcommand() {
            Some((get_downinfo::CMD, args)) => get_downinfo::run(&self.tool, args).await,
            Some((get_vers::CMD, args)) => get_vers::run(&self.tool, args).await,
            Some((alias::CMD, args)) => alias::run(&self.tool, &paths.tool_dir, args).await,
            Some((copy::CMD, args)) => copy::run(&self.tool, &paths.tool_dir, args).await,
            Some((delete::CMD, args)) => delete::run(&self.tool, &paths.tool_dir, args).await,
            Some((install::CMD, args)) => {
                install::run(&self.tool, &client, &paths.tool_dir, args).await
            }
            Some((install_local::CMD, args)) => {
                install_local::run(&self.tool, &paths.tool_dir, args).await
            }
            Some((list::CMD, _)) => list::run(&self.tool, &paths.tool_dir).await,
            Some((path::CMD, args)) => path::run(&self.tool, &paths.tool_dir, args),
            Some((exe_path::CMD, args)) => exe_path::run(&self.tool, &paths.tool_dir, args),
            Some((run::CMD, args)) => run::run(&self.tool, &paths.tool_dir, args).await,
            Some((clean::CMD, _)) => clean::run(&self.tool, &paths.tool_dir).await,
            _ => unreachable!(),
        }
    }
}

pub fn add_platform_arg(
    subcmd: clap::Command,
    all_platforms: Option<&[SmolStr]>,
    default_platform: Option<&SmolStr>,
) -> clap::Command {
    let Some(all_platforms) = all_platforms else {
        return subcmd;
    };

    let arg = clap::Arg::new("platform")
        .long("platform")
        .value_parser(PossibleValuesParser::new(
            all_platforms.iter().map(|s| s.to_string()),
        ))
        .help("Platform to use");

    let arg = if let Some(default) = default_platform {
        arg.default_value(default.to_string())
    } else {
        arg
    };

    subcmd.arg(arg)
}

pub fn add_flavor_arg(
    subcmd: clap::Command,
    all_flavors: Option<&[SmolStr]>,
    default_flavor: Option<&SmolStr>,
) -> clap::Command {
    let Some(all_flavors) = all_flavors else {
        return subcmd;
    };

    let arg = clap::Arg::new("flavor")
        .long("flavor")
        .value_parser(PossibleValuesParser::new(
            all_flavors.iter().map(|s| s.to_string()),
        ))
        .help("Flavor to use");

    let arg = if let Some(default) = default_flavor {
        arg.default_value(default.to_string())
    } else {
        arg
    };

    subcmd.arg(arg)
}

pub fn get_platform(args: &clap::ArgMatches) -> Option<&str> {
    match args.try_get_one::<String>("platform") {
        Ok(s) => s.map(|s| s.as_str()),
        Err(_) => None,
    }
}

pub fn get_flavor(args: &clap::ArgMatches) -> Option<&str> {
    match args.try_get_one::<String>("flavor") {
        Ok(s) => s.map(|s| s.as_str()),
        Err(_) => None,
    }
}

pub fn version_arg() -> clap::Arg {
    clap::Arg::new("version").help("Specific version to install")
}

pub fn major_arg() -> clap::Arg {
    clap::Arg::new("major")
        .short('m')
        .long("major")
        .help("Major version filter")
}

pub fn get_major(args: &clap::ArgMatches) -> Option<&String> {
    args.try_get_one::<String>("major")
        .expect("major_arg is not added")
}

pub fn lts_arg() -> clap::Arg {
    clap::Arg::new("lts")
        .long("lts")
        .action(clap::ArgAction::SetTrue)
        .help("Only show LTS versions")
}

pub fn get_lts_flag(args: &clap::ArgMatches) -> bool {
    args.get_flag("lts")
}

pub fn get_install_version_filter(args: &clap::ArgMatches) -> VersionFilter {
    let exact_version = args
        .try_get_one::<String>("version")
        .expect("version_arg is not added")
        .map(SmolStr::from);
    let lts_only = get_lts_flag(args);
    let major_version = get_major(args).map(SmolStr::from);

    VersionFilter {
        exact_version,
        lts_only,
        major_version,
    }
}

pub fn src_tag_arg() -> clap::Arg {
    clap::Arg::new("src_tag")
        .value_name("src tag")
        .help("The source tag")
        .required(true)
}

pub fn get_src_tag(args: &clap::ArgMatches) -> &String {
    args.get_one::<String>("src_tag")
        .expect("src_tag is not added")
}
