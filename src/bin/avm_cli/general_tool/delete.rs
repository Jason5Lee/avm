use smol_str::SmolStr;

use any_version_manager::tool::general_tool;
use any_version_manager::tool::GeneralTool;
use any_version_manager::tool::ToolInfo;

pub const CMD: &str = "delete";

pub fn command(_info: &ToolInfo) -> clap::Command {
    clap::Command::new(CMD)
        .about("Delete an existing tag")
        .arg(
            clap::Arg::new("tag")
                .value_name("tag")
                .help("The tag(s) to be deleted")
                .required(true)
                .num_args(1..),
        )
        .arg(
            clap::Arg::new("allow-dangling")
                .long("allow-dangling")
                .help("Allow deleting a tag that is an alias target, create a dangling alias tag")
                .action(clap::ArgAction::SetTrue),
        )
}

pub async fn run(
    tool: &impl GeneralTool,
    tools_base: &std::path::Path,
    args: &clap::ArgMatches,
) -> anyhow::Result<()> {
    let tags_to_delete = args
        .get_many::<String>("tag")
        .expect("tag is required")
        .map(SmolStr::from)
        .collect::<Vec<_>>();
    let allow_dangling = args.get_flag("allow-dangling");
    general_tool::delete_tag(tool, tools_base, tags_to_delete, allow_dangling).await
}
