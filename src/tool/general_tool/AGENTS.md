# AGENTS

This document defines instructions for coding agents working in `src/tool/general_tool/`.

## Dotnet LTS Handling

- In `dotnet.rs`, prerelease .NET versions must never be surfaced as LTS, even when they come from an LTS release channel.

## Dotnet Release Channel Selection

- When scanning multiple .NET release channels, sort channels by `(major, minor)` descending.
- For max-only lookups (e.g., `get_down_info`/`install`), stop at the first channel that yields a matching release and select that channel's highest matching asset.
