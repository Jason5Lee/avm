use any_version_manager::tool::general_tool;
use any_version_manager::tool::GeneralTool;
use any_version_manager::tool::ToolInfo;

use super::{get_src_tag, src_tag_arg};

pub const CMD: &str = "copy";

pub fn command(_info: &ToolInfo) -> clap::Command {
    clap::Command::new(CMD)
        .about("Copy an existing tag to a new tag")
        .arg(src_tag_arg())
        .arg(
            clap::Arg::new("target_tag")
                .value_name("target tag")
                .help("The destination tag")
                .required(true),
        )
}

pub async fn run(
    tool: &impl GeneralTool,
    tools_base: &std::path::Path,
    args: &clap::ArgMatches,
) -> anyhow::Result<()> {
    let src_tag = get_src_tag(args).into();
    let target_tag = args
        .get_one::<String>("target_tag")
        .expect("target_tag is required")
        .into();

    general_tool::copy_tag(tool, tools_base, src_tag, target_tag).await
}
