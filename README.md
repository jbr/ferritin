# 🩸 Ferritin

[![ci][ci-badge]][ci]
[![codecov](https://codecov.io/gh/jbr/ferritin/graph/badge.svg?token=FDpsPBK9zl)](https://codecov.io/gh/jbr/ferritin)
[![crates.io version badge][version-badge]][crate]

[ci]: https://github.com/jbr/ferritin/actions?query=workflow%3ACI
[ci-badge]: https://github.com/jbr/ferritin/workflows/CI/badge.svg
[version-badge]: https://img.shields.io/crates/v/ferritin.svg?style=flat-square
[crate]: https://crates.io/crates/ferritin

A terminal-based Rust documentation viewer

Ferritin reads rustdoc JSON output to display Rust documentation directly in your terminal. It supports local workspace crates, standard library documentation, and crates from crates.io, with search functionality and modern terminal features including mouse support, syntax highlighting, and clickable links.

## Features

- **Interactive documentation browser** with keyboard and mouse navigation
- **One-shot CLI** for quick documentation lookups in the terminal
- **Search across crates** or within specific crates
- **Works seamlessly across three contexts:**
  - Local workspace crates (requires nightly toolchain)
  - Standard library (requires `rust-docs-json` rustup component)
  - Crates from crates.io (fetched from docs.rs on demand)
- **Modern terminal UI** with features like italics, unicode box drawing, syntax highlighting, OSC8 hyperlinks, cursor changes in terminals that support it, and mouse hover previews
- **Fast navigation** with history and breadcrumb trail

## Installation

### Prebuilt binary (recommended)

Prebuilt binaries skip compilation and are available for x86_64/aarch64 macOS, x86_64 Linux,
and x86_64 Windows.

If you have [cargo-binstall](https://github.com/cargo-bins/cargo-binstall), it reads the
release metadata and installs the right binary for your platform:

```bash
cargo binstall ferritin
```

Otherwise, the installer scripts detect your platform, download the matching binary, and
place it in `~/.cargo/bin`:

**macOS and Linux:**

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/jbr/ferritin/releases/download/ferritin-latest/ferritin-installer.sh | sh
```

**Windows (PowerShell):**

```powershell
powershell -c "irm https://github.com/jbr/ferritin/releases/download/ferritin-latest/ferritin-installer.ps1 | iex"
```

Both of the above always install the most recent release. You can also pick a specific
version or download archives directly from the [releases page].

[releases page]: https://github.com/jbr/ferritin/releases

### From source

To build from source (or for a platform without a prebuilt binary):

```bash
cargo install ferritin
```

### Optional: Standard library documentation

To view documentation for `std`, `core`, and `alloc`:

```bash
rustup component add rust-docs-json
```

### For local workspace documentation

Local crate documentation requires a nightly toolchain:

```bash
rustup toolchain install nightly
```

Note: There is a relationship between ferritin's version and the nightly toolchain version. Ferritin
supports a range of recent rustdoc JSON format versions, and if your nightly generates a newer
format, ferritin will make a best effort at parsing it.

## Usage

### Interactive mode

Launch the interactive browser:

```bash
ferritin -i
```

Once running, use `h` or `?` to see available keybindings. Basic navigation:
- `g` - go to an item by path (e.g., `std::vec::Vec`)
- `s` - search (Tab to toggle between current crate and all crates)
- `l` - list available crates
- Arrow keys, j/k, or mouse to navigate
- Click on links to follow them

### CLI mode

View documentation for a specific item:

```bash
ferritin get std::vec::Vec
ferritin get serde::Serialize
```

Search for items:

```bash
ferritin search "hash map"
ferritin search --crate tokio "spawn"
```

List available crates in your workspace:

```bash
ferritin list
```

### Output format

Ferritin auto-detects how to render: ANSI colors on a terminal, plain text when
piped, and a compact markdown-flavored format under a coding agent. Override it
with `--format <tty|plain|agent|json>`. `--format json` emits a structured
representation — of the resolved item for `get`, or of the results for
`search`/`list` — for scripting and programmatic consumers:

```bash
ferritin get std::vec::Vec --format json
```

(The JSON shape is still settling as the library API evolves; a published JSON
Schema is planned.)

## Caching and Storage

Ferritin caches documentation JSON files to avoid repeated downloads and builds:

- **Crates.io documentation**: Cached in `$CARGO_HOME/rustdoc-json/{format-version}/{crate_name}/{crate_version}.json`
- **Search indices**: Binary `.index` files are generated lazily on first search and stored alongside JSON files
- **Standard library search indices**: Written to `{rustc sysroot}/share/doc/rust/json/` if available

The cache uses cargo's home directory (typically `~/.cargo` on Unix systems).

## Current Status

Ferritin is at version 0.x and actively used by the author as a primary documentation interface. It's ready for general use, though the output format should be considered unstable and may change between versions.

**If you're scripting against ferritin's output**, be aware that the text format may change. Pin to a specific version or be prepared to update your scripts.

## Platform Support

Ferritin is developed primarily on Unix-like systems and is also tested on Windows, where both the CLI and interactive TUI work (prebuilt x86_64 Windows binaries are provided). If you encounter issues on any platform, please open an issue or pull request.

## Related Projects

Ferritin was originally developed to support the [rustdoc-mcp MCP server](./rustdoc-mcp/README.md), which provides Rust documentation access for Claude Code and other MCP clients.


## License

<sup>
Licensed under either of <a href="LICENSE-APACHE">Apache License, Version
2.0</a> or <a href="LICENSE-MIT">MIT license</a> at your option.
</sup>

---

<sub>
Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this crate by you, as defined in the Apache-2.0 license, shall
be dual licensed as above, without any additional terms or conditions.
</sub>
