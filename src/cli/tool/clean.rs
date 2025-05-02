use crate::tool::general_tool;
use crate::tool::GeneralTool;

pub const CMD: &str = "clean";

pub fn command() -> clap::Command {
    clap::Command::new(CMD).about("Clean up the temporary directories and dangling alias tags")
}

pub async fn run(tool: &dyn GeneralTool, tools_base: &std::path::Path) -> anyhow::Result<()> {
    general_tool::clean(tool, tools_base).await
}
