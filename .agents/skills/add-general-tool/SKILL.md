---
name: add-general-tool
description: 'Add a new AVM general tool. Use when implementing a new tool under src/tool/general_tool, wiring CLI dispatch, updating tool metadata, and validating README or command help coverage.'
argument-hint: 'Describe the tool name and any platform, flavor, or metadata requirements.'
---

# Add General Tool

## When to Use

- Add a new general tool implementation to this repository.
- Extend the CLI so `avm <subcommand> <tool>` supports the new tool.
- Update tool metadata and usage guidance for `avm tool` output.

## Procedure

1. Implement the tool in `src/tool/general_tool/<tool>.rs` and satisfy `GeneralTool`.
2. Add the module export in `src/tool/general_tool.rs` if needed.
3. Extend `ToolName` and `ToolSet` in `src/bin/avm_cli/general_tool/mod.rs`.
4. Wire dispatch branches for every relevant subcommand in `src/bin/avm_cli/general_tool/mod.rs`.
5. Update `ToolInfo` in `src/tool.rs` so `about`, platform metadata, flavor metadata, and `after_long_help` stay complete.
6. Ensure `avm tool` and `avm tool <tool>` show correct metadata and examples.
7. Add or update README usage examples if CLI behavior changed.
8. Validate with `cargo fmt`, `cargo test`, `cargo clippy`, and `cargo clippy --all-targets --all-features` when the change scope warrants it.

## Repository-Specific Notes

- General tool commands must keep the order `avm <subcommand> <tool> [other args]`.
- Keep `platform` and `flavor` as optional strings in Clap parsers; expose valid values through `avm tool [tool]`.
- Prefer the `FnTool` and `AsyncFnTool` dispatch pattern over repeating tool-name matches at call sites.