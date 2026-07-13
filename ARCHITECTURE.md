# Architecture Overview

This document describes the architecture of the ferritin project, a terminal-based Rust documentation viewer and its supporting libraries.

## Project Structure

The workspace contains three crates:

- **ferritin-common**: Shared library for rustdoc navigation, search, and data management
- **ferritin**: Terminal documentation viewer with CLI and TUI frontends
- **rustdoc-mcp**: MCP server providing Rust documentation access for Claude Code and other MCP clients

This document focuses primarily on ferritin-common and ferritin, as rustdoc-mcp is intended to eventually become a thin layer on top of ferritin.

### Cargo Features

The `ferritin` crate ships the CLI and TUI by default; everything web is opt-in.

- **`serve`** (off by default) gates `ferritin serve` in its entirety: the `serve` and `typeahead` modules, the `Serve` subcommand, the typeahead JSON DTOs, and the trillium *server* stack (router, caching headers, static-compiled assets), plus rayon, querystrong, and the `crate-names` reader. It is not a default because most users want a documentation CLI, not an HTTP server. Distributed binaries (`dist`) are built with default features and therefore have no server in them.

  Note that the trillium *client* stack and rustls are **not** serve-only — `ferritin-common` needs them to fetch rustdoc JSON from docs.rs — so they are in every build regardless. The feature's dependency delta is the ~20 crates of the server side.

  **Web client packaging.** `ferritin/client` is a *symlink* to the workspace-root `client/`, and `frontend!("client")` deliberately points through it rather than at `../client`: cargo cannot package files above the package root, so an escaping path would leave the published crate unable to build this feature at all. `frontend!` chooses its mode by whether the target directory contains a `package.json`, and the `include` list ships only `client/dist/**/*` — never the source. Those two facts combine to give each consumer what it needs from one macro invocation:

  - *Repo build*: the client source is visible through the symlink, so the macro shells out to the client's pnpm build at macro-expansion time. Expanding the macro **is** building the web client, which is why a `serve` build from a checkout needs a node toolchain (and why CI's serve job installs one). `--features dev-proxy` instead spawns and proxies the Vite dev server, with HMR.
  - *Published tarball*: only the built `dist` is present, so the macro takes its prebuilt path and embeds those assets — `serve` compiles from crates.io with no node toolchain at all.

  The corollary is that **`client/dist` must exist on disk before `cargo publish`**, and nothing in a default build creates it: `serve` is off, so the macro that would build the client never expands, and an `include` glob matching nothing is not an error — the failure mode is a silently published crate whose `serve` feature cannot compile. `release-plz.yml` therefore builds the client explicitly before packaging. (`dist` is gitignored; an explicit `include` entry overrides that, so the tarball carries whatever was last built on disk.)

- **`acme`** (off by default, implies `serve`) is the public-deployment feature: automatic HTTPS via Let's Encrypt (trillium-acme, tls-alpn-01 challenges) plus HTTP/3 (trillium-quinn), sharing one ACME-managed certificate resolver between the TLS and QUIC listeners. It is *activated at runtime* by `FERRITIN_ACME_DOMAIN` — absent that variable, an acme-enabled binary serves cleartext exactly like a plain `serve` build, so the deployed binary still runs locally. When active, the server binds TLS on tcp/443 and QUIC on udp/443 (the listener builder auto-advertises `alt-svc: h3` for the matching pair) plus a tcp/80 redirect to the canonical https authority. Configuration is env-var-based (12-factor, matching `HOST`/`PORT`): `FERRITIN_ACME_CACHE_DIR` (required — persists the account key and certificates, since Let's Encrypt rate-limits re-issuance), `FERRITIN_ACME_CONTACT` (optional), and `FERRITIN_ACME_PRODUCTION` (defaults to the staging directory, so a fresh deployment proves itself against staging before flipping to production).

- **`schema`** (off by default, implies `serve`) adds the `schema` subcommand, which emits the OpenAPI document for the JSON API to `assets/openapi.json`. It implies `serve` because it describes that server's endpoints and their models. The generated schema is committed, so regenerating it is a dev task rather than part of any normal build.

- **`dev-proxy`** (off by default, implies `serve`) makes the frontend handler spawn and proxy the Vite dev server (with HMR) instead of embedding assets built at compile time.

## Core Design Principles

### Zero-Copy Architecture

Throughout the codebase, data is borrowed rather than copied. Crate data lives in a long-lived, shared `Store` behind `Arc`s; each query's `Navigator` clones those `Arc`s into an append-only pin map (`FrozenMap<CrateName, Arc<RustdocData>>`), and all references (`&'a RustdocData`, `DocRef<'a>`) borrow from that map — `'a` means "this query". Because the pin map never removes entries and `Arc` is `StableDeref`, every borrow handed out stays valid for the Navigator's lifetime even if the Store evicts the entry; the pin keeps the data alive until the query ends. String data uses `Cow<'a, str>` to borrow wherever possible, minimizing allocations and memory pressure.

The per-item data those references point at is *materialized lazily* from a memory-mapped rkyv archive (see [Sparse Storage](#rustdocdata---per-crate-documentation) and [rkyv Sidecar Cache](#rkyv-sidecar-cache)) and cached in an append-only `FrozenMap<Id, Box<Item>>`, so a lookup that touches one item out of a huge crate (e.g. one type from the 61 MB `core`) does not pay to parse the whole thing. The `FrozenMap` hands out stable addresses, so `&'a Item` borrows stay valid as more items are materialized — the same interior-mutability trick the `working_set` itself uses.

### Cross-Crate Transparency

A key architectural challenge is handling re-exports and cross-crate references. For example, `std::vec::Vec` is actually defined in the `alloc` crate and re-exported by `std`. The architecture makes this transparent to users and most application code through automatic crate loading and smart reference traversal.

### Runtime Model

**CLI mode** is single-threaded; blocking operations occur on the main thread. **Interactive TUI mode** uses scoped threads for parallelism: a request thread owns `Navigator` and handles documentation operations, while a UI thread handles rendering and input. Channel-based communication maintains the zero-copy borrowing architecture across thread boundaries (`Navigator` and `DocRef` are `Send + Sync`). **Serve mode** shares one `Arc<Store>` across requests and builds a short-lived `Navigator` per request on a big-stack rayon worker, so nothing borrowed crosses an `.await` and Store eviction never invalidates an in-flight request.

One serve-mode endpoint stands apart from the Store/Navigator machinery: `/api/typeahead` (crate-name completion) is answered by `TypeaheadService`, which lazily fetches the daily [crate-names](https://github.com/jbr/crate-names) artifact (~2 MB zstd TSV of every crates.io name/default-version/rank) and queries it in memory via the sans-io `crate-names` reader. Freshness is stale-while-revalidate — hourly conditional GETs, one revalidator at a time, brief failure cooldown — so queries never block on the network once data is loaded, and a cold offline server degrades to fast 503s.

Query-level errors are handled via `Option` types with fail-fast or skip semantics. At the Store layer, definitive absence (no such crate, no rustdoc JSON for a release) and transient failures (network errors) are distinguished and negatively cached with different TTLs; the `Source` trait encodes the distinction as `Ok(None)` versus `Err`.

---

# ferritin-common

The common library provides the core functionality for loading, caching, navigating, and searching Rust documentation.

## Store & Navigator - Central Orchestrators

Documentation access is split between two types:

- **`Store`** (long-lived, shared): owns the source backends and the in-memory caches — resolution (which exact version a request means), crate data, and search indexes. Where it is shared — serve mode, potentially many Navigators — it sits behind an `Arc`.
- **`Navigator`** (per-query): the entry point for documentation operations. It resolves through the Store and pins everything the query touches in its own append-only maps, so borrows are stable for the query's lifetime.

### Data Sources

Three sources provide documentation:

1. **StdSource** - Standard library crates (std, core, alloc) from rustup's `rust-docs-json` component
2. **LocalSource** - Workspace crates and dependencies, built on demand with nightly toolchain. Workspace crates are rebuilt when their JSON is older than any `src/` file; dependencies when the cached format/crate version no longer matches. The CLI `--rebuild` flag forces a one-shot rebuild of the first crate loaded (the queried one), bypassing those checks — useful when cached docs go stale across branch switches.

   **Feature selection (`--features`/`--all-features`/`--no-default-features`):** local-only (docs.rs builds are not under our control), these pass through to the `cargo doc` invocation. Because cargo writes to a fixed path (`target/doc/{crate}.json`) regardless of features, the feature selection a cached JSON was built with is recorded as *build provenance* in `target/ferritin.json` (see `workspace_metadata`). The model is **sticky**: passing features rebuilds with exactly those (skipping the rebuild when they already match the recorded selection), while a bare invocation inherits the recorded selection rather than reverting to default — so you type `--features` once and later lookups (and mtime-triggered rebuilds during active development) keep them. `--rebuild` is the escape hatch: it forces a clean build at the *requested* selection, or plain default if none were given. Like `--rebuild`, a requested selection is one-shot — it applies to the queried crate, not to cross-crate dependencies loaded afterward, which keep their own recorded selections.
3. **DocsRsSource** - Published crates fetched from docs.rs and cached locally

Each source implements the `Source` trait, providing name canonicalization, metadata lookup, and crate loading.

### Navigator Lifecycle

A `Navigator` instance is created:
- **CLI mode:** Once per command invocation, over its own `Store` (both discarded after rendering)
- **TUI and MCP modes:** Once at startup, persists for the entire session — everything the session touches stays pinned (status-quo memory behavior; per-view scopes are deferred until history holds owned paths instead of `DocRef`s)
- **Serve mode:** Once per HTTP request, over the shared `Arc<Store>`

This explains why the per-query one-version-per-name pinning (described below) is tolerable in practice: CLI invocations are short-lived, and TUI sessions rarely need conflicting versions of the same crate.

### Source Fallthrough & Two-Phase Resolution

When loading a crate (e.g., `tokio` or `tokio@1.40`), the Store performs two-phase resolution, each phase behind its own cache (see In-Memory Cache below). An exact request (`=1.2.3` — what cross-crate traversal produces) skips phase 1 entirely: the version *is* the data-cache key, and absence surfaces at load time.

**Phase 1: Lookup metadata (CrateInfo)**

Sources are tried in priority order until one returns a version match:
1. StdSource (if crate name matches std/core/alloc)
2. LocalSource (if present and has a matching version) — only when `--local` is active
3. DocsRsSource (if present) — active by default; disabled when `--local` is set

Each source's `lookup` method checks if it can satisfy the `VersionReq` and returns `CrateInfo` containing:
- Resolved crate name (canonicalized)
- Resolved version (e.g., `tokio@1.40` from request `tokio@1`)
- Provenance (Std, Workspace, LocalDependency, DocsRs)
- Description and metadata

If a source has the crate but not a matching version, it returns `None` and the next source is tried. For example, if LocalSource has `tokio 1.39` and the request is `tokio@1.40`, it falls through to DocsRsSource.

**Phase 2: Load documentation (RustdocData)**

Using the resolved name and exact version, the Store calls the appropriate source's `load` method (falling through all sources when the version came from an exact request and carries no provenance hint) to fetch/build the actual rustdoc JSON and parse it into `RustdocData`.

**Why two phases?** Separating metadata lookup from data loading allows:
- Fast version resolution without parsing large JSON files
- Source precedence (prefer local over docs.rs when versions match)
- Metadata-only operations (like `list_available_crates`)

### In-Memory Cache

**Store caches** (shared): three instances of one generic `Cache<K, T>` — singleflight slots plus eviction/TTL metadata under a mutex:

```rust
resolutions:    Cache<(CrateName, VersionReq), Resolved>     // Resolved { name, version, provenance }
crates:         Cache<(CrateName, Version), RustdocData>
search_indexes: Cache<(CrateName, Version), SearchIndex>
```

The resolution cache is keyed by the canonicalized *requested* name; the data caches by *resolved* name and exact version, so aliases that only resolution can collapse (the local `crate` shorthand) share one data entry, and different versions of one crate are distinct entries — the Store serves them concurrently. "Crate doesn't exist" negative-caches at the resolution layer; "no rustdoc JSON for this release" at the data layer.

- **Singleflight:** concurrent loaders of the same key block on the slot's `OnceLock::get_or_init` and share the winner's result, so a cold crate is loaded once no matter how many requests race for it.
- **Negative caching with TTL:** failures are cached as `LoadFailure::NotAvailable` (definitive; long TTL) or `LoadFailure::Transient` (network error; short TTL). A docs.rs blip no longer poisons a crate for the process lifetime.
- **Positive TTL** (resolution cache only): what a floating req (`*`, `^1`) resolves to moves when a new version publishes, so successful resolutions expire after `RESOLUTION_TTL`. Crate data for a released version is immutable and has no positive TTL.
- **Expired-entry sweep:** an expired entry is otherwise only replaced on re-access, so a stream of never-repeated keys (a crawler walking garbage names) would grow a map forever. When a map outgrows its sweep threshold, expired entries are dropped and the threshold doubles — amortized O(1) per insert.
- **Eviction:** runs on insert when summed entry weights exceed a byte cap (weight proxy: JSON file size at load, ×`COLD_FORM_WEIGHT` for cold forms — see below; unbounded by default, serve mode sets caps via `FERRITIN_CACHE_BYTES`). The policy is deliberately dumb — least-recently-accessed first, **among entries no query currently pins** (`Arc::strong_count > 1` means a pin exists, so evicting would free no memory) — but entries record the metadata (weight, access count, timestamps) a smarter policy will use once there is real traffic data. Evicted entries are dropped outside the map lock, since dropping the last `Arc<RustdocData>` joins its sidecar write thread. Because eviction only runs on insert, a pin-heavy burst can leave the cache transiently over cap until the next load sweeps it.
- **Supersede-on-sidecar-write:** a cold load keeps the fully parsed `Crate` resident at ~4–5× the JSON size, which the weight proxy badly underestimates — a cold-heavy workload (the public server's steady state) would OOM before weight eviction fired. Caching the fat form is load-bearing only while the background sidecar write is in flight (singleflight covers a burst on a cold crate); the moment the sidecar lands, a thin mmap-lazy reload is strictly better. The write thread flags the data (`RustdocData::sidecar_written`) and the crate cache drops flagged entries opportunistically — on access to the key, on every completed load, and in the sweep — so fat cold forms *transit* memory rather than accumulate. Dropping a pinned superseded entry is fine (unlike eviction): the pin keeps the fat data alive for its query, and the drop is what lets the next query load thin. If a write fails the fat entry stays cached as before, which is why cold forms are charged the `COLD_FORM_WEIGHT` multiplier: eviction pressure finds a lingering one first.
- **RSS guard (OOM backstop):** the weight caps are proactive but built on proxies that can drift from real memory use; the reactive layer checks the process's *anonymous* RSS (Linux `RssAnon` — deliberately not total RSS, which counts the resident mmap'd sidecar pages the kernel reclaims on its own) on the load path, rate-limited to one `/proc` read per second. Over the high water, both data caches shed a quarter of their summed weight, LRA-first, then the guard cools down before looking again — RSS is the trigger but weight is the response variable, because freed pages may not return to the OS promptly and a guard that chased RSS would spiral the cache to empty. Off unless `FERRITIN_RSS_HIGH_WATER_BYTES` is set (serve mode), inert on non-Linux platforms; the outermost layer remains systemd `MemoryMax`+restart, which the node's reconstructible-cache design makes survivable.

**Navigator pin maps** (per-query):

```rust
working_set:     FrozenMap<CrateName, Arc<RustdocData>>
pinned_versions: FrozenMap<CrateName, Box<Version>>   // the data-cache key each pin was loaded at
```

All `&'a RustdocData` and `DocRef<'a>` references borrow from `working_set`. The `elsa::sync::FrozenMap` provides thread-safe interior mutability with `&self` and never removes entries, so addresses are stable within a query — `ItemKey` cycle detection and `DocRef` identity are unaffected by anything the Store does. The safety argument for eviction is entirely borrow-checker-visible: Store eviction drops an `Arc`; a query's pin keeps the data alive until the query ends. Memory bound = cache cap + in-flight pins.

The pin map stays keyed by *name*: one version per name per query, first pin wins, which is what `DocRef` identity relies on. A later request for a different exact version of an already-pinned name is served the pin (logged at debug). The Store itself has no version limit — conflicting versions across concurrent queries are simply different data-cache entries.

### Cross-Crate Traversal

Every crate's `external_crates` table records the exact versions of its entire build graph (docs.rs builds carry them in `html_root_url`s like `https://docs.rs/tokio/1.0.0` — transitive dependencies included, and real crates.io names, which can differ from internal names beyond dash/underscore folding, e.g. `sha1` → `sha-1`).

When resolving an item reference to an external crate, the version comes from a fallback chain:

1. **Entrypoint version authority:** the query's first-loaded crate (its *entrypoint* — the CLI query root, serve's request crate) is the version oracle. If its build graph pins the name (`RustdocData::built_against`, a lazy index over its `external_crates`), traversal requests exactly that version — so every crate the entrypoint knows resolves to the entrypoint's version, independent of traversal order.
2. **The referencing crate's own `html_root_url`** — for names outside the entrypoint's graph (e.g. pulled in by an intermediate crate's feature-gated deps).
3. **Latest** (`VersionReq::STAR`), when no exact version is recorded anywhere.

Exact versions become `=x.y.z` requests, which skip resolution and hit the data cache directly — traversal never touches crates.io. The loaded `Arc` is pinned in `working_set` (first pin per name wins) and the item is returned from the external crate. This makes viewing `std::vec::Vec` automatically load the `alloc` crate transparently.

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
    name: Option<&'a str>,        // For renamed imports
    visibility: Option<&'a Visibility>,  // For re-exports (see below)
}
```

`DocRef` is a lightweight (Copy) wrapper that carries context alongside item references:
- Which crate the item comes from
- Access to the `Navigator` for cross-crate traversal
- Optional name override for renamed imports
- Optional visibility override for re-exports: an item reached through a `use`
  has the *use's* visibility, not the target's (a `pub use` of a private item is
  publicly reachable). Set during use resolution and read via
  `effective_visibility()`. This is what lets `--public` filtering keep
  publicly-re-exported items while dropping genuinely private ones. (Glob
  re-exports are best-effort — expanded items keep their own visibility.)

It derefs to the inner item for convenience. The presence of `navigator` enables cross-crate operations without requiring mutable state or re-borrowing.

**Identity is logical, not pointer-based.** `DocRef`'s `Eq` and `Hash` both key on
`(crate name, item id)` — *not* on the `&item` address. Item addresses are an
implementation detail of `RustdocData`'s storage (resident crate vs. lazily
materialized `item_cache`), and identity must not depend on which store produced a
reference; `HashSet`/`HashMap` uses keyed on `DocRef` (the recursive-listing `visited`
dedup set, the search indexer's link-count aggregation) rely on this. (Historically
the warm path really did materialize one logical item at two addresses, via a
since-removed whole-index store.) Crate name maps 1:1 to a `RustdocData` within a
`Navigator` (one version per crate name), and `id` is unique within a crate's index, so
`(crate name, id)` is a sound identity.

## RustdocData - Per-Crate Documentation

### Sparse Storage

`RustdocData` provides query methods over a rustdoc `Crate`, but does not hold one. Storage is sparse:

- **`archive: Option<Archive>`** — a memory-mapped rkyv archive (see [rkyv Sidecar Cache](#rkyv-sidecar-cache)). `Archive::krate()` is an O(1) pointer cast yielding `&ArchivedCrate`; the OS pages in only the bytes actually touched.
- **`item_cache: FrozenMap<Id, Box<Item>>`** — point lookups (`get_item`) deserialize one item from the archive on first access and cache it here with a stable address. This is the *only* warm-path item store: no code path materializes the full index, so a crate's resident footprint is bounded by the items actually touched.
- **Derived reverse indexes** (`indexes` module) — rustdoc JSON stores impl blocks flat, keyed only by their own `Id`, so "the impls targeting this type", "the impls of this trait", and "the item containing this method/variant/field" would each be whole-index scans. `DerivedIndexes` precomputes all four maps (inherent impls by type, trait impls by type, implementors by trait, child→parent) in one O(n) pass: at sidecar-write time on the cold path (looked up directly in the mapped archive thereafter, never deserialized), or into a `OnceLock` from the resident crate. Each id list is sorted, so results are deterministic rather than `FxHashMap`-iteration-ordered.

  The **child→parent** map must be fed from every kind that *owns* items, not just impls: a trait's members live in `Trait::items` and a union's fields in `Union::fields`, neither of which the `Impl` arm ever sees. Missing them is not a cosmetic gap — an associated item with no parent has no page of its own to fall back on, so it loses *both* its in-app path (`item_nav_path` walks `parent_item()`) and its URL fragment (`generate_docsrs_url` hangs `#method.{name}` off the parent), leaving intra-doc links to a trait's own methods pointing at the bare crate root. Blanket-impl members are the one deliberate omission (see below).
- **Eager small maps** — `paths` (`ItemSummary`), `external_crates`, `root`, and `crate_version` are materialized up front (they are small and consulted constantly for cross-crate and link resolution), so accessors can hand out borrows cheaply.

Two constructors: `from_crate` (cold path — keeps the parsed index resident and best-effort writes the sidecar) and `try_from_sidecar` (warm path — mmaps the archive, deserializes only the small maps, leaves items lazy). All access goes through accessor methods (`get_item`, `path_summary`, `root_id`, `external_crate`, `crate_version`, the impl-id accessors) rather than field access; there is deliberately no `Deref` to `Crate`, so the storage strategy stays an implementation detail.

### Cross-crate traversal

The key method is `traverse_to_crate_by_id`:

```rust
pub fn traverse_to_crate_by_id(&self, navigator: &Navigator, crate_id: u32)
    -> Option<&RustdocData>
```

When an `Id` references an item in another crate (indicated by `crate_id` in the item's `ItemSummary`):
1. If `crate_id == 0`, return self (same crate)
2. Look up `external_crates[crate_id]` to get the external crate info
3. Determine name and exact version via the fallback chain above (entrypoint authority → own `html_root_url` → latest)
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
- Supports rustdoc JSON format versions 55-60 natively, plus newer additive formats (see below)
- Fetches zstd-compressed JSON from docs.rs via the suffix-less URL
  (`.../json`), which serves whatever format the release was built with; the
  actual `format_version` is read from the JSON itself. Formats older than the
  supported floor are treated as definitive absence ("no rustdoc JSON we can
  read exists for this release"). This replaced a historical probe of
  exact-format URLs in descending order — one request instead of up to seven.
- Stores raw JSON indexed by source format version (not normalized)
- On read, normalizes to current format version (v60) via conversions module
- docs.rs/crates.io requests are currently pinned to HTTP/1.1 as a workaround
  for a timing-sensitive stall in trillium-client's h2 path (see the comment on
  `DocsRsClient::get`); remove the pin when that race is fixed upstream

**Version resolution:**
- Queries crates.io API for crate metadata and available versions
- Matches against semver version requirements
- Extracts version numbers from `html_root_url` in external_crates for precise dependency versions

### Format Version Normalization

The `conversions` module normalizes any supported rustdoc JSON format to the canonical `FORMAT_VERSION` on read. This lets us cache older-format JSON and avoid re-fetches when normalization logic changes.

Every format bump in the supported range (55..=current) has so far been **read-compatible**: each adds a field or an enum variant without removing, renaming, or retyping anything we read. Because `rustdoc-types` does not `deny_unknown_fields`, an added `Option` field defaults to `None` when absent, and an added enum variant never appears in older data, such formats deserialize *directly* into the canonical types — no per-version `rustdoc-types` crate is needed. The single exception across 55..=60 is `ExternalCrate::path`, a required `PathBuf` added in format 57; `load_and_normalize` injects an empty value into pre-57 JSON (its only structural patch) before parsing. This same additive tolerance handles formats *newer* than the `rustdoc-types` we build against: `load_and_normalize` parses them directly and surfaces a clear "needs an update" error only if a genuinely breaking change prevents it, so ferritin can read crates built with a newer docs.rs toolchain before a matching `rustdoc-types` release exists.

**If a future format makes a genuinely read-breaking change**, the additive shortcut fails for that hop. The fix is to pin a `rustdoc-types` crate for the older format, parse with it, and translate to the canonical types in `conversions` — the chained-conversion pattern this module used before the formats turned out to be uniformly additive (preserved in git history).

### rkyv Sidecar Cache

JSON parsing dominates load time — even a single-item lookup parses the whole file (e.g. ~410 ms to parse the 61 MB `core.json`). To avoid re-parsing on every invocation, the first load of a crate serializes the parsed `Crate` to an rkyv archive beside the JSON, and subsequent loads memory-map that archive instead (see [Sparse Storage](#sparse-storage)). On the warm path, `core` loads in ~7 ms instead of ~500 ms and a `std::vec::Vec` lookup drops from ~880 ms to ~190 ms wall.

The archive (`archive` module) is a purely local, derived cache — docs.rs only serves JSON:

- **Archive root.** The root is a `Sidecar` wrapper: the `Crate` plus the derived reverse indexes (see [Sparse Storage](#sparse-storage)), computed once on the background write thread so warm-path impl lookups read straight from the mapped bytes.
- **Filename tag.** The sidecar is `{json}.rkyv{N}-fmt{FORMAT_VERSION}-{arch}-{ptr_width}.rkyv`. An rkyv archive is not portable across rustdoc/rkyv layout, target architecture, or pointer width, so all of those go in the name: a foreign or stale archive simply isn't found and is regenerated. `N` (`ARCHIVE_SCHEMA`, currently 3) is bumped on any other layout-affecting change, such as the `Sidecar` root introduction — **and whenever the archived contents change meaning while the layout stays the same**, since a stale sidecar is found by name and trusted: a `DerivedIndexes` built by older logic would otherwise be read back and silently used, so a fix to how an index is *built* would never appear on any already-cached crate (this is what bumped `N` to 3).
- **Fail-safe, never authoritative.** Writes go through a temp file + atomic rename (no torn reads); freshness is checked against the JSON mtime. *Any* miss, staleness, or error falls back to parsing the JSON, which is kept as the source of truth (and for `format_version` peeking and regeneration). This best-effort fallback is why there is no feature flag — a bad or absent archive behaves exactly like the pre-archive code.
- **Cost.** Roughly doubles cache disk usage (JSON + archive) and adds a one-time serialize (~180 ms for `core`) on the first load after a cache fill.

Because the read path uses `access_unchecked` (no validation pass) over a file keyed by the layout tag and written atomically, the archived bytes are trusted as something this exact build produced.

## Search - Lazy TF-IDF Indexing

### Index Building

Search indices are built lazily on first search and cached as `.index` files (rkyv binary format) alongside the JSON:

1. Walk the item tree using the `ChildItems` iterator
2. Tokenize item names (2x weight) and documentation (1x weight)
3. Handle CamelCase, snake_case, kebab-case by splitting into subwords
4. Store shortest path (as ID sequence) to each indexed item
5. Check mtime to invalidate stale indices

### Tokenization & Scoring

Tokenization handles CamelCase, snake_case, and kebab-case splitting. Scoring uses BM25 with global statistics aggregated across all searched crates for consistent ranking.

## Item Traversal - Transparent Re-export Handling

The `iterators` module provides smart iterators that handle re-exports and imports transparently. This is a key part of making cross-crate references invisible.

### ChildItems Iterator

Returns appropriate items based on item type:
- Module → module items
- Enum → variants + methods
- Struct → methods
- Use → follows to source and returns source children

**Key feature:** When encountering a `Use` item (re-export or glob import):
1. Resolve the source — see *Resolving a `Use` target* below
2. For glob imports (`pub use foo::*`), recursively expand to iterate all source items
3. For regular imports, return source item with the imported/renamed name
4. Chain through multiple layers of re-exports

**Why this matters:** Module children appear to "just work" even when they're re-exports from other crates. The iterator transparently loads external crates and follows import chains, making `std::vec::Vec` (re-exported from `alloc`) appear as a natural child of the `std::vec` module.

### Resolving a `Use` target

A `Use` carries both a target `id` and a `source` *string* (the path as written
in Rust source). `Resolver::follow_use` resolves them in this order, and the
order matters for correctness:

1. **`id` in the local index** → a same-crate target; return it directly. Index
   membership is the definitive test for locality, so this is deterministic, not
   a guess: within one crate an `id` is either a local item (in `index`) or a
   foreign reference (in the `paths` map with a non-zero `crate_id`), never both.
2. **`id` via the `paths` map** (`Resolver::get_path`) → a cross-crate re-export.
   The summary names the owning crate and the item's *definition path*; we cross
   into that crate and resolve there.
3. **The `source` string** → last resort only (the `id` is absent, or names no
   reachable item).

The `source` string is the fallback rather than the primary key because **its
leading segment can be a local alias, not a real crate name.** rustdoc emits
`source` verbatim from the Rust source, so a renamed dependency
(`use quinn_proto as proto; pub use proto::ServerConfig`) yields
`source = "proto::ServerConfig"`. Resolving that string would try to load a
crate literally named `proto` (an unrelated crate on docs.rs) and fail; the
`use.id`, by contrast, points through the `paths` map at the real
`quinn_proto`. The fixture `tests/test-workspace/crate-b` reproduces this with a
source-level alias (`use crate_a as aliased_a`).

`get_path` resolves the cross-crate definition path through the target crate's
reverse path index (`RustdocData::lookup_definition_path`) rather than a
public-tree walk, so it reaches items defined behind a private module but
re-exported at the crate root — e.g. `quinn_proto::config::ServerConfig`, where
`config` is private. The same helper backs the search indexer's link resolution.

### IdIter

Iterates a list of `Id`s, but handles `Use` items specially:
- Supports `include_use` flag for search indexing (to index use statements themselves)
- Automatically expands glob imports when encountered
- Follows import chains to resolve to the actual item

### MethodIter & TraitIter

These walk the `impl` blocks targeting an item:
- **MethodIter** yields members of inherent impls (no trait) targeting an item
- **TraitIter** yields trait impls targeting an item

Impl blocks are stored flat in the crate index, not as children of the type they implement, so these historically scanned the entire index; they now resolve through the precomputed reverse indexes (see [Sparse Storage](#sparse-storage)) as point lookups.

---

# ferritin

The ferritin binary provides terminal-based documentation viewing with both single-shot CLI and interactive TUI modes.

## Two-Stage Rendering Architecture

The architecture separates content generation from presentation through an intermediate representation (IR).

### Stage 1: Format to IR

Item formatting is a **two-level lowering**. `Request::model_item` first resolves a `DocRef` into a *domain model* — `ItemDoc { meta, metadata_nodes, docs, body, source }`. The `meta` (`ItemMeta`: name, kind, visibility, definition path, crate) is the structured, JSON-facing view of the header; `metadata_nodes` is the *presentation* metadata node kept verbatim alongside it so terminal output stays byte-identical; `docs` is the item's own doc prose; `source` is the optional source block; and the kind-specific `body` is an `ItemBody`:

```rust
enum ItemBody<'a> {
    Struct(StructDoc<'a>),               // shape, fields, methods, trait impls
    Enum(EnumDoc<'a>),                   // variants, methods, trait impls
    Trait(TraitDoc<'a>),                 // members (required/provided), supertraits, implementors
    Module(ModuleDoc<'a>),               // flat list of children (path, kind, nav target, brief docs)
    Function(FunctionDoc<'a>),           // single signature + fn flags/returns (mirrors MethodDoc)
    TypeAlias(TypeAliasDoc<'a>),         // name + aliased type spans
    Constant(ConstantDoc<'a>),           // name + type + optional value
    Static(StaticDoc<'a>),               // name + type + value
    Macro(MacroDoc<'a>),                 // macro_rules! definition source
    Union(UnionDoc<'a>),                 // named fields, methods, trait impls (struct-like)
    AssocItem(TraitMember<'a>),          // a directly-queried trait assoc type/const
    Presentation(Vec<DocumentNode<'a>>), // the catch-all for unknown item kinds
}
```

This is the seam that lets the domain-IR migration proceed one kind at a time. Per kind, `format_<kind>` splits into `model_<kind>` (index lookups + type resolution → a structural model) and `lower_<kind>` (span assembly → presentation nodes). **Modeled so far:** `struct`, `enum`, `trait`, `module`, and `function`, plus the shared `MethodDoc` (inherent associated items, via `model_inherent_methods` — reused by struct and enum) and the structured header (`ItemMeta`). The `module` model is a flat `Vec<ModuleItem>` (each a `{ path, kind, nav target, brief docs }`) in traversal order; grouping by kind into the terminal's sections happens only in `lower_module`, so JSON ships the flat list and a client groups however it likes. The `function` model (`FunctionDoc`) mirrors `MethodDoc`'s fn-fields (`is_async`/`is_const`/`is_unsafe`/`returns`/`signature`) minus the assoc-item `kind`/`visibility` — params stay inside the `signature` span sequence, so a function and a method serialize identically. The single-signature kinds `type-alias` (`TypeAliasDoc`), `const` (`ConstantDoc`), `static` (`StaticDoc`), and `macro` (`MacroDoc`) are likewise modeled: each lifts its cheap scalars (name, value expr, macro source) and keeps types as span-sequence leaves. `union` (`UnionDoc`) is a plain struct restricted to named fields, so it reuses the struct field machinery (`model_named_fields`/`lower_plain`) and renders identically but for the `union` keyword — this was the one *feature* in the series (the old formatter was a `[not yet implemented]` placeholder), landed model-first like the rest. **Trait implementations** (`TraitImplDoc`, shared by `StructDoc`/`EnumDoc`/`UnionDoc::trait_impls`, in `trait_impls.rs`) are also modeled. The IR is deliberately *richer than the terminal rendering*: it carries the impl's documented methods (as `MethodDoc`), `provided_trait_methods`, the negative/unsafe/synthetic flags, blanket source type, assoc-type bindings, and impl docs — most of which the terminal still drops.

**An impl is an edge, and only edge-specific facts belong on it.** `model_impl_items` keeps an impl's assoc-type bindings and its *documented* methods, and drops undocumented ones. A method's signature is dictated by the trait — the node the edge points at — so repeating it on every implementor is denormalization rather than documentation, and it dominated the payload (an implementor of `Iterator` cost ~18 KB, nearly all of it re-stated signatures). What is genuinely specific to the edge is kept: `type Item = u8` is a fact about this impl alone, and custom prose on an impl (or on one of its methods) exists nowhere else. This is a deliberate *documentation view* of the rustdoc data, not a lossy mirror of it, and it applies to both directions of the trait×type intersection because `model_impl_items` is shared by `trait_impls` (type → traits) and `implementors` (trait → types). The two fiddly span assemblies (the compact merged trait-ref and the full `impl<…> Trait` header) are memoized into an `ImplLeaf` at model time (the `MethodDoc.signature` precedent), so JSON serializes the structural projection while lowering only buckets (compact-non-std / compact-std / expanded) and selects. Impls are sorted by trait name in `model_trait_impls`, because the raw `traits()` order is `FxHashMap` iteration order (nondeterministic w.r.t. the crate's item set); sorting makes both terminal and JSON output stable and alphabetical. Negative impls (`impl !Send`), previously dropped, now render with a `!` prefix. A trait's **implementors** section (`ImplementorDoc`, in `trait.rs`) is modeled the same way: each implementor carries the implementing type (the impl's `for_`, bounds merged inline, as the leaf) plus the same edge-specific fields (documented methods, assoc types, flags, docs), sorted by type name. The model is **uncapped** — the 20-item cap is a *terminal* concern and lives in `lower_implementors`, because a terminal page has finite room while an API client can show the whole list. (Sorting happens before the cap, so which implementors survive it is deterministic rather than hash-order-dependent.) There is consequently no `implementorOverflow` on the wire: JSON ships every implementor, and the web client reveals the ones past its preview in place. The shared impl-item partition (`model_impl_items`) **skips methods of blanket impls** — their `Item` is shared across every implementor, so the method belongs to the blanket and its self-link can't be attributed to one implementor (blanket members are deliberately absent from the derived parent index, so their URL would be the crate-root fallback); assoc-type bindings are kept. Finally, a **directly-queried trait assoc type/const** (`get crate::SomeTrait::Item`) is modeled as a standalone `TraitMember` (`ItemBody::AssocItem`), reusing that model. With that, **every item kind is modeled** — `ItemBody::Presentation` now only catches genuinely unknown kinds (the `_` arm in `model_item_body`); the kind-by-kind migration is complete. Signature-level references (field/return types, generics, bounds) stay as span sequences — the shared "leaf" vocabulary, each span carrying its resolved `url` — while the item's own structure (fields, variants, members) becomes explicit. The hybrid rule: **structural containers, span-sequence leaves**.

The terminal renderers go through `ItemDoc::lower()` (and `format_item`, a thin wrapper over it), which produces the *presentation IR* — a relatively flat tree:

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

The five renderers below consume this presentation IR. The **JSON output** instead serializes the *domain model* directly (see [JSON output](#json-output)) — which is why a struct appears in JSON as `{ name, fields, methods }` rather than a flat code block.

### Stage 2: Render IR to Output

Five distinct renderers transform the same presentation IR:

1. **Plain** - Plain text output (no colors, no interactivity)
2. **TTY** - Single-shot CLI with colors and OSC8 hyperlinks
3. **TestMode** - Normalized output for snapshot testing
4. **Agent** - Token-efficient, markdown-flavored output for coding agents and
   other LLM consumers. Selected by `--format agent` or auto-detected from the
   `CLAUDECODE`/`GEMINI_CLI`/`CODEX_SANDBOX` env vars
5. **Interactive** - ratatui-based TUI with mouse/keyboard navigation

The output format is chosen by `--format <tty|plain|agent|json>` (overriding
autodetection); without it, ferritin picks agent format under a coding agent,
ANSI on a TTY, and plain when piped.

**Renderer differences:**
- **Styling:** Plain ignores SpanStyle; TTY/Interactive map to terminal colors; TestMode normalizes; Agent leans on markdown conventions (`#` headers, `-` bullets) instead of ANSI
- **Actions:** Plain/TestMode/Agent ignore TuiActions; TTY renders OSC8 hyperlinks; Interactive makes clickable regions
- **Truncation:** Each interprets TruncationLevel hints differently (SingleLine, Brief, Full)
- **Layout:** Plain/TTY/Agent stream to stdout; Interactive uses ratatui for scrolling/paging

Example - Plain renderer handles truncation:
- **SingleLine:** Render until first newline, append `[...]`
- **Brief:** Render until first paragraph break, show `[+N more paragraphs]`
- **Full:** Render everything

### Format Context & Render Context

The architecture separates formatting concerns (what to include in a `Document`) from rendering concerns (how to display it). `FormatContext` holds thread-safe formatting preferences (source inclusion, recursion, hiding non-public items) that can be mutated at runtime. `RenderContext` holds immutable display configuration (colors, terminal width, output mode, themes) used by renderers.

`FormatContext` also carries an optional **display predicate** (`DisplayPredicate` — a boxed `for<'a> Fn(DocRef<'a, Item>) -> bool + Send + Sync`), the erased internal contract behind item filtering. The CLI `--kind` flag (`get`/`search`) parses a typed `Kind` `ValueEnum` whose only job is to *build* such a predicate; the listing code never sees the enum, only the predicate, so other narrowing terms (name substrings, `as Trait`) can compose into the same mechanism later. It's boxed rather than a generic parameter because the closure type is unnameable and the predicate is set *after* construction (like the atomics), and lives behind a `RwLock` rather than an atomic because a closure can't be atomic. Module listings filter what's *collected* but still descend into modules, so `--kind fn --recursive` reaches nested functions without listing the intervening modules.

`FormatContext` also carries a `DocLevel` (CLI `--docs <full|brief|none>`, `get`-local, default `full`) controlling how much of the resolved item's *own* doc prose renders ahead of its body. `none` omits it — the pure-listing case (e.g. you want a module's items, not its essay); `brief` renders only the leading paragraph. It maps to the existing `TruncationLevel`, stored as an `AtomicU8` to stay atomic like the other prefs. Note this is orthogonal to `--kind`: `--kind` selects *which items* list, `--docs` controls *how much prose* precedes them.

The `public` preference (CLI `--public`) filters non-`pub` items at format time rather than at build time: workspace crates are always built with `--document-private-items`, and the formatters skip items whose `DocRef::effective_visibility()` isn't public (module children, struct fields, inherent methods). Enum variants are exempt — they carry `Visibility::Default` in rustdoc JSON but are as public as their enum.

### JSON output

`--format json` bypasses the `Document` render pipeline. For `get`, it serializes the `ItemDoc` domain model: structured `meta` (name/kind/visibility/path/crate), and a `body` that is structural for modeled kinds — `struct` → `{ shape, fields, methods, traitImpls }`, `enum` → `{ variants, methods, traitImpls }`, `trait` → `{ supertraits, members, implementors }` (every implementor, each `{ typeName, typeUrl, forType, assocTypes, methods, … }`), `module` → `{ items: [{ path, kind, url, docs }] }`, `function` → `{ name, isAsync, isConst, isUnsafe, returns, signature }`, `type-alias` → `{ name, aliased }`, `const` → `{ name, type, value? }`, `static` → `{ name, type, value }`, `macro` → `{ definition }`, `union` → `{ name, fields, methods, traitImpls }`, `assocItem` → `{ assocKind, name, signature }` (a directly-queried trait assoc type/const; `assocKind` avoids colliding with the body's `kind` tag) — or a faithful JSON mirror of presentation nodes for the rare unknown kind. Methods carry `{ kind, visibility, isAsync, returns, signature, … }`; trait members carry `hasDefault` (required vs. provided); each `traitImpls` entry is structural — `{ traitName, traitUrl, args, assocTypes, methods, providedMethods, isNegative, isUnsafe, isSynthetic, isStd, blanket, docs }` — exposing impl data the terminal omits, with `methods` holding only the impl's *documented* methods (see the edge rule above). Leaf references carry **one** navigation pointer, and the two forms are mutually exclusive. `path` — a `::`-joined item path (e.g. `trillium::Conn`) the web client routes to in-app — is emitted whenever the target resolves to an item. `url` appears *only* when it does not: a hyperlink written into the prose (a blog post, an RFC), a same-page anchor, or a link that resolved to no item. Both derive from the span's `TuiAction` (`Span::url`/`Span::nav_path`), but `json_span` drops the `url` wherever a `path` exists. This is a payload decision as much as a modeling one: the two were emitted side by side, and the redundant `url`s were ~24% of a large item's bytes (36% counting both pointers) — absolute docs.rs URLs a client that routes on `path` never follows. Measured on `core::fmt::Debug`, 705 of 706 linked spans carried both. The item's *own* upstream page is still served, once, as `canonicalUrl`. An item with its own page uses its `paths` entry directly. Associated items, variants, and fields have none — rustdoc documents them in a fragment on their parent's page — so `Span::nav_path` walks `parent_item()`, mirroring exactly how `generate_docsrs_url` hangs `#method.{name}` off the parent's URL.

**Struct fields stop at the parent** rather than appending their name, because `resolve_path` cannot reach a field; the struct is the page the field's URL names anyway, so only the `#structfield.x` anchor is lost. `DocsRsLink::parse` makes the identical choice from the other direction, declining to fold `#structfield.x` into the path (see `FRAGMENT_KINDS`) — the two must agree, or a docs.rs link to a field would resolve differently from an intra-doc one. Between them, every span pointing at a documented item now carries a `path`; only genuinely external links (blog posts, repositories) and same-page anchors fall back to `url`. Blanket-impl members are the residual gap: they have no attributable parent, so they get neither a precise URL nor a path. The item itself carries a `canonicalUrl`. The other commands also serialize structural models: a **not-found** result (when a `get` path doesn't resolve, shared with search's no-crates case) is `{ error: "notFound", query, suggestions: [{ path, kind?, url? }] }` (top-5 score-ranked candidates); **search** is `{ query, results: [{ path, kind, url, score, docs? }] }` (an empty query or no matches → `results: []`; no crates loaded → `{ error: "noCratesLoaded", suggestions }`); **list** is `{ crates: [{ name, version?, isDefault, isWorkspace, usedBy, description? }] }`. Search follows the same **model+lower** seam as item kinds — `search::model` → `SearchDoc`, `lower_search` reproduces the terminal `Document` (so the TUI is unaffected), and `execute = lower_search(model())`. `list` is a deliberately minimal **JSON-only** projection (`list::json_model`), since the command is slated for rework; its terminal `execute` is untouched. So the generic `document_to_string` (`{ nodes: [...] }`) is no longer reached by any first-class command — it remains only as a fallback.

Serialization lives in the `json` module: `#[derive(Serialize)]` DTOs (`JsonItem`/`JsonNode`/`JsonSpan`/…) that borrow (`Cow`) from the model, serialized with `sonic-rs`. `model_item` is the seam both frontends share: `--format json` calls it on the main thread, and (under the `serve` feature) each HTTP handler calls it on a rayon worker, rendering to owned bytes so the model never crosses an `.await`.

## Intra-doc Link Resolution

A subtle challenge: real-world documentation contains multiple link formats due to evolution of rustdoc's link system.

### Link Format Variations

1. **Modern intra-doc links:** `[Vec]`, `[std::vec::Vec]`
2. **Older relative HTML links:** `task/index.html`, `macro.trace.html`, `../attr.main.html`
3. **Absolute rendered-documentation URLs:** `https://docs.rs/futures-io/latest/futures_io/trait.AsyncRead.html` — written out by hand in place of an intra-doc link, both across crates and within one. Common enough in real crates to be worth resolving rather than treating as an ordinary external link.
4. **Quirk:** Links in rustdoc's `links` map may have backticks or not: `HashMap` vs `` `HashMap` ``

### Resolution Strategy (`extract_link_target`)

```rust
fn extract_link_target(origin: DocRef<'a, Item>, url: &str)
    -> Option<LinkTarget<'a>>
```

**Returns:** a `LinkTarget` — either a resolved `DocRef` or an unresolved path string paired with an optional authoritative URL. `None` means "not a link we can resolve; keep it as an external URL."

**Algorithm:**

1. **Fragment-only link** (`#method.foo`) → keep as-is

2. **Absolute URL** → try [`DocsRsLink::parse`](#docsrs_url---itemurl-in-both-directions). On success the link becomes a path *plus* the original URL, so it navigates in-app while the external pointer stays byte-exact (it is authoritative about version and anchor, and a regenerated one would not be). On failure, keep as-is.

3. **Relative HTML path** (`.html` suffix or contains `/`) → resolve against the page `origin` is *itself* rendered on. `generate_docsrs_url(origin)` names that page, its directory is the link's base, and joining the two yields an absolute URL that step 2's parser reads. This is why the two directions have to live together: resolving `../attr.main.html` in `tokio::runtime`'s docs to `tokio::main` requires knowing what URL `tokio::runtime` would be generated at.

   Relative links can only address the origin's own crate, so a link walking out of its documentation tree (`../../other_crate/…`) is broken and left as-is; and being same-crate, the resolved path carries no version qualifier. Resolving against the origin's *module* — rather than assuming the crate root — is what makes `struct.TcpStream.html` inside `tokio::net` mean `tokio::net::TcpStream`.

4. **For intra-doc links:**
   - Look up in `origin.links` map (try both with and without backticks)
   - If same crate: return resolved `DocRef` (fast path, no loading)
   - If external crate: extract path from `ItemSummary` without loading the crate

5. **Fallback:**
   - Handle `crate::`/`self::` prefixes
   - Emit a bare path; the renderer derives a heuristic search URL

**Key insight:** We avoid loading external crates during link resolution. Same-crate links get resolved `DocRef`s for instant navigation. External links become path strings that lazily resolve when clicked in the TUI, carrying a URL only when one was already known.

## `docsrs_url` - item↔URL in both directions

`generate_docsrs_url` maps a `DocRef` to the docs.rs (or doc.rust-lang.org) page documenting it; `DocsRsLink::parse` maps such a URL back to a path `Navigator::resolve_path` accepts. Both read one `SIGILS` table — the `{sigil}.{Name}.html` filename vocabulary rustdoc emits — so the two directions cannot drift. (They did: all three proc-macro kinds once generated `macro.{name}.html`, which 404s for attributes and derives.)

### Package name vs. library name

A URL's shape is `docs.rs/{crate_name}/{version}/{target}?/{lib_name}/…`, and the two names are not interchangeable:

- **`RustdocData::name`** is the *package* name, a field each `Source` fills in with whatever it resolved (`futures-io`). It is what the Store is keyed by and what the version qualifies, so it belongs in the URL prefix.
- **`RustdocData::lib_name`** is the *library* name, the Rust identifier rustdoc writes into every item path (`futures_io`), read from the eagerly-materialized `paths` map at the root. It is the directory rustdoc roots its output at, so it belongs after the version.

They coincide for std, fold together under dash/underscore for most crates, and genuinely diverge when a crate declares an explicit `[lib] name` — `sha-1` vs `sha1`, where `sha1` is an *unrelated* crate on crates.io. rustdoc JSON records only the library name (in `paths`, in `index[root].name`, and in each `external_crates[i].name`); the package name appears nowhere as a field, and is recoverable for *external* crates only by parsing it back out of an `html_root_url` — which is what `store::parse_docsrs_url` does for cross-crate version resolution. A crate's own package name is therefore knowable only from the `Source` that loaded it.

Generation uses `name` for the prefix and `lib_name` for everything after it; parsing does the inverse, keeping the slug and dropping the library-name segment. Emitting one where the other belongs 404s.

Parsing is otherwise lossy in one direction only, and deliberately:

- **An optional target triple precedes the library name** for crates documented on several targets. A triple always contains a hyphen and a library name — a Rust identifier — never can, which tells them apart unambiguously.
- **`latest` becomes no version at all**, since an absent version requirement means the same thing to the resolver.
- **Anchors naming a child item are folded into the path**, so `struct.Runtime.html#method.spawn` is the single lookup path `tokio::runtime::Runtime::spawn` and the parsed kind describes the child. Anchors naming no item (`#impl-Display-for-Foo`, the disambiguated `#method.spawn-1`) are kept verbatim.
- **URLs addressing no single item return `None`** — `all.html`, source listings, docs.rs site routes, and the hand-written prose at `doc.rust-lang.org/book/`.

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

Multi-crate search with BM25 scoring:

1. Determines crates to search (single crate if specified, or all from local/std sources)
2. Calls `Navigator::search()` which parallelizes index loading and searching
3. BM25 scorer aggregates global statistics (document frequencies, average document length) across all crates for consistent cross-crate ranking
4. Results sorted by BM25 score descending, with early stopping thresholds
5. Resolves items via `Navigator::get_item_from_id_path` and shows doc preview

### Agent skill (`skills/SKILL.md`)

`skills/SKILL.md` is a Claude Code [Agent Skill](https://code.claude.com/docs/en/skills)
documenting the CLI surface for coding agents, so they reach for `ferritin`
to look up Rust APIs instead of guessing. Its `description` (a semantic
trigger) and `paths` globs (Rust-context scoping) are what make ferritin
discoverable to an agent without an MCP server.

It is consumed two ways: symlinked into `~/.claude/skills/ferritin` for local
dogfooding, and (eventually) `include_str!`'d into the binary by a generator
subcommand that installs it into other projects. The body is deliberately
*hybrid* — it pins a stable core of examples but defers churny flags to
`ferritin <cmd> --help` — so most CLI tweaks need no edit here.

**When you change the CLI surface (commands or their stable flags), update
`skills/SKILL.md` to match.**

## Markdown Rendering

Markdown documentation (from doc comments) is parsed with pulldown_cmark and transformed into the same `Document` IR as generated content. Events are processed with state flags (`in_strong`, `in_emphasis`) to create styled `Span`s. A `link_resolver` callback enables same-crate links to become clickable `DocRef`s. The result is indistinguishable from programmatically generated content—both go through the same rendering pipeline.

### Fenced code blocks

A fenced block's info string is parsed once at markdown-parse time into a *grammar language* (the syntect syntax to highlight with — rustdoc doctest tags like `should_panic`/`no_run`/`edition2021` collapse to `rust`) and a small set of *reader attributes* — currently just `should_panic` and `compile_fail`, the tags that are positive assertions the example is a deliberate counterexample. Both live on `DocumentNode::CodeBlock { lang, code, attrs }`; the terminal renderers ignore `attrs`.

Highlighting itself is split by output. The terminal renderers highlight at *render* time (syntect → theme RGB). The **JSON path** instead highlights at *serialization* time (`highlight` module): `json.rs` runs syntect's parser over the block and collapses each token's TextMate scope stack into a small fixed *lexical* vocabulary (`keyword`, `type`, `string`, `comment`, …), emitting class-tagged spans that tile the source (concatenating their `text` reconstructs the code — there is no separate `code` field on the wire, and a block whose language has no grammar degrades to one unclassed span). This vocabulary is deliberately distinct from `SpanStyle`: those spans are semantic and navigable (a `TypeName` carries a resolve link), while code-block classes are purely lexical highlights the web client colors with its own light/dark CSS (sharing the palette, not the meaning). The scope→class mapping mirrors the fallback chains `ColorScheme` uses in the other direction. `should_panic`/`compile_fail` render as a prominent banner on the block.

## Interactive TUI

The TUI mode (`ferritin -i`) uses scoped threads to maintain UI responsiveness during expensive operations. A request thread (main) owns `Navigator` and processes documentation commands, while a spawned UI thread handles rendering (ratatui + crossterm) and input. Channel-based communication passes commands and formatted `Document<'a>` results between threads. Because both threads operate within the scoped lifetime, `Document<'a>` can safely borrow from `Navigator` across thread boundaries, preserving the zero-copy architecture.

## Testing

Both ferritin and rustdoc-mcp use insta snapshot tests to catch regressions in output formatting and structure. The TestMode renderer produces normalized output suitable for diffing.

---

## Summary

The ferritin architecture achieves its goals through several key design choices:

1. **Zero-copy borrowing** from per-query pins (`Navigator`'s `working_set`) over a shared, evictable `Store`
2. **Transparent cross-crate traversal** via `DocRef` and automatic crate loading
3. **Smart iterators** that hide re-export complexity
4. **Two-stage rendering** separating content from presentation
5. **Lazy indexing** for fast search with disk caching
6. **Format version normalization** for long-term cache compatibility
7. **Scoped threading** for responsive TUI without sacrificing zero-copy architecture

The architecture is designed to feel instant despite working with large documentation datasets, by caching aggressively (both in-memory and on disk) and borrowing rather than copying wherever possible. The multi-threaded interactive mode maintains this efficiency while keeping the UI responsive during expensive operations.
