# avm

`avm` is a CLI for managing multiple versions of development tools with a shared workflow.

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

Current top-level commands:

- `config-path`
- `tool`
- `install`
- `get-vers`
- `get-downinfo`
- `install-local`
- `list`
- `path`
- `exe-path`
- `run`
- `alias`
- `copy`
- `remove`
- `clean`
- `dirln`

## Usage Notes

- `avm` does not modify shell environment variables automatically.
- Use `avm path <tool> [tag]` or `avm exe-path <tool> [tag]` and wire paths in your shell config.
- Tags and aliases are filesystem-based and can be managed with `alias`, `copy`, `remove`, and `clean`.
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
