# anyup

Managing development environments for any programming language.

This project is in active development, see `dev` branch for the progress.

## Motivation

This project is inspired by [rustup](https://rustup.rs/). Rust uses `rustup` for quickly installing and updating its development environment, including the compiler, build system, and more. The principles behind `rustup` can be applied to many other scenarios. Therefore, this tool aims to adapt those ideas for setting up any type of development environment.

## Roadmap

[ ] Liberica Java
[ ] Gradle
[ ] NodeJS
[ ] Python
[ ] Go
[ ] .NET

## Command Line 

get-vers [--major/-m <major version>] get-downlink [--latest [major version]] install [--latest [major version]] [--from-archive <archive path>] [--set-default <link/copy>]
list => tag,[link from]
link <from> <to> [--set]
copy <from> <to> [--set]del <tag> [--force/-f]
del <Tag> -f/--force
-- ... (run default)
run <tag> ...

Common args: latest, set, force


### gradle

Config: GRADLE_HOME (default: ~/.gradle)
get-downurl [--latest]
install [--latest] [--from-archive <archive path>] [--set-default <link/copy>] [--force/-f]
list => tag,[link from]
link <from> <to> [--set]
copy <from> <to> [--set]
del <tag> [--force/-f]
path <tag>
-- ... (run default)
run <tag> ...

