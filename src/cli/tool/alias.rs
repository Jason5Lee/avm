use crate::tool::general_tool;
use crate::tool::GeneralTool;
use crate::tool::ToolInfo;
use smol_str::ToSmolStr;

use super::{get_src_tag, src_tag_arg};

pub const CMD: &str = "alias";

pub fn command(_info: &ToolInfo) -> clap::Command {
    clap::Command::new(CMD)
        .about("Create a tag alias")
        .arg(src_tag_arg())
        .arg(
            clap::Arg::new("alias_tag")
                .value_name("alias tag")
                .help("The tag name to be created as an alias")
                .required(true),
        )
}

pub async fn run(
    tool: &dyn GeneralTool,
    tools_base: &std::path::Path,
    args: &clap::ArgMatches,
) -> anyhow::Result<()> {
    let src_tag = get_src_tag(args).to_smolstr();
    let alias_tag = args
        .get_one::<String>("alias_tag")
        .expect("alias_tag is required")
        .to_smolstr();

    general_tool::create_alias_tag(tool, tools_base, src_tag, alias_tag).await
}
