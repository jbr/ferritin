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

Ferritin keeps its caches under a single directory — `$FERRITIN_HOME` if set,
else `$XDG_CACHE_HOME/ferritin`, else `~/.cache/ferritin`. Everything in it is
reconstructible (fetched or derived), so it is always safe to delete:

- **Crates.io documentation**: raw rustdoc JSON at
  `docs/{crate}/{version}/{format-version}.json`, with derived binary caches
  beside it (an rkyv archive for fast partial loads, and a `.index` file
  generated lazily on first search)
- **crates.io metadata**: the crate-names artifacts (`crate-names/`) used for
  version resolution and typeahead, and per-crate version lookups
  (`crates-io-versions/`)
- **Standard library search indices**: written to
  `{rustc sysroot}/share/doc/rust/json/` if available
- **Local workspace documentation**: built into the workspace's own
  `target/doc/`, not the shared cache

Earlier versions cached under `$CARGO_HOME/rustdoc-json`; a cache found there
is migrated automatically on first run.

## Environment Variables

| Variable | Meaning |
| --- | --- |
| `FERRITIN_HOME` | Cache root override (see above) |
| `FERRITIN_THEME` | Default color theme (equivalent to `--theme`) |
| `FERRITIN_CRATE_NAMES_URL`, `FERRITIN_CRATE_DESCRIPTIONS_URL` | Override where the crate-names artifacts are fetched from (e.g. a mirror) |

`ferritin serve` (the opt-in `serve`/`acme` features; not present in
distributed binaries) is configured 12-factor style:

| Variable | Meaning |
| --- | --- |
| `HOST`, `PORT` | Bind address (`::` binds dual-stack) |
| `FERRITIN_CACHE_BYTES` | In-memory crate-cache weight cap |
| `FERRITIN_RSS_HIGH_WATER_BYTES` | RSS guard threshold (Linux only): shed cache weight when anonymous RSS exceeds it |
| `FERRITIN_RATELIMIT` | Per-client `/api` requests per minute |
| `FERRITIN_ACME_DOMAIN` | Activates automatic HTTPS via Let's Encrypt for this domain |
| `FERRITIN_ACME_CACHE_DIR` | Required with ACME: persists the account key and certificates |
| `FERRITIN_ACME_CONTACT` | Optional ACME contact email |
| `FERRITIN_ACME_PRODUCTION` | Use the Let's Encrypt production directory (default: staging) |

Development-only knobs (not part of the stable interface): `FERRITIN_TEST_MODE`
(normalized snapshot-test output), `FERRITIN_REINDEX` (rebuild search indexes
in memory, bypassing the disk cache), and the search-scoring probes
`FERRITIN_NAME_WEIGHT`, `FERRITIN_ANCESTOR_WEIGHTS`, and
`FERRITIN_NAME_PREFIX_COUNT`.

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
