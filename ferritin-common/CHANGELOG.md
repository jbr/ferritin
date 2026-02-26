# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.7.0](https://github.com/jbr/ferritin/compare/ferritin-common-v0.6.0...ferritin-common-v0.7.0) - 2026-02-26

### Added

- add support for ItemSummary::path lookup

### Other

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
