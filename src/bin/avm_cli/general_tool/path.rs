use any_version_manager::tool::general_tool;
use any_version_manager::tool::GeneralTool;
use clap::Arg;

pub const CMD: &str = "path";

pub fn command() -> clap::Command {
    clap::Command::new(CMD)
        .about("Get the tool path of a specific tag")
        .arg(
            Arg::new("tag")
                .help("Tag to get path for")
                .default_value("default"),
        )
}

pub fn run(
    tool: &impl GeneralTool,
    tools_base: &std::path::Path,
    args: &clap::ArgMatches,
) -> anyhow::Result<()> {
    let tag = args.get_one::<String>("tag").unwrap();
    let path = general_tool::get_tag_path(tool, tools_base, tag)?;
    println!("{}", path.display());
    Ok(())
}
