use crate::tool::general_tool;
use crate::tool::GeneralTool;
use crate::tool::ToolInfo;

pub const CMD: &str = "delete";

pub fn command(_info: &ToolInfo) -> clap::Command {
    clap::Command::new(CMD).about("Delete an existing tag").arg(
        clap::Arg::new("tag")
            .value_name("tag")
            .help("The tag name to be deleted")
            .required(true),
    )
}

pub async fn run(
    tool: &dyn GeneralTool,
    tools_base: &std::path::Path,
    args: &clap::ArgMatches,
) -> anyhow::Result<()> {
    let tag_to_delete = args
        .get_one::<String>("tag")
        .expect("Tag argument is required")
        .into();

    general_tool::delete_tag(tool, tools_base, tag_to_delete).await
}
