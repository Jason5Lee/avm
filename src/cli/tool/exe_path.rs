use crate::tool::general_tool;
use crate::tool::GeneralTool;
use clap::Arg;

pub const CMD: &str = "exe-path";

pub fn command() -> clap::Command {
    clap::Command::new(CMD)
        .about("Get the path of the executable program")
        .arg(
            Arg::new("tag")
                .help("Tag to get path for")
                .default_value("default"),
        )
}

pub fn run(
    tool: &dyn GeneralTool,
    tools_base: &std::path::Path,
    args: &clap::ArgMatches,
) -> anyhow::Result<()> {
    let tag = args
        .get_one::<String>("tag")
        .expect("tag has default value");
    let path = general_tool::get_exe_path(tool, tools_base, tag)?;
    println!("{}", path.display());
    Ok(())
}
