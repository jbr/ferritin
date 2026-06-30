---
name: ferritin
description: >-
  Look up real Rust documentation with the `ferritin` CLI instead of guessing
  APIs from memory. Use whenever you need a crate's types, traits, methods,
  function signatures, or module contents — for dependencies, the standard
  library, or local workspace crates. Reach for it before answering any Rust
  API question ("what methods does X have", "how do I use Y", "what's the
  signature of Z", "does this trait require..."), and when writing or reviewing
  Rust against an unfamiliar API. Runs as a plain shell command; no MCP server.
paths: "**/*.rs, **/Cargo.toml"
allowed-tools: Bash(ferritin:*)
---

# ferritin — Rust documentation lookup

`ferritin` renders real rustdoc for any crate. Prefer it over recalling Rust
APIs from memory: it returns actual current signatures, trait impls, and prose
for dependencies, `std`, and the local workspace's own crates — at the exact
versions this project resolves.

In an agent session ferritin auto-detects the environment and emits
LLM-friendly output; you do not need to pass any output-format flag.

## Core commands

- `ferritin get <path>` — documentation for one item, where `<path>` is
  `crate::Module::Item`:
  - `ferritin get std::vec::Vec`
  - `ferritin get serde::Serialize`
  - `ferritin get serde@1.0::Serialize`  (pin a version — on the crate
    segment, not as a suffix on the full path)
  - The version is a semver **spec**, not an exact version: `serde@1.0`
    matches any `1.x.y`, while `serde@=1.0.0` pins exactly `1.0.0`. The full
    semver requirement syntax (`^`, `~`, `>=`, `*`, etc.) applies.
- `ferritin search <crate> <query>...` — search within one crate:
  - `ferritin search tokio spawn`
  - `ferritin search serde@1.0 Deserialize`  (pin a version)
- `ferritin list` — list the crates available to query.

By default ferritin sources docs straight from docs.rs as rustdoc JSON, so a
crate does **not** need to be a dependency of this project or installed locally
— you can query any published crate by name. To inspect the **current
workspace's own** (in-development) crates instead, add `-l` / `--local` (uses
the `Cargo.toml` in the working directory), or target one explicitly with
`--manifest-path <path>`:

- `ferritin -l get my_crate::MyType`

## Discovering current flags

This CLI is actively evolving — treat `--help` as the source of truth for the
live flag surface rather than guessing:

- `ferritin --help`
- `ferritin get --help` · `ferritin search --help`

Flags worth knowing (confirm names/values with `--help`):

- `-k, --kind <struct|enum|trait|function|module|…>` — restrict a listing or
  search to given kinds (comma-separated or repeated).
- `--docs <full|brief|none>` — how much doc prose to render; `none` gives just
  the signature/listing.
- `-r, --recursive` (on `get`) — descend into nested items.

## When to use it

Reach for ferritin whenever a task touches an unfamiliar Rust API: before
writing code against a crate, when answering "what methods/fields/variants does
X have", when checking a trait's required methods, or when verifying a
signature. It is faster and more accurate than inferring from memory, and
reflects the exact versions resolved in this project.
