# 🩸 Ferretin

[![ci][ci-badge]][ci]
[![codecov](https://codecov.io/gh/jbr/ferretin/graph/badge.svg?token=FDpsPBK9zl)](https://codecov.io/gh/jbr/ferretin)
[![crates.io version badge][version-badge]][crate]

[ci]: https://github.com/jbr/ferretin/actions?query=workflow%3ACI
[ci-badge]: https://github.com/jbr/ferretin/workflows/CI/badge.svg
[version-badge]: https://img.shields.io/crates/v/ferretin.svg?style=flat-square
[crate]: https://crates.io/crates/ferretin

A terminal-based Rust documentation viewer

Ferretin reads rustdoc JSON output to display Rust documentation directly in your terminal. It supports local workspace crates, standard library documentation, and crates from crates.io, with search functionality and modern terminal features including mouse support, syntax highlighting, and clickable links.

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

```bash
cargo install ferretin
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

Note: There is a relationship between ferretin's version and the nightly toolchain version. Ferretin currently supports rustdoc JSON format versions 55, 56, and 57. If your nightly generates a newer format, ferretin won't be able to build local documentation until support is added.

## Usage

### Interactive mode

Launch the interactive browser:

```bash
ferretin -i
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
ferretin get std::vec::Vec
ferretin get serde::Serialize
```

Search for items:

```bash
ferretin search "hash map"
ferretin search --crate tokio "spawn"
```

List available crates in your workspace:

```bash
ferretin list
```

## Caching and Storage

Ferretin caches documentation JSON files to avoid repeated downloads and builds:

- **Crates.io documentation**: Cached in `$CARGO_HOME/rustdoc-json/{format-version}/{crate_name}/{crate_version}.json`
- **Search indices**: Binary `.index` files are generated lazily on first search and stored alongside JSON files
- **Standard library search indices**: Written to `{rustc sysroot}/share/doc/rust/json/` if available

The cache uses cargo's home directory (typically `~/.cargo` on Unix systems).

## Current Status

Ferretin is at version 0.1 and actively used by the author as a primary documentation interface. It's ready for general use, though the output format should be considered unstable and may change between versions.

**If you're scripting against ferretin's output**, be aware that the text format may change. Pin to a specific version or be prepared to update your scripts.

## Platform Support

Ferretin is developed and tested on Unix-like systems. Windows compatibility is untested. If you encounter issues on Windows or other platforms, please open an issue or pull request.

## Related Projects

Ferretin was originally developed to support the [rustdoc-mcp MCP server](./rustdoc-mcp/README.md), which provides Rust documentation access for Claude Code and other MCP clients.


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
