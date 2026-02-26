# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.8.0](https://github.com/jbr/ferritin/compare/ferritin-v0.7.0...ferritin-v0.8.0) - 2026-02-26

### Added

- add support for ItemSummary::path lookup

### Other

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

