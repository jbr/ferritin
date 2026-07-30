//! Typed shims for rustdoc JSON formats older than the canonical
//! [`FORMAT_VERSION`](rustdoc_types::FORMAT_VERSION).
//!
//! A shim mirrors only the structs that lie on the path from [`Crate`] down to a
//! field whose *wire shape* changed, and reuses the canonical `rustdoc-types`
//! types for everything else. Because a rustdoc format bump touches a handful of
//! fields at most, the mirror stays small — currently [`Crate`]'s 8 fields and
//! [`Item`]'s 12 — while `ItemEnum`, `Type`, `Generics`, `Span`, `ItemSummary`
//! and the rest of the tree deserialize straight into their final types.
//!
//! This is what makes normalization single-pass: old JSON is parsed exactly once,
//! streaming, and every field that did *not* change is never re-read, re-boxed,
//! or routed through an intermediate [`serde_json::Value`].
//!
//! # Format 61: `Stability`
//!
//! Format 61 ([rust-lang/rust#160032]) made [`Stability::level`] externally
//! tagged so the format would survive non-self-describing serializers. Up to and
//! including format 60 the level was `#[serde(flatten)]`ed and internally tagged:
//!
//! ```text
//! ..=60  {"feature": "rust1", "level": "stable", "since": "1.0.0"}
//!   61   {"feature": "rust1", "level": {"stable": {"since": "1.0.0"}}}
//! ```
//!
//! Neither shape deserializes into the other's types, in either direction, so
//! this is a genuinely read-breaking hop rather than the usual additive bump.
//!
//! [rust-lang/rust#160032]: https://github.com/rust-lang/rust/pull/160032

use rustc_hash::FxHashMap;
use rustdoc_types::{
    Attribute, Crate, Deprecation, ExternalCrate, Id, Item, ItemEnum, ItemSummary, Span, Stability,
    StabilityLevel, Target, Visibility,
};
use serde::Deserialize;

/// A [`Crate`] as serialized by formats 48..=60.
///
/// Only `index` differs from the canonical type — it carries [`LegacyItem`] —
/// so every other field lands directly in its final type.
#[derive(Deserialize)]
pub(super) struct LegacyCrate {
    root: Id,
    crate_version: Option<String>,
    includes_private: bool,
    index: FxHashMap<Id, LegacyItem>,
    paths: FxHashMap<Id, ItemSummary>,
    external_crates: FxHashMap<u32, ExternalCrate>,
    target: Target,
    /// Mirrored for completeness and discarded on conversion — the normalized
    /// `Crate` reports [`rustdoc_types::FORMAT_VERSION`], not the version the
    /// document arrived as. Kept as a field so this struct stays a faithful
    /// mirror of [`Crate`] and the exhaustive destructure below keeps its
    /// force.
    #[allow(dead_code)]
    format_version: u32,
}

/// An [`Item`] as serialized by formats 48..=60: identical to the canonical type
/// apart from the two [`Stability`] fields, which carry the pre-61 wire shape.
///
/// `stability` and `const_stability` were added in formats 58 and 59
/// respectively, and `Option` fields are the one kind serde lets a document omit
/// (a missing one deserializes as `None`), so this single shim covers the whole
/// 48..=60 range rather than needing a variant per era.
#[derive(Deserialize)]
struct LegacyItem {
    id: Id,
    crate_id: u32,
    name: Option<String>,
    span: Option<Span>,
    visibility: Visibility,
    docs: Option<String>,
    links: FxHashMap<String, Id>,
    attrs: Vec<Attribute>,
    deprecation: Option<Deprecation>,
    stability: Option<Box<LegacyStability>>,
    const_stability: Option<Box<LegacyStability>>,
    inner: ItemEnum,
}

/// [`Stability`] as serialized by formats 58..=60, with `level` flattened into
/// the parent object and internally tagged.
#[derive(Deserialize)]
struct LegacyStability {
    feature: String,
    #[serde(flatten)]
    level: LegacyStabilityLevel,
}

/// [`StabilityLevel`] as serialized by formats 58..=60.
#[derive(Deserialize)]
#[serde(tag = "level", rename_all = "snake_case")]
enum LegacyStabilityLevel {
    Stable { since: Option<String> },
    Unstable,
}

impl From<LegacyCrate> for Crate {
    fn from(legacy: LegacyCrate) -> Self {
        // Destructured exhaustively, with no `..` rest pattern: when
        // `rustdoc-types` adds a field to `Crate`, this stops compiling instead
        // of silently dropping the field from every pre-61 document.
        let LegacyCrate {
            root,
            crate_version,
            includes_private,
            index,
            paths,
            external_crates,
            target,
            format_version: _,
        } = legacy;

        Crate {
            root,
            crate_version,
            includes_private,
            index: index
                .into_iter()
                .map(|(id, item)| (id, item.into()))
                .collect(),
            paths,
            external_crates,
            target,
            // Normalized: the document now matches the canonical types, so it
            // reports the canonical version rather than the one it arrived as.
            format_version: rustdoc_types::FORMAT_VERSION,
        }
    }
}

impl From<LegacyItem> for Item {
    fn from(legacy: LegacyItem) -> Self {
        // Exhaustive destructure — see the note in `From<LegacyCrate>`.
        let LegacyItem {
            id,
            crate_id,
            name,
            span,
            visibility,
            docs,
            links,
            attrs,
            deprecation,
            stability,
            const_stability,
            inner,
        } = legacy;

        Item {
            id,
            crate_id,
            name,
            span,
            visibility,
            docs,
            links,
            attrs,
            deprecation,
            stability: stability.map(|s| Box::new((*s).into())),
            const_stability: const_stability.map(|s| Box::new((*s).into())),
            inner,
        }
    }
}

impl From<LegacyStability> for Stability {
    fn from(legacy: LegacyStability) -> Self {
        let LegacyStability { feature, level } = legacy;
        Stability {
            feature,
            level: match level {
                LegacyStabilityLevel::Stable { since } => StabilityLevel::Stable { since },
                LegacyStabilityLevel::Unstable => StabilityLevel::Unstable,
            },
        }
    }
}
