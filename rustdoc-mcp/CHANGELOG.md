# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
