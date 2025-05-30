# avm

(Potentially) Any language Version Manager, a Command-Line Interface tool designed to manage multiple versions of development tools for potentially any programming language, maximizing code reuse.

## Key designs

* **Version Management:** Easily install, manage, and switch between different versions of various development tools.
* **Flexible Installation:**
  * Configure mirrors using URL prefix replacement to adapt to different network environments.
  * Install tools from local archives for offline use. Use `get-downinfo` to get download details and `install-local` with the downloaded files and info for offline installation.
* **Tagging and Aliasing:**
  * Each tool version/architecture is identified by a unique `tag`.
  * Create `aliases` for tags, providing fixed paths that can point to different underlying tool versions. This is useful for configuring your environment.
  * `copy` a tag to duplicate its contents, ideal for tools that modify themselves during execution.
* **Manual Environment Setup:** `avm` does not automatically modify your system's environment variables. Use the `path` subcommand to retrieve the installation path for a specific tag or alias and manually configure your environment. Aliases provide stable paths that can point to different underlying tags.

## Roadmap

* [x] Liberica JDK/JRE
* [x] Go
* [x] Node.js
* [ ] Python
* [ ] .NET
  * .NET CLI has its own SDKs manager. Need to decide how to integrate it.
* [ ] pnpm
  * Manage multiple pnpm versions is needed based on real-world usage scenarios.
* [ ] Rustup
  * [x] Rustup delegate
  * [ ] Install rustup if not found
  * [ ] Install MSVC toolchain on Windows if not found
* [ ] uv for Python
  * [ ] uv delegate
  * [ ] Install uv if not found

## Configuration

The configuration file path can be found by running `avm config-path`.

Format:

```yaml
mirror: # optional: Define download mirrors
  - from: https://original-prefix.com/a/b
    to: https://mirror-prefix.com/c/d # e.g., https://original-prefix.com/a/b/e/f becomes https://mirror-prefix.com/c/d/e/f
  # - ... more mirror rules
dataPath: /path/to/data # Optional: Directory to store downloaded tools. Uses an OS-specific default if omitted.
rustup:
  path: /path/to/rustup # Optional: Path to rustup executable.
```

