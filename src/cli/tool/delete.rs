use crate::tool::general_tool;
use crate::tool::GeneralTool;
use crate::tool::ToolInfo;

pub const CMD: &str = "delete";

pub fn command(_info: &ToolInfo) -> clap::Command {
    clap::Command::new(CMD)
        .about("Delete an existing tag")
        .arg(
            clap::Arg::new("tag")
                .value_name("tag")
                .help("The tag to be deleted")
                .required(true),
        )
        .arg(
            clap::Arg::new("allow-dangling")
                .long("allow-dangling")
                .help("Allow deleting a tag that is an alias target, create a dangling alias tag")
                .action(clap::ArgAction::SetTrue),
        )
}

pub async fn run(
    tool: &dyn GeneralTool,
    tools_base: &std::path::Path,
    args: &clap::ArgMatches,
) -> anyhow::Result<()> {
    let tag_to_delete = args
        .get_one::<String>("tag")
        .expect("tag is required")
        .into();
    let allow_dangling = args.get_flag("allow-dangling");
    general_tool::delete_tag(tool, tools_base, tag_to_delete, allow_dangling).await
}
