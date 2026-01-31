# Architecture Overview

This document describes the architecture of the ferritin project, a terminal-based Rust documentation viewer and its supporting libraries.

## Project Structure

The workspace contains three crates:

- **ferritin-common**: Shared library for rustdoc navigation, search, and data management
- **ferritin**: Terminal documentation viewer with CLI and TUI frontends
- **rustdoc-mcp**: MCP server providing Rust documentation access for Claude Code and other MCP clients

This document focuses primarily on ferritin-common and ferritin, as rustdoc-mcp is intended to eventually become a thin layer on top of ferritin.

## Core Design Principles

### Zero-Copy Architecture

Throughout the codebase, data is borrowed from the rustdoc JSON rather than copied. The `Navigator` owns all `RustdocData` instances in a `FrozenMap`, and all references (`&'a RustdocData`, `DocRef<'a>`) borrow from this single source. String data uses `Cow<'a, str>` to borrow from JSON wherever possible, minimizing allocations and memory pressure.

### Cross-Crate Transparency

A key architectural challenge is handling re-exports and cross-crate references. For example, `std::vec::Vec` is actually defined in the `alloc` crate and re-exported by `std`. The architecture makes this transparent to users and most application code through automatic crate loading and smart reference traversal.

### Runtime Model

**CLI mode** is single-threaded; blocking operations occur on the main thread. **Interactive TUI mode** uses scoped threads for parallelism: a request thread owns `Navigator` and handles documentation operations, while a UI thread handles rendering and input. Channel-based communication maintains the zero-copy borrowing architecture across thread boundaries (`Navigator` and `DocRef` are `Send + Sync`).

Errors are handled via `Option` types with fail-fast or skip semantics—nonexistent crates and load failures are not distinguished.

---

# ferritin-common

The common library provides the core functionality for loading, caching, navigating, and searching Rust documentation.

## Navigator - Central Orchestrator

The `Navigator` is the main entry point for all documentation operations. It coordinates between multiple data sources and manages in-memory caching.

### Data Sources

Three sources provide documentation in priority order:

1. **StdSource** - Standard library crates (std, core, alloc) from rustup's `rust-docs-json` component
2. **LocalSource** - Workspace crates and dependencies, built on demand with nightly toolchain
3. **DocsRsSource** - Published crates fetched from docs.rs and cached locally

Each source implements the `Source` trait, providing name canonicalization, metadata lookup, and crate loading.

### Navigator Lifecycle

A `Navigator` instance is created:
- **CLI mode:** Once per command invocation (discarded after rendering)
- **TUI mode:** Once at startup, persists for the entire interactive session

This explains why the single-version-per-crate limitation (described below) is tolerable in practice: CLI invocations are short-lived, and TUI sessions rarely need conflicting versions of the same crate.

### Source Fallthrough & Two-Phase Resolution

When loading a crate (e.g., `tokio` or `tokio@1.40`), Navigator performs two-phase resolution:

**Phase 1: Lookup metadata (CrateInfo)**

Sources are tried in priority order until one returns a version match:
1. StdSource (if crate name matches std/core/alloc)
2. LocalSource (if present and has a matching version)
3. DocsRsSource (if present)

Each source's `lookup` method checks if it can satisfy the `VersionReq` and returns `CrateInfo` containing:
- Resolved crate name (canonicalized)
- Resolved version (e.g., `tokio@1.40` from request `tokio@1`)
- Provenance (Std, Workspace, LocalDependency, DocsRs)
- Description and metadata

If a source has the crate but not a matching version, it returns `None` and the next source is tried. For example, if LocalSource has `tokio 1.39` and the request is `tokio@1.40`, it falls through to DocsRsSource.

**Phase 2: Load documentation (RustdocData)**

Using the resolved `CrateInfo`, Navigator calls the appropriate source's `load` method to fetch/build the actual rustdoc JSON and parse it into `RustdocData`.

**Why two phases?** Separating metadata lookup from data loading allows:
- Fast version resolution without parsing large JSON files
- Source precedence (prefer local over docs.rs when versions match)
- Metadata-only operations (like `list_available_crates`)

### In-Memory Cache

```rust
working_set: FrozenMap<CrateName, Box<Option<RustdocData>>>
```

This is **the only place** in ferritin-common that owns `RustdocData` instances. All `&'a RustdocData` and `DocRef<'a>` references borrow from this map. The `elsa::sync::FrozenMap` provides thread-safe interior mutability with `&self`, enabling caching without mutable borrows while supporting multi-threaded access.

**Known limitation:** The cache stores only one version of each crate per `Navigator` instance. Multiple crates with conflicting dependency versions may load the wrong version or fail. This is not currently a practical issue but noted for future consideration.

### Cross-Crate Traversal

When a crate is loaded, `Navigator` indexes its `external_crates` field, which contains `html_root_url` entries like `https://docs.rs/tokio/1.0.0`. These are parsed to extract real crate names and exact version numbers, stored in:

```rust
external_crate_names: FrozenMap<CrateName, Box<ExternalCrateInfo>>
```

When resolving an item reference to an external crate:
1. Check if the external crate is already loaded in `working_set`
2. If not, look up in `external_crate_names` to get real name and version
3. Load the external crate (which caches it in `working_set`)
4. Return the item from the external crate

This makes viewing `std::vec::Vec` automatically load the `alloc` crate transparently.

### Path Resolution

The `resolve_path` method handles user-provided paths like:
- Standard: `std::vec::Vec`
- Version-pinned: `tokio@1::runtime::Runtime`
- Partial: `Vec` (searches within current crate)

It walks the item tree recursively and generates fuzzy suggestions if the path is not found.

## DocRef<'a, T> - Smart Reference

```rust
pub struct DocRef<'a, T> {
    crate_docs: &'a RustdocData,
    item: &'a T,
    navigator: &'a Navigator,
    name: Option<&'a str>,  // For renamed imports
}
```

`DocRef` is a lightweight (Copy) wrapper that carries context alongside item references:
- Which crate the item comes from
- Access to the `Navigator` for cross-crate traversal
- Optional name override for renamed imports

It derefs to the inner item for convenience. The presence of `navigator` enables cross-crate operations without requiring mutable state or re-borrowing.

## RustdocData - Per-Crate Documentation

`RustdocData` wraps `rustdoc_types::Crate` with convenience methods. The key method is `traverse_to_crate_by_id`:

```rust
pub fn traverse_to_crate_by_id(&self, navigator: &Navigator, crate_id: u32)
    -> Option<&RustdocData>
```

When an `Id` references an item in another crate (indicated by `crate_id` in the item's `ItemSummary`):
1. If `crate_id == 0`, return self (same crate)
2. Look up `external_crates[crate_id]` to get the external crate info
3. Parse `html_root_url` to extract real name and version
4. Call `Navigator::load_crate` to load the external crate (cached)
5. Navigate within the external crate to find the item

This is how re-exports work transparently. Viewing documentation for an item automatically loads any external crates needed to resolve its definition, completely invisible to calling code.

## Data Sources & Disk Caching

### DocsRs Caching Strategy

Documentation fetched from docs.rs is cached at:
```
$CARGO_HOME/rustdoc-json/{format-version}/{crate}/{version}.json
```

**Multi-version support:**
- Supports rustdoc JSON format versions 55-57
- Fetches zstd-compressed JSON from docs.rs
- Stores raw JSON indexed by source format version (not normalized)
- On read, normalizes to current format version (v57) via conversions module
- Tries format versions in descending order when fetching (prefers newer)

**Version resolution:**
- Queries crates.io API for crate metadata and available versions
- Matches against semver version requirements
- Extracts version numbers from `html_root_url` in external_crates for precise dependency versions

### Format Version Normalization

The `conversions` module chains format conversions to normalize older rustdoc JSON formats to the current version on read. This allows caching older format JSON and avoiding re-fetches when normalization logic changes.

## Search - Lazy TF-IDF Indexing

### Index Building

Search indices are built lazily on first search and cached as `.index` files (rkyv binary format) alongside the JSON:

1. Walk the item tree using the `ChildItems` iterator
2. Tokenize item names (2x weight) and documentation (1x weight)
3. Handle CamelCase, snake_case, kebab-case by splitting into subwords
4. Store shortest path (as ID sequence) to each indexed item
5. Check mtime to invalidate stale indices

### Tokenization & Scoring

Tokenization handles CamelCase, snake_case, and kebab-case splitting. Scoring uses standard TF-IDF with additive combination for multi-term queries.

## Item Traversal - Transparent Re-export Handling

The `iterators` module provides smart iterators that handle re-exports and imports transparently. This is a key part of making cross-crate references invisible.

### ChildItems Iterator

Returns appropriate items based on item type:
- Module → module items
- Enum → variants + methods
- Struct → methods
- Use → follows to source and returns source children

**Key feature:** When encountering a `Use` item (re-export or glob import):
1. Resolve the source (by Id if same crate, or by path string for cross-crate)
2. For glob imports (`pub use foo::*`), recursively expand to iterate all source items
3. For regular imports, return source item with the imported/renamed name
4. Chain through multiple layers of re-exports

**Why this matters:** Module children appear to "just work" even when they're re-exports from other crates. The iterator transparently loads external crates and follows import chains, making `std::vec::Vec` (re-exported from `alloc`) appear as a natural child of the `std::vec` module.

### IdIter

Iterates a list of `Id`s, but handles `Use` items specially:
- Supports `include_use` flag for search indexing (to index use statements themselves)
- Automatically expands glob imports when encountered
- Follows import chains to resolve to the actual item

### MethodIter & TraitIter

These scan the entire crate index to find `impl` blocks:
- **MethodIter** finds inherent impls (no trait) targeting an item
- **TraitIter** finds trait impls targeting an item

This is necessary because impl blocks are stored flat in the crate index, not as children of the type they implement.

---

# ferritin

The ferritin binary provides terminal-based documentation viewing with both single-shot CLI and interactive TUI modes.

## Two-Stage Rendering Architecture

The architecture separates content generation from presentation through an intermediate representation (IR).

### Stage 1: Format to IR

Format functions (`format_struct`, `format_module`, etc.) convert rustdoc JSON to a structured IR:

```rust
Document<'a> {
    nodes: Vec<DocumentNode<'a>>
}
```

The IR is a relatively flat tree structure (not deeply nested like HTML). Nodes represent semantic block-level elements (headings, sections, lists, code blocks, tables) with leaf nodes being styled text spans. This structure is renderer-agnostic and supports both presentation (plain text, colored terminal) and interaction (clickable links, expandable sections).

**Leaf nodes (Span):**
```rust
Span<'a> {
    text: Cow<'a, str>,           // Borrows from JSON where possible
    style: SpanStyle,              // Semantic styling (Keyword, TypeName, etc.)
    action: Option<TuiAction<'a>>  // Interactive action (Navigate, ExpandBlock, OpenUrl)
}
```

The `SpanStyle` enum represents semantic categories (Keyword, TypeName, FunctionName, etc.), not terminal colors. This makes the IR renderer-agnostic. The IR also supports conditional nodes that appear only in specific modes (interactive vs. non-interactive), enabling formatters to prepare mode-specific content.

### Stage 2: Render IR to Output

Four distinct renderers transform the same IR:

1. **Plain** - Plain text output (no colors, no interactivity)
2. **TTY** - Single-shot CLI with colors and OSC8 hyperlinks
3. **TestMode** - Normalized output for snapshot testing
4. **Interactive** - ratatui-based TUI with mouse/keyboard navigation

**Renderer differences:**
- **Styling:** Plain ignores SpanStyle; TTY/Interactive map to terminal colors; TestMode normalizes
- **Actions:** Plain/TestMode ignore TuiActions; TTY renders OSC8 hyperlinks; Interactive makes clickable regions
- **Truncation:** Each interprets TruncationLevel hints differently (SingleLine, Brief, Full)
- **Layout:** Plain/TTY stream to stdout; Interactive uses ratatui for scrolling/paging

Example - Plain renderer handles truncation:
- **SingleLine:** Render until first newline, append `[...]`
- **Brief:** Render until first paragraph break, show `[+N more paragraphs]`
- **Full:** Render everything

### Format Context & Render Context

The architecture separates formatting concerns (what to include in a `Document`) from rendering concerns (how to display it). `FormatContext` holds thread-safe formatting preferences (source inclusion, recursion) that can be mutated at runtime. `RenderContext` holds immutable display configuration (colors, terminal width, output mode, themes) used by renderers.

## Intra-doc Link Resolution

A subtle challenge: real-world documentation contains multiple link formats due to evolution of rustdoc's link system.

### Link Format Variations

1. **Modern intra-doc links:** `[Vec]`, `[std::vec::Vec]`
2. **Older relative HTML links:** `task/index.html`, `macro.trace.html`
3. **Quirk:** Links in rustdoc's `links` map may have backticks or not: `HashMap` vs `` `HashMap` ``

### Resolution Strategy (`extract_link_target`)

```rust
fn extract_link_target(origin: DocRef<Item>, url: &str)
    -> Option<(String, LinkTarget)>
```

**Returns:** Absolute docs.rs URL + LinkTarget (either resolved DocRef or unresolved path string)

**Algorithm:**

1. **Check if external URL or fragment** → Keep as-is

2. **If relative HTML path** (`.html` suffix or contains `/`):
   - Parse to item path (e.g., `task/index.html` → `tokio::task`)
   - Convert to absolute docs.rs URL

3. **For intra-doc links:**
   - Look up in `origin.links` map (try both with and without backticks)
   - If same crate: return resolved `DocRef` (fast path, no loading)
   - If external crate: extract path from `ItemSummary` without loading the crate
   - Use `html_root_url` to generate accurate docs.rs URL

4. **Fallback:**
   - Handle `crate::`/`self::` prefixes
   - Generate search URL

**Key insight:** We avoid loading external crates during link resolution. Same-crate links get resolved `DocRef`s for instant navigation. External links become path strings with accurate docs.rs URLs that lazily resolve when clicked in the TUI.

## Commands

Three main commands, each returning `(Document, is_error, HistoryEntry)`:

### get

Thin wrapper around `Navigator::resolve_path`:
1. Resolve the path string to a `DocRef<Item>`
2. Call `format_item` to generate IR
3. On failure, show "did you mean" suggestions

### list

Lists available crates from all sources:
1. Call `Navigator::list_available_crates`
2. Sort by name
3. Format as list with version info and descriptions
4. Show usage hints if no local project

### search

Multi-crate search with score-based ranking:

1. **Determine crates to search:**
   - If `--crate` specified: search single crate
   - Otherwise: all crates from local/std sources

2. **Build result set:**
   - For each crate: `SearchIndex::load_or_build` (skips on failure)
   - Search each index: returns `(id_path, score)`
   - Collect all results as `(crate_name, id_path, score)` tuples

3. **Global ranking:**
   - Sort all results by score descending across all crates
   - This enables finding the best match regardless of which crate it's in

4. **Early stopping:**
   - Results are filtered by score drop-off thresholds to show only relevant matches

5. **Resolve items:**
   - Use `Navigator::get_item_from_id_path` to get `DocRef`
   - Show doc preview (first 2 lines)

## Markdown Rendering

Markdown documentation (from doc comments) is parsed with pulldown_cmark and transformed into the same `Document` IR as generated content. Events are processed with state flags (`in_strong`, `in_emphasis`) to create styled `Span`s. A `link_resolver` callback enables same-crate links to become clickable `DocRef`s. The result is indistinguishable from programmatically generated content—both go through the same rendering pipeline.

## Interactive TUI

The TUI mode (`ferritin -i`) uses scoped threads to maintain UI responsiveness during expensive operations. A request thread (main) owns `Navigator` and processes documentation commands, while a spawned UI thread handles rendering (ratatui + crossterm) and input. Channel-based communication passes commands and formatted `Document<'a>` results between threads. Because both threads operate within the scoped lifetime, `Document<'a>` can safely borrow from `Navigator` across thread boundaries, preserving the zero-copy architecture.

## Testing

Both ferritin and rustdoc-mcp use insta snapshot tests to catch regressions in output formatting and structure. The TestMode renderer produces normalized output suitable for diffing.

---

## Summary

The ferritin architecture achieves its goals through several key design choices:

1. **Zero-copy borrowing** from a single source of truth (`Navigator`'s `working_set`)
2. **Transparent cross-crate traversal** via `DocRef` and automatic crate loading
3. **Smart iterators** that hide re-export complexity
4. **Two-stage rendering** separating content from presentation
5. **Lazy indexing** for fast search with disk caching
6. **Format version normalization** for long-term cache compatibility
7. **Scoped threading** for responsive TUI without sacrificing zero-copy architecture

The architecture is designed to feel instant despite working with large documentation datasets, by caching aggressively (both in-memory and on disk) and borrowing rather than copying wherever possible. The multi-threaded interactive mode maintains this efficiency while keeping the UI responsive during expensive operations.
