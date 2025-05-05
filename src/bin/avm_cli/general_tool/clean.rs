use any_version_manager::tool::general_tool;
use any_version_manager::tool::GeneralTool;

pub const CMD: &str = "clean";

pub fn command() -> clap::Command {
    clap::Command::new(CMD).about("Clean up the temporary directories and dangling alias tags")
}

pub async fn run(tool: &impl GeneralTool, tools_base: &std::path::Path) -> anyhow::Result<()> {
    general_tool::clean(tool, tools_base).await
}
