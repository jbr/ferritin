# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.16.0](https://github.com/jbr/ferritin/compare/ferritin-v0.15.1...ferritin-v0.16.0) - 2026-07-30

### Added

- update format version to 61
- log the mcp tool call
- *(design)* redesign web ui
- *(serve)* add og tags
- add /~mcp
- MCP HTTP endpoint (/mcp) exposing get and search as tools

### Other

- update/upgrade deps
- update snaps to rustc 1.99.0-nightly (1a833e165 2026-07-29)
- update snaps to rustc 1.99.0-nightly (dc3f85158 2026-07-26)
- *(deps)* update ferritin/assets/themes/solarized digest to 132a4db
- update snaps to rustc 1.99.0-nightly (da86f4d07 2026-07-24)
- update snaps to rustc 1.99.0-nightly (89c61a754 2026-07-23)
- update snaps to rustc 1.99.0-nightly (6f72b5dd5 2026-07-22)
- update snaps to rustc 1.99.0-nightly (0e29c21d9 2026-07-21)
- update snaps to rustc 1.99.0-nightly (87e5904f5 2026-07-20)
- update snaps to rustc 1.99.0-nightly (9f36de775 2026-07-19)

## [0.15.1](https://github.com/jbr/ferritin/compare/ferritin-v0.15.0...ferritin-v0.15.1) - 2026-07-18

### Other

- migrate storage paths

## [0.15.0](https://github.com/jbr/ferritin/compare/ferritin-v0.14.0...ferritin-v0.15.0) - 2026-07-17

### Added

- move crate-names lookup off of the request path, do it no more frequently than every 24h
- typeahead fuzziness and fix crate-not-found

### Other

- update snaps to rustc 1.99.0-nightly (3d50c25bc 2026-07-16)
- update snaps to rustc 1.99.0-nightly (3d50c25bc 2026-07-16)

## [0.14.0] - 2026-07-16

### Added

- `--format <tty|plain|agent|json>` selects the output format explicitly.
  Without it, ferritin autodetects as before: agent format under a coding agent,
  ANSI on a TTY, plain when piped.
- `--format json` prints a structured model of the result rather than rendered
  text. `get` emits the item as its kind's own shape — a struct's fields, an
  enum's variants, a trait's members and every one of its implementors, a
  module's children, a type's trait impls — each type reference carrying the
  path or URL it points at, and each code block carrying syntax-highlight
  classes. `search` and `list` emit their results the same way. It cannot be
  combined with `--interactive`.
- Unions render like structs. They were previously `[not yet implemented]`.
- Negative impls (`impl !Send for T`) appear among a type's trait impls,
  prefixed with `!`. They were previously dropped.
- Reads rustdoc JSON format version 60.
- A `serve` cargo feature, off by default, adds a `ferritin serve` subcommand
  that runs a documentation web server. Distributed binaries are built without
  it.

### Changed

- **`--agent` and its `--ai` alias are removed**; use `--format agent`.
  Autodetection from `CLAUDECODE`/`GEMINI_CLI`/`CODEX_SANDBOX` is unchanged.
- Resolving a bare crate name — or any requirement the newest release satisfies
  — no longer asks crates.io. ferritin keeps its own copy of the crates.io
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
  that crate actually compiles against, and no longer depend on the order you
  navigated in.
- Fetching a crate from docs.rs takes one request instead of up to seven, and
  waits 30 seconds rather than 2 — cold fetches of large crates were timing out
  and reporting the crate as unavailable. A release whose rustdoc JSON predates
  format 55 is now reported unavailable rather than probed for.
- A failed docs.rs fetch is retried after a short interval instead of being
  remembered for the life of the process. In the TUI, a network blip no longer
  makes a crate unavailable for the rest of the session.
- Search ranking is retuned and result order will differ throughout. Names weigh
  more heavily against prose than before, and long prose that dwells on a term
  can outrank the item named for it — for documentation search that is usually
  the page you want, and the named item lands just below it.
- Search matches a name prefix on the final word of a query, so results appear
  before you finish typing (`Vec::pu` reaches `push`), including at interior
  word boundaries (`set` reaches `TypeSet`).
- A query naming both a container and a member (`hashmap insert`) finds
  `HashMap::insert`. Items are indexed under their ancestors' names as well as
  their own, weighted by distance.
- The caches under `$CARGO_HOME/rustdoc-json/` changed format: the first lookup
  of each crate after upgrading re-parses its JSON and rebuilds its search
  index. The superseded `.rkyv1-*` sidecars are not cleaned up — delete them to
  reclaim the disk.

### Fixed

- Looking up an item in a large family of crates that glob-re-export each other
  (the ~40 `solana-*` crates found this) could overflow the stack and abort
  ferritin. Over-deep resolution is now abandoned the way a cycle is.
- Crates with deeply nested generic types — anything leaning on `typenum` —
  failed to load at all.
- Searching a crate for a term its documentation uses everywhere, its own name
  included, ranked the best matches last: `Regex` was unfindable in a search of
  `regex`.
- docs.rs links for a crate whose library name differs from its package name
  (`sha-1`, whose lib is `sha1`) pointed at a 404.
- Links to attribute and derive macros pointed at `macro.{name}.html`, which
  404s for both.
- A relative link in a crate's prose (`struct.TcpStream.html`) resolved against
  the crate root rather than the module the page is rendered in, reaching the
  wrong item or none.
- A docs.rs URL written out by hand in a doc comment now resolves to the item it
  names and navigates in place, instead of being treated as an ordinary external
  link.
- The TUI status line's key hints vanished on the first mouse move and stayed
  gone for the session.
- A crate could fail to resolve when its version metadata couldn't be written to
  the cache directory, and two ferritin processes resolving the same crate at
  once could race over that file.

## [0.13.0](https://github.com/jbr/ferritin/compare/ferritin-v0.12.0...ferritin-v0.13.0) - 2026-06-28

### Added

- add support for version 59 and simplify how we normalize
- [**breaking**] add support for rykv sidecars
- add official support for format 58

### Other

- update snaps to rustc 1.98.0-nightly (f28ac764c 2026-06-23)
- update snaps to rustc 1.98.0-nightly (f28ac764c 2026-06-23)

## [0.12.0](https://github.com/jbr/ferritin/compare/ferritin-v0.11.1...ferritin-v0.12.0) - 2026-06-24

### Added

- add type filtering
- attempt to parse newer format versions than we're compiled against

### Other

- *(deps)* update log, trillium-client, and trillium-logger
- update snaps to rustc 1.98.0-nightly (4429659e4 2026-06-22)
- update binstall templates

## [0.11.1](https://github.com/jbr/ferritin/compare/ferritin-v0.11.0...ferritin-v0.11.1) - 2026-06-20

### Fixed

- *(windows)* address some failures

### Other

- update readme installation instructions

## [0.11.0](https://github.com/jbr/ferritin/compare/ferritin-v0.10.2...ferritin-v0.11.0) - 2026-06-20

### Added

- feature selection and cached metadata
- --rebuild and --public

### Fixed

- resolve aliased cross-crate re-exports by use.id instead of source path

### Other

- update snaps to rustc 1.98.0-nightly (bc2112ed5 2026-06-18)
- fmt
- *(deps)* upgrade/update
- update snaps
- update snaps to rustc 1.98.0-nightly (bc2112ed5 2026-06-18)
- update snaps to rustc 1.98.0-nightly (14210df0e 2026-05-31)

## [0.10.2](https://github.com/jbr/ferritin/compare/ferritin-v0.10.1...ferritin-v0.10.2) - 2026-05-24

### Fixed

- display trait bounds

### Other

- update snaps to rustc 1.98.0-nightly (23a3312d9 2026-05-23)
- update snaps to rustc 1.98.0-nightly (54333ff07 2026-05-22)

## [0.10.1](https://github.com/jbr/ferritin/compare/ferritin-v0.10.0...ferritin-v0.10.1) - 2026-05-22

### Fixed

- render trait methods and associated items

### Other

- fmt
- update snaps to rustc 1.97.0-nightly (b954122bb 2026-05-20)
- update snaps to rustc 1.97.0-nightly (9eb3be26b 2026-05-18)

### Added

- render individual trait associated items fetched with `get`: methods,
  associated types (`type T: Clone;`), and associated constants

## [0.10.0](https://github.com/jbr/ferritin/compare/ferritin-v0.9.2...ferritin-v0.10.0) - 2026-05-15

### Added

- ai mode improvements
- improved table rendering

### Other

- update snaps to rustc 1.97.0-nightly (7c3c88f42 2026-05-14)
- update snaps
- clippy
- update snaps
- update snaps to rustc 1.97.0-nightly (8b03437a8 2026-05-12)
- update snaps to rustc 1.97.0-nightly (fb0a5a5a9 2026-05-08)

## [0.9.2](https://github.com/jbr/ferritin/compare/ferritin-v0.9.1...ferritin-v0.9.2) - 2026-05-08

### Fixed

- introduce a more coherent approach to cycle detection
- table rendering

### Other

- update snaps to rustc 1.97.0-nightly (f964de49b 2026-05-07)
- update snaps to rustc 1.97.0-nightly (f964de49b 2026-05-07)
- update snaps to rustc 1.97.0-nightly (f964de49b 2026-05-07)

## [0.9.1](https://github.com/jbr/ferritin/compare/ferritin-v0.9.0...ferritin-v0.9.1) - 2026-05-07

### Other

- update snaps to rustc 1.97.0-nightly (e95e73209 2026-05-05)

## [0.9.0](https://github.com/jbr/ferritin/compare/ferritin-v0.8.1...ferritin-v0.9.0) - 2026-05-02

### Added

- improved search interface
- search bonus for terminal-segment name coverage

### Fixed

- deterministic search order
- address the -l overlap between search --limit and --local

### Other

- update snaps to rustc 1.97.0-nightly (f53b654a8 2026-04-30)
- update snaps to rustc 1.97.0-nightly (f53b654a8 2026-04-30)
- clippy
- *(deps)* upgrade deps
- update rust
- update snaps to rustc 1.97.0-nightly (66da6cae1 2026-04-20)

## [0.8.1](https://github.com/jbr/ferritin/compare/ferritin-v0.8.0...ferritin-v0.8.1) - 2026-04-20

### Fixed

- address search instability
- address module-listing order instability
- utf-8 mid-grapheme-aware truncation
- imroved handling of relative path prefixes like super::, crate::, and self::
- recover from failed resolution in iterators

### Other

- fix search snaps
- update rust
- update snaps
- update snaps
- add tests for currently-broken crate::/self::/super:: resolution

## [0.8.0](https://github.com/jbr/ferritin/compare/ferritin-v0.7.0...ferritin-v0.8.0) - 2026-04-14

### Added

- add ai-mode
- display implementations of a trait
- improved trait impl list display
- [**breaking**] ferritin defaults to docs.rs, falls back to local with --local
- add support for ItemSummary::path lookup

### Other

- Merge pull request #105 from jbr/renovate/ferritin-assets-themes-solarized-digest
- Merge pull request #115 from jbr/ai-mode
- fmt
- update rust
- update snaps
- *(deps)* upgrade deps and rebuild snapshots

## [0.7.0](https://github.com/jbr/ferritin/compare/ferritin-v0.6.0...ferritin-v0.7.0) - 2026-02-13

### Added

- exclude fenced blocks from search indexing

### Fixed

- address issue where scrolling was broken in screens with no links
- improve handling of workspaces with no default crate root

### Other

- Merge pull request #88 from jbr/scroll-bug
- Merge pull request #86 from jbr/default-search-when-no-crate-root

## [0.6.0](https://github.com/jbr/ferritin/compare/ferritin-v0.5.0...ferritin-v0.6.0) - 2026-02-12

### Added

- add a notion of authority based on inbound link count to search

### Fixed

- use .0 precision for display because there's a strange fp discrepancy in ci
- use .0 precision because there's a strange floating point difference in CI
- drop early-stopping logic from search
- update nightly snapshots
- always underline links

### Other

- update architecture doc to reflect search algorithm
- cache a working set of search indexes in memory on Navigator

## [0.5.0](https://github.com/jbr/ferritin/compare/ferritin-v0.4.0...ferritin-v0.5.0) - 2026-02-10

### Added

- scrollbar!

### Fixed

- improved heading display

## [0.4.0](https://github.com/jbr/ferritin/compare/ferritin-v0.3.0...ferritin-v0.4.0) - 2026-02-10

### Added

- [**breaking**] improved search algorithm (BM25)

### Other

- remove unused deps

## [0.3.0](https://github.com/jbr/ferritin/compare/ferritin-v0.2.0...ferritin-v0.3.0) - 2026-02-09

### Added

- loading bar
- add logs to the status line to indicate what's happening

### Fixed

- loading spinner updates even when there are no events
- no longer include rust sha in snapshots
- multiple performance improvements and bugfixes

### Other

- improve ttfp for interactive mode by lazily populating Navigator

## [0.2.0](https://github.com/jbr/ferritin/compare/ferritin-v0.1.2...ferritin-v0.2.0) - 2026-02-06

### Added

- add theme picker
- small improvement to interactive theme selection
- improve color scheme scopes
- improved theming support

### Fixed

- tests

### Other

- Merge pull request #58 from jbr/fix-some-more-typos
- fix some more embarrassing typos
- fmt
- fix build and improve error message

## [0.1.2](https://github.com/jbr/ferritin/compare/ferritin-v0.1.1...ferritin-v0.1.2) - 2026-02-01

### Other

- tui cleanup
- refactor render loop, add initial tui tests

## [0.1.1](https://github.com/jbr/ferritin/compare/ferritin-v0.1.0...ferritin-v0.1.1) - 2026-01-31

### Added

- ferritin interactive-mode is no longer single-threaded

### Other

- Merge pull request #28 from jbr/threading

## [0.1.0](https://github.com/jbr/ferritin/releases/tag/ferritin-v0.1.0) - 2026-01-29

