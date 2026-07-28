# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.17.0](https://github.com/jbr/ferritin/compare/ferritin-common-v0.16.1...ferritin-common-v0.17.0) - 2026-07-28

### Added

- *(serve)* add og tags

## [0.16.1](https://github.com/jbr/ferritin/compare/ferritin-common-v0.16.0...ferritin-common-v0.16.1) - 2026-07-18

### Other

- migrate storage paths

## [0.16.0](https://github.com/jbr/ferritin/compare/ferritin-common-v0.15.0...ferritin-common-v0.16.0) - 2026-07-17

### Added

- move crate-names lookup off of the request path, do it no more frequently than every 24h
- typeahead fuzziness and fix crate-not-found
- add support for older json formats (now back to 48)

### Fixed

- special-case None-id primitives so they don't fall through to crate resolution

## [0.15.0] - 2026-07-16

### Added

- `Store`: the long-lived owner of the sources and the caches, and the type the
  source builders now live on. A `Store` is shared — behind an `Arc`, by as many
  `Navigator`s as you like, concurrently — and its caches are keyed by resolved
  name *and* exact version, so it holds conflicting versions of one crate at
  once and loads any given crate only once no matter how many callers race for
  it.
- `Store::with_weight_cap`, `with_search_weight_cap`, and `with_rss_high_water`
  bound the caches, by summed entry weight and by process RSS respectively. All
  three are unbounded by default, which is the previous behavior: nothing is
  ever evicted unless you ask for a cap. Eviction never invalidates a borrow —
  a `Navigator` pins everything its query touches, and a pinned entry outlives
  its eviction.
- `LoadFailure`, distinguishing a crate the sources definitively do not have
  from one that failed transiently.
- `CrateIndex` and `CrateEntry` (the `crate_names` module): a local copy of the
  crates.io name→version table, answering version resolution and name
  completion without a request per crate. It fetches ~7.5 MB of compressed
  artifacts into a `crate-names` subdirectory of the docs.rs cache dir on first
  query and revalidates hourly; `FERRITIN_CRATE_NAMES_URL` and
  `FERRITIN_CRATE_DESCRIPTIONS_URL` point it at a mirror.
- `CratePath`, the parsed form of a `name@req` crate specifier, so callers can
  identify the crate a request names without loading it.
- `Navigator::new`, `Navigator::store`, `Navigator::local_source`, and
  `Navigator::pinned_version` (the exact version a name was pinned at).
- `DocRef::parent_item`, the item a member is documented under.
- `RustdocData::lib_name`, the library name rustdoc writes into item paths
  (`sha1`) — distinct from `RustdocData::name`, the package name the version
  qualifies (`sha-1`). They diverge whenever a crate declares an explicit
  `[lib] name`, and mixing them up produces URLs that 404.
- `FERRITIN_USER_AGENT`, the `User-Agent` every ferritin http client sends.
- Reads rustdoc JSON format version 60.

### Changed

- **`Navigator` no longer owns sources or caches**; it is now a per-query view
  over a `Store`, holding pins that keep its own borrows valid. The source
  builders moved with the ownership, so
  `Navigator::default().with_std_source(s)` becomes
  `Navigator::new(Arc::new(Store::default().with_std_source(s)))`.
  `Navigator::default()` still exists but now yields a Navigator over an empty
  Store with no sources at all — useful in tests, not the start of a builder
  chain. Construct one `Store` and a `Navigator` per query.
- **`Source::lookup` and `Source::load` return `Result<Option<T>>`** rather than
  `Option<T>`. `Ok(None)` means this source definitively does not have the
  crate; `Err` means a transient failure that must not be remembered as absence.
  `Source::load` also takes `&Version` rather than `Option<&Version>` — the
  exact version is resolved before the load, not during it.
- **`CrateInfo::version` returns `&Version`, not `Option<&Version>`.** Every
  listed crate has a resolved version. The type also moved from `navigator` to
  `store`; the `ferritin_common::CrateInfo` re-export is unchanged.
- **`RustdocData::all_items` is removed.** Nothing needs to iterate the whole
  index any more: the queries it existed for — the impls targeting a type, the
  implementors of a trait, the parent of a member — are precomputed reverse
  indexes read straight out of the mmapped sidecar, and a crate's resident
  footprint is now bounded by the items actually touched rather than by its size.
- **`LocalSource::can_load` is removed.**
- Following a link into another crate loads the version the query's first crate
  was *built against*, rather than the newest release or whatever the
  referencing crate recorded — so cross-crate traversal no longer depends on the
  order crates were requested in. An exact version request (`=1.2.3`) skips
  resolution entirely.
- Resolving a bare crate name, or any requirement the newest release satisfies,
  reads the `CrateIndex` rather than the crates.io API. Resolution works offline
  once the table is on disk, and can lag crates.io by up to a day. A requirement
  that excludes the newest release, and a crate published since the table was
  built, still reach the API.
- Fetching a crate from docs.rs takes one request instead of up to seven, and
  waits 30 seconds rather than 2 — cold fetches of large crates were timing out.
  A release whose rustdoc JSON predates format 55 is now reported unavailable
  rather than probed for.
- A failed docs.rs fetch is retried after a short interval instead of being
  remembered for the life of the `Store`.
- Search ranking is retuned and result order will differ throughout: names weigh
  more heavily against prose, the final query word matches name prefixes so
  partial words return results, and items are indexed under their ancestors'
  names as well as their own, so `hashmap insert` finds `HashMap::insert`.
- The on-disk caches changed format — sidecars regenerate and search indexes
  rebuild on first touch. Sidecar filenames now carry the rkyv version, so
  superseded `.rkyv1-*` files are ignored rather than replaced, and linger until
  deleted.

### Fixed

- **`CrateName`'s `Borrow<str>` impl is removed; it was unsound.** `CrateName`
  hashes `-` and `_` alike, which `str` does not, so a `&str` lookup into a
  `CrateName`-keyed map hashed differently from the key it was meant to find and
  silently never matched. `LocalSource::canonicalize` was a no-op for this
  reason. Look up with a `CrateName`.
- Resolving a path through a large family of crates that glob-re-export each
  other (the ~40 `solana-*` crates found this) could overflow the stack and
  abort the process. Resolution deeper than 256 frames is now abandoned the way
  a cycle is.
- Crates with deeply nested generic types — anything leaning on `typenum` —
  failed to load at all: the JSON parser had a hard 255-layer recursion cap.
- Searching a crate for a term its documentation uses everywhere, its own name
  included, ranked the best matches last: `Regex` was unfindable in a search of
  `regex`.
- A crate could fail to resolve when its version metadata couldn't be written to
  the cache directory, and two processes resolving the same crate at once could
  race over that file. The write is now best-effort and atomic.

## [0.14.0](https://github.com/jbr/ferritin/compare/ferritin-common-v0.13.0...ferritin-common-v0.14.0) - 2026-06-28

### Added

- add support for version 59 and simplify how we normalize
- [**breaking**] add support for rykv sidecars
- add official support for format 58

### Fixed

- *(deps)* update rust crate trillium-smol to 0.7.0

## [0.13.0](https://github.com/jbr/ferritin/compare/ferritin-common-v0.12.0...ferritin-common-v0.13.0) - 2026-06-24

### Added

- attempt to parse newer format versions than we're compiled against
- improve http client

### Other

- *(deps)* update log, trillium-client, and trillium-logger

## [0.12.0](https://github.com/jbr/ferritin/compare/ferritin-common-v0.11.2...ferritin-common-v0.12.0) - 2026-06-20

### Added

- feature selection and cached metadata
- --rebuild and --public

### Fixed

- resolve aliased cross-crate re-exports by use.id instead of source path

### Other

- fmt
- *(deps)* upgrade/update

## [0.11.2](https://github.com/jbr/ferritin/compare/ferritin-common-v0.11.1...ferritin-common-v0.11.2) - 2026-05-24

### Fixed

- display trait bounds

## [0.11.1](https://github.com/jbr/ferritin/compare/ferritin-common-v0.11.0...ferritin-common-v0.11.1) - 2026-05-22

### Fixed

- *(deps)* update rust crate trillium-client to 0.9.0
- render trait methods and associated items

### Added

- resolve trait-declared associated items (methods, associated types, and
  associated constants) by path, e.g. `std::ops::Deref::deref`
- include trait associated items in "did you mean" suggestions for mistyped
  trait member paths

## [0.11.0](https://github.com/jbr/ferritin/compare/ferritin-common-v0.10.0...ferritin-common-v0.11.0) - 2026-05-15

### Added

- document private types when using --local on workspace crates

### Other

- clippy

## [0.10.0](https://github.com/jbr/ferritin/compare/ferritin-common-v0.9.1...ferritin-common-v0.10.0) - 2026-05-08

### Fixed

- introduce a more coherent approach to cycle detection

## [0.9.1](https://github.com/jbr/ferritin/compare/ferritin-common-v0.9.0...ferritin-common-v0.9.1) - 2026-05-07

### Other

- *(deps)* upgrade trillium

## [0.9.0](https://github.com/jbr/ferritin/compare/ferritin-common-v0.8.0...ferritin-common-v0.9.0) - 2026-05-02

### Added

- search bonus for terminal-segment name coverage

### Fixed

- deterministic search order
- improved resolver
- handle version specifiers in search

### Other

- clippy
- *(deps)* upgrade deps

## [0.8.0](https://github.com/jbr/ferritin/compare/ferritin-common-v0.7.0...ferritin-common-v0.8.0) - 2026-04-20

### Fixed

- address search instability
- imroved handling of relative path prefixes like super::, crate::, and self::
- handle crate:: prefixes in cross-crate `use` items
- recover from failed resolution in iterators

### Other

- add tests for currently-broken crate::/self::/super:: resolution

## [0.7.0](https://github.com/jbr/ferritin/compare/ferritin-common-v0.6.0...ferritin-common-v0.7.0) - 2026-04-14

### Added

- display implementations of a trait
- add support for ItemSummary::path lookup

### Other

- *(deps)* upgrade all deps
- fmt
- *(deps)* upgrade deps and rebuild snapshots

## [0.6.0](https://github.com/jbr/ferritin/compare/ferritin-common-v0.5.0...ferritin-common-v0.6.0) - 2026-02-13

### Added

- exclude fenced blocks from search indexing

### Fixed

- [**breaking**] DocRef<'a, Use>::name and DocRef<'a, Item>::name collision

### Other

- Merge pull request #85 from jbr/no-indexing-code-examples

## [0.5.0](https://github.com/jbr/ferritin/compare/ferritin-common-v0.4.0...ferritin-common-v0.5.0) - 2026-02-12

### Added

- add a notion of authority based on inbound link count to search

### Fixed

- tune search because searching std for vec wasn't finding std::vec::Vec

### Other

- cache a working set of search indexes in memory on Navigator

## [0.4.0](https://github.com/jbr/ferritin/compare/ferritin-common-v0.3.0...ferritin-common-v0.4.0) - 2026-02-10

### Added

- [**breaking**] improved search algorithm (BM25)

### Other

- remove unused deps

## [0.3.0](https://github.com/jbr/ferritin/compare/ferritin-common-v0.2.0...ferritin-common-v0.3.0) - 2026-02-09

### Added

- loading bar

### Fixed

- multiple performance improvements and bugfixes

### Other

- improve ttfp for interactive mode by lazily populating Navigator

## [0.2.0](https://github.com/jbr/ferritin/compare/ferritin-common-v0.1.0...ferritin-common-v0.2.0) - 2026-01-31

### Added

- *(ferritin-common)* DocRef and Navigator are now Sync

## [0.1.0](https://github.com/jbr/ferritin/releases/tag/ferritin-common-v0.1.0) - 2026-01-29

### Added

- improvements to intra-doc-link handling
- large restructure to Navigator, fix crate name typo

### Fixed

- index paths for docsrs sources
