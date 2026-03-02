# avm

`avm` is a CLI for managing multiple versions of multiple development tools with a shared workflow.

## Supported Tools

Built-in general tools:

- `go`
- `node`
- `liberica`

Use `avm tool` to list all supported tools, and `avm tool <tool>` to inspect platform/flavor values and install examples.

## Command Model

General tool commands follow this shape:

```bash
avm <subcommand> <tool> [args]
```

Examples:

```bash
avm install node --lts
avm get-vers go --platform x64-linux
avm install liberica --platform x64-linux --flavor jdk
```

## Usage Notes

- `avm` does not modify shell environment variables.
- Use `avm path <tool> [tag]` or `avm exe-path <tool> [tag]` and wire paths in your shell config.
- Tags and aliases are filesystem-based and can be managed with `alias`, `copy`, `remove`, and `clean`.
  - This means an alias tag can point to arbitary versions while having the same path.
- For offline installation:
  1. Run `avm get-downinfo <tool> ...` to obtain URL/hash metadata.
  2. Download the archive.
  3. Run `avm install-local <tool> <archive> <target_tag> [--hash ...]`.

## Configuration

Print effective config file path:

```bash
avm config-path
```

You can also override config path via environment variable `CONFIG_PATH`.

Config format (`toml`):

```toml
# Optional: Storage directory for AVM data.
# Default: OS-specific local data directory.
data_path = "/path/to/data"

# Optional: URL prefix replacement rules for downloads.
[[mirrors]]
from = "https://origin.example.com/tool"
to = "https://mirror.example.com/tool"

# Optional: Override the default platform for tools that support platform selection
# (currently: go, node, liberica).
# By default, AVM detects the platform from the current OS and CPU.
# The value must be a valid platform string for the tool (see `avm tool <tool>` for available platforms).
# If the value does not match any supported platform of the tool, it is ignored
# and detection falls back to the current OS and CPU.
# Resolution order: tool-specific entry -> global -> current OS and CPU.
[default-platform]
global = "x64-linux"    # applies to all tools that support platform selection
go = "arm64-macos"      # tool-specific override (takes precedence over global)
```

## Roadmap

* [x] Liberica JDK/JRE
* [x] Go
* [x] Node.js
* [ ] .NET
* [ ] pnpm
  * Manage multiple pnpm versions is needed based on real-world usage scenarios.
* [ ] gcc
* [ ] clang
* [ ] Feature: External Alias

Won't consider:
- Rust: please use [rustup](https://rustup.rs/) instead.
- Python: please use [uv](https://docs.astral.sh/uv/) instead.
- MSVC: I don't want to reverse-engineer what have the installer from Microsoft done.
