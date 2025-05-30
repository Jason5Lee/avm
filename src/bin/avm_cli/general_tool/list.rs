use any_version_manager::tool::general_tool;
use any_version_manager::tool::GeneralTool;

pub const CMD: &str = "list";

pub fn command() -> clap::Command {
    clap::Command::new(CMD).about("List existing tags")
}

pub async fn run(tool: &impl GeneralTool, tools_base: &std::path::Path) -> anyhow::Result<()> {
    for (tag, target) in general_tool::list_tags(tool, tools_base).await? {
        print!("{}", tag);
        if let Some(target) = target {
            print!(" -> {}", target);
        }
        println!();
    }
    Ok(())
}
