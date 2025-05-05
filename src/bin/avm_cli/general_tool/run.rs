use any_version_manager::tool::{general_tool, GeneralTool};
use clap::Arg;

pub const CMD: &str = "run";

pub fn command() -> clap::Command {
    clap::Command::new(CMD)
        .about("Run a tool with the specified tag")
        .arg(Arg::new("tag").help("Tag to run").default_value("default"))
        .arg(
            Arg::new("args")
                .help("Arguments to pass to the tool")
                .num_args(0..)
                .last(true)
                .value_parser(clap::value_parser!(std::ffi::OsString)),
        )
}

pub async fn run(
    tool: &impl GeneralTool,
    tools_base: &std::path::Path,
    args: &clap::ArgMatches,
) -> anyhow::Result<()> {
    let tag = args.get_one::<String>("tag").unwrap();
    let tool_args: Vec<std::ffi::OsString> = args
        .get_many::<std::ffi::OsString>("args")
        .unwrap_or_default()
        .cloned()
        .collect();

    let mut command = general_tool::run_command(tool, tools_base, tag, tool_args).await?;
    any_version_manager::spawn_blocking(move || {
        command.spawn()?.wait()?;
        Ok(())
    })
    .await
}
