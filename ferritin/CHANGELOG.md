# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

