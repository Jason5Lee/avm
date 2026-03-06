# AGENTS

This document defines repository-wide instructions for coding agents.

## Scope

- Global: applies to the entire repository unless a narrower rule overrides it.

## Global Rules

### Language

All code comments, commit messages, documentation strings, error messages, and user-facing text in this repository must be written in English.

Guidance:
- Write all inline comments in English.
- Write thrown error messages and logs in English.
- Write documentation and docstrings in English.
- Use English for commit messages.
- Avoid mixed-language identifiers and text.

## Project Structure

- `src/lib.rs`: library entry.
- `src/tool.rs`: tool traits and shared metadata (`ToolInfo`, `GeneralTool`).
- `src/tool/general_tool/`: built-in general tool implementations (`go`, `node`, `liberica`).
- `src/bin/avm.rs`: CLI binary entry.
- `src/bin/avm_cli/mod.rs`: top-level Clap parser, config loading, and command dispatch.
- `src/bin/avm_cli/general_tool/`: argument types and handlers for tool-specific commands.
- `src/bin/avm_cli/global/`: `avm tool` output handlers.
- `src/bin/avm_cli/dirln.rs`: directory link utility command.
- `src/io/`: blocking and async I/O helpers for archive, file, and link operations.

## CLI Conventions

- The CLI uses **Clap derive macros** (`Parser`, `Subcommand`, `Args`, `ValueEnum`), not runtime command building.
- General tool commands follow this fixed order:
  - `avm <subcommand> <tool> [other args]`
  - Example: `avm install node --lts`
  - Example: `avm get-vers go --platform x64-linux`
- Keep non-tool utility commands (`config-path`, `tool`, `dirln`) in top-level command space.
- Do not re-introduce tool-specific argument value restrictions in Clap parsers for `platform` and `flavor`.
  - Keep `platform`/`flavor` as optional strings.
  - Expose valid values through the `avm tool [tool]` command.
- Keep global help text aligned with discoverability:
  - Users should be guided to run `avm tool` and `avm tool <tool>` for installation guidance.

## Tool Dispatch Pattern

- Use `FnTool` and `AsyncFnTool` as the standard dispatch abstraction whenever code needs to call a concrete `GeneralTool` method selected by `ToolName` from `ToolSet`.
- This pattern is not limited to `src/bin/avm_cli/general_tool.rs`; it applies to any module that performs tool-specific behavior through shared dispatch.
- Use `invoke_tool(...)` for synchronous operations and `async_invoke_tool(...)` for asynchronous operations.
- Prefer this pattern over repeating `match ToolName { ... }` branches at each call site.
- Implement operation-specific context as small structs (for example, `RunInstallFn`, `RunEntryPathFn`) and implement `FnTool`/`AsyncFnTool` on them.
- Keep direct non-dispatch calls (such as archive/tag filesystem operations that only need `tool_name`) outside this pattern when no `GeneralTool` instance is required.
- For logic coupled to a specific tool (for example, tool-specific descriptions or behavior variants), define it as a `GeneralTool` trait method (with a default implementation when appropriate) and call it through dispatch. This keeps call sites open for extension and avoids hard-coded tool branches outside tool implementations.

## Tool Metadata Guidance

- The single source of truth for tool capability metadata is `ToolInfo` in `src/tool.rs`.
- `ToolInfo` fields should be maintained when adding/changing tools:
  - `about`
  - `all_platforms`, `default_platform`
  - `all_flavors`, `default_flavor`
  - `after_long_help` (optional)
- The `avm tool <tool>` output depends on this metadata; keep metadata complete and user-oriented.

## Adding a New General Tool

1. Implement the tool in `src/tool/general_tool/<tool>.rs` and satisfy `GeneralTool`.
2. Add it to `src/tool/general_tool.rs` module exports if needed.
3. Extend `ToolName` and `ToolSet` in `src/bin/avm_cli/general_tool/mod.rs`.
4. Wire dispatch branches for every relevant subcommand in `src/bin/avm_cli/general_tool/mod.rs`.
5. Ensure `avm tool` and `avm tool <tool>` show correct metadata and examples.
6. Add or update README usage examples if CLI behavior changed.

## Development Notes

- Prefer minimal, type-safe changes to argument structs in `src/bin/avm_cli/general_tool/mod.rs`.
- Keep command handler modules focused on business logic (`install.rs`, `get_vers.rs`, etc.) and avoid duplicating parsing logic there.
- `src/io/blocking` contains blocking I/O helpers; when calling them from async code, wrap the blocking work in `spawn_blocking`.
- Before committing, run formatting and checks available in the current environment:
  - `cargo fmt`
  - `cargo clippy`
