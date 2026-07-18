# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.8.2](https://github.com/jbr/ferritin/compare/rustdoc-mcp-v0.8.1...rustdoc-mcp-v0.8.2) - 2026-07-18

### Other

- updated the following local packages: ferritin-common

## [0.8.1](https://github.com/jbr/ferritin/compare/rustdoc-mcp-v0.8.0...rustdoc-mcp-v0.8.1) - 2026-07-17

### Other

- updated the following local packages: ferritin-common

## [0.8.0] - 2026-07-16

### Added

- Reads rustdoc JSON format version 60.

### Changed

- Resolving a bare crate name — or any requirement the newest release satisfies
  — no longer asks crates.io. rustdoc-mcp keeps its own copy of the crates.io
  name→version table in `$CARGO_HOME/rustdoc-json/crate-names/`, fetched once
  (~7.5 MB compressed) and revalidated hourly in the background. So resolution
  works offline once the table is on disk, and it can lag crates.io by up to a
  day: a release published this morning may still resolve to yesterday's
  version. A requirement that excludes the newest release, and a crate published
  since the table was built, still reach the crates.io API.
- Following a link into another crate loads the version the crate you queried
  was *built against*, rather than the newest release or whatever the
  referencing crate happened to record. Items reached through `std::vec::Vec`
  into `alloc`, or through a dependency's re-export, now come from the versions
  that crate actually compiles against, and no longer depend on the order they
  were requested in.
- Fetching a crate from docs.rs takes one request instead of up to seven, and
  waits 30 seconds rather than 2 — cold fetches of large crates were timing out
  and reporting the crate as unavailable. A release whose rustdoc JSON predates
  format 55 is now reported unavailable rather than probed for.
- A failed docs.rs fetch is retried after a short interval instead of being
  remembered for the life of the process, so a network blip no longer makes a
  crate unavailable for the rest of the server's session.
- `search` ranking is retuned and result order will differ throughout. Names
  weigh more heavily against prose; a query's final word matches name prefixes,
  so partial words return results; and a query naming both a container and a
  member (`hashmap insert`) finds `HashMap::insert`, since items are now indexed
  under their ancestors' names as well as their own.
- The caches under `$CARGO_HOME/rustdoc-json/` changed format: the first lookup
  of each crate after upgrading re-parses its JSON and rebuilds its search
  index. The superseded `.rkyv1-*` sidecars are not cleaned up — delete them to
  reclaim the disk.

### Fixed

- Looking up an item in a large family of crates that glob-re-export each other
  (the ~40 `solana-*` crates found this) could overflow the stack and abort the
  server. Over-deep resolution is now abandoned the way a cycle is.
- Crates with deeply nested generic types — anything leaning on `typenum` —
  failed to load at all.
- Searching a crate for a term its documentation uses everywhere, its own name
  included, ranked the best matches last: `Regex` was unfindable in a search of
  `regex`.
- A crate could fail to resolve when its version metadata couldn't be written to
  the cache directory, and two processes resolving the same crate at once could
  race over that file.

## [0.7.2](https://github.com/jbr/ferritin/compare/rustdoc-mcp-v0.7.1...rustdoc-mcp-v0.7.2) - 2026-06-28

### Other

- update Cargo.toml dependencies

## [0.7.1](https://github.com/jbr/ferritin/compare/rustdoc-mcp-v0.7.0...rustdoc-mcp-v0.7.1) - 2026-06-24

### Other

- *(deps)* update log, trillium-client, and trillium-logger
- update binstall templates

## [0.7.0](https://github.com/jbr/ferritin/compare/rustdoc-mcp-v0.6.7...rustdoc-mcp-v0.7.0) - 2026-06-20

### Added

- feature selection and cached metadata
- --rebuild and --public

### Fixed

- resolve aliased cross-crate re-exports by use.id instead of source path

### Other

- *(deps)* upgrade/update
- update snaps
- update snaps to rustc 1.98.0-nightly (bc2112ed5 2026-06-18)
- update snaps to rustc 1.98.0-nightly (14210df0e 2026-05-31)

## [0.6.7](https://github.com/jbr/ferritin/compare/rustdoc-mcp-v0.6.6...rustdoc-mcp-v0.6.7) - 2026-05-24

### Fixed

- display trait bounds

### Other

- update snaps to rustc 1.98.0-nightly (23a3312d9 2026-05-23)
- update snaps to rustc 1.98.0-nightly (54333ff07 2026-05-22)

## [0.6.6](https://github.com/jbr/ferritin/compare/rustdoc-mcp-v0.6.5...rustdoc-mcp-v0.6.6) - 2026-05-22

### Other

- update snaps to rustc 1.97.0-nightly (9eb3be26b 2026-05-18)

## [0.6.5](https://github.com/jbr/ferritin/compare/rustdoc-mcp-v0.6.4...rustdoc-mcp-v0.6.5) - 2026-05-15

### Other

- update snaps to rustc 1.97.0-nightly (7c3c88f42 2026-05-14)
- clippy

## [0.6.4](https://github.com/jbr/ferritin/compare/rustdoc-mcp-v0.6.3...rustdoc-mcp-v0.6.4) - 2026-05-08

### Fixed

- introduce a more coherent approach to cycle detection

## [0.6.3](https://github.com/jbr/ferritin/compare/rustdoc-mcp-v0.6.2...rustdoc-mcp-v0.6.3) - 2026-05-07

### Other

- update Cargo.lock dependencies

## [0.6.2](https://github.com/jbr/ferritin/compare/rustdoc-mcp-v0.6.1...rustdoc-mcp-v0.6.2) - 2026-05-02

### Fixed

- address test instability in rustdoc-mcp

### Other

- update snaps to rustc 1.97.0-nightly (f53b654a8 2026-04-30)
- update snaps to rustc 1.97.0-nightly (f53b654a8 2026-04-30)
- clippy
- *(deps)* upgrade deps
- update rust
- update snaps to rustc 1.97.0-nightly (66da6cae1 2026-04-20)

## [0.6.1](https://github.com/jbr/ferritin/compare/rustdoc-mcp-v0.6.0...rustdoc-mcp-v0.6.1) - 2026-04-20

### Fixed

- imroved handling of relative path prefixes like super::, crate::, and self::
- recover from failed resolution in iterators

### Other

- update snaps

## [0.6.0](https://github.com/jbr/ferritin/compare/rustdoc-mcp-v0.5.0...rustdoc-mcp-v0.6.0) - 2026-04-14

### Added

- add support for ItemSummary::path lookup

### Other

- *(deps)* upgrade all deps
- update rust
- update snaps
- *(deps)* upgrade deps and rebuild snapshots

## [0.5.0](https://github.com/jbr/ferritin/compare/rustdoc-mcp-v0.4.0...rustdoc-mcp-v0.5.0) - 2026-02-13

### Added

- exclude fenced blocks from search indexing

## [0.4.0](https://github.com/jbr/ferritin/compare/rustdoc-mcp-v0.3.0...rustdoc-mcp-v0.4.0) - 2026-02-12

### Added

- add a notion of authority based on inbound link count to search

### Other

- update architecture doc to reflect search algorithm
- cache a working set of search indexes in memory on Navigator

## [0.3.0](https://github.com/jbr/ferritin/compare/rustdoc-mcp-v0.2.0...rustdoc-mcp-v0.3.0) - 2026-02-10

### Added

- scrollbar!

## [0.2.0](https://github.com/jbr/ferritin/compare/rustdoc-mcp-v0.1.9...rustdoc-mcp-v0.2.0) - 2026-02-10

### Added

- [**breaking**] improved search algorithm (BM25)

## [0.1.9](https://github.com/jbr/ferritin/compare/rustdoc-mcp-v0.1.8...rustdoc-mcp-v0.1.9) - 2026-02-09

### Fixed

- multiple performance improvements and bugfixes

## [0.1.8](https://github.com/jbr/ferritin/compare/rustdoc-mcp-v0.1.7...rustdoc-mcp-v0.1.8) - 2026-02-06

### Other

- Merge pull request #58 from jbr/fix-some-more-typos
- fix some more embarrassing typos

## [0.1.7](https://github.com/jbr/ferritin/compare/rustdoc-mcp-v0.1.6...rustdoc-mcp-v0.1.7) - 2026-01-31

### Other

- Merge pull request #32 from jbr/rustdoc-mcp-readme
- update rustdoc-mcp README

## [0.1.6](https://github.com/jbr/ferritin/compare/rustdoc-mcp-v0.1.5...rustdoc-mcp-v0.1.6) - 2026-01-29

### Added

- large restructure to Navigator, fix crate name typo

### Other

- Merge pull request #23 from jbr/large-refactor-and-rename

## [0.1.5](https://github.com/jbr/ferritin/releases/tag/rustdoc-mcp-v0.1.5) - 2026-01-21

### Added

- use versioned ferritin-common
- [**breaking**] docs.rs client
- render main item full docs
- [**breaking**] ferritin is functional
- [**breaking**] initial commit of ferritin

### Fixed

- tests

### Other

- update cargo files, add readme for ferritin-common
- readme
- fmt
- clippy
- fmt
- fmt
- upgrade deps
- convert to workspace, extract core library
