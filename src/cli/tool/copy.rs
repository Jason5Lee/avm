use crate::tool::general_tool;
use crate::tool::GeneralTool;
use crate::tool::ToolInfo;
use smol_str::ToSmolStr;

use super::{get_src_tag, src_tag_arg};

pub const CMD: &str = "copy";

pub fn command(_info: &ToolInfo) -> clap::Command {
    clap::Command::new(CMD)
        .about("Copy an existing tag to a new tag")
        .arg(src_tag_arg())
        .arg(
            clap::Arg::new("target_tag")
                .value_name("target tag")
                .help("The tag name to be copied")
                .required(true),
        )
}

pub async fn run(
    tool: &dyn GeneralTool,
    tools_base: &std::path::Path,
    args: &clap::ArgMatches,
) -> anyhow::Result<()> {
    let src_tag = get_src_tag(args).to_smolstr();
    let target_tag = args
        .get_one::<String>("target_tag")
        .expect("target_tag is required")
        .to_smolstr();

    general_tool::copy_tag(tool, tools_base, src_tag, target_tag).await
}
