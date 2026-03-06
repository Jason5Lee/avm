# avm

`avm` is a CLI for managing multiple versions of multiple development tools with a shared workflow.

## Supported Tools

Built-in general tools:

- `go`: Go programming language
- `node`: Node.js JavaScript runtime
- `liberica`: Liberica Java JDK/JRE
- `pnpm`: Fast, disk space efficient package manager for Node.js

Use `avm tool` to list all supported tools, and `avm tool <tool>` to inspect platform/flavor values and install examples.

## Command Model

General tool commands follow this shape:

```bash
avm <subcommand> <tool> [args]
```

Examples:

```bash
avm install node --lts-only # Install the latest LTS version
avm get-vers go --platform x64-linux # Install the latest non-prerelease x64 Linux (no matter what platform it runs on) version.
avm install liberica --platform x64-linux --flavor jdk
avm install pnpm -x 10 # Install the latest non-prerelease version in the 10.x.x series.
```

## Usage Notes

- `avm` does not modify shell environment variables.
- Use `avm path <tool> [tag]` or `avm entry-path <tool> [tag]` and wire paths in your shell config.
- `entry-path` may point to an executable binary or to a runtime entry file that should be invoked by the corresponding runtime.
- Tags and aliases are filesystem-based and can be managed with `alias`, `copy`, `remove`, and `clean`.
  - This means an alias tag can point to arbitary versions while having the same path
- For offline installation:
  1. Run `avm get-downinfo <tool> ...` to obtain URL/hash metadata.
  2. Download the archive.
  3. Run `avm install-local <tool> <archive> <target_tag> [--hash ...]`.

## Example: Multiple Versions, Alias, and Paths

This example uses `node`; the workflow is identical for other tools.

Install two versions:

```bash
avm install node -x 22 # Install NodeJS 22.x.x
avm install node -x 24 # Install NodeJS 24.x.x
```

Create an alias that points to a specific installed version:

```bash
avm alias node arm64-mac_24.14.0 default
```

Show the alias path and the concrete version path:

```bash
avm path node default
avm path node arm64-mac_24.14.0
```

Point the same alias at a different installed version:

```bash
avm alias node arm64-mac_22.22.0 default
```

The alias path stays the same but now points to the other version:

```bash
avm path node default
avm path node arm64-mac_22.22.0
```

If you wire your shell to use the alias path (for example `$(avm path node default)`),
updating the alias switches the tool version without changing the path.

The `default` tag is treated specially. It is the default tag to run with `avm run` and `avm path` if no extra arguments are provided and can be set automatically during installation with the `--default` option.

## Configuration

Print effective config file path:

```bash
avm config-path
```

You can also override config path via environment variable `CONFIG_PATH`.

Config format (`toml`):

```toml
# Optional: Storage directory for AVM data, including installed tools.
# Default: OS-specific local data directory.
data_path = "/path/to/data"

# Optional: URL prefix replacement rules for downloads.
[[mirrors]]
from = "https://origin.example.com/tool"
to = "https://mirror.example.com/tool"

# Optional: Override the default platform for tools that support platform selection
# (currently: go, node, liberica).
# By default, AVM uses the compile-target platform baked into the avm binary at build time.
# The value must be a valid platform string for the tool (see `avm tool <tool>` for available platforms).
# If the value does not match any supported platform of the tool, it is ignored
# and fallback uses that same compile-target platform.
# Resolution order: tool-specific entry -> global -> compile-target platform.
[default-platform]
global = "x64-linux"    # applies to all tools that support platform selection
go = "arm64-macos"      # tool-specific override (takes precedence over global)
```

## Roadmap

* [x] Liberica JDK/JRE
* [x] Go
* [x] Node.js
* [x] pnpm
  * Manage multiple pnpm versions is needed based on real-world usage scenarios.
* [ ] .NET
* [ ] gcc
* [ ] clang
* [ ] Feature: External Alias

Won't consider:
- Rust: please use [rustup](https://rustup.rs/) instead.
- Python: please use [uv](https://docs.astral.sh/uv/) instead.
- MSVC: I don't want to reverse-engineer what have the installer from Microsoft done.
