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
//! Where a field's shape differs *between* older formats, the shim is generic
//! over that field rather than duplicated per era ([`LegacyCrate`] is
//! parameterized by its attribute representation). The whole 48..=60 range
//! therefore needs just two instantiations, both monomorphized — no runtime
//! sniffing, because `format_version` is known before the parse begins.
//!
//! # Format 54: `Item::attrs`
//!
//! Attributes were plain source-form strings until format 54 retyped them as the
//! structured [`Attribute`] enum. Legacy strings map onto [`Attribute::Other`],
//! which is documented as carrying exactly that — "a HIR debug printing, like
//! `#[attr = Optimize(Speed)]`, or the attribute as it appears in source form" —
//! so pre-54 attributes survive normalization as the same variant a modern
//! rustdoc emits for any attribute it does not model.
//!
//! # Format 57: `ExternalCrate::path`
//!
//! [`ExternalCrate::path`] became a required `PathBuf` in format 57. A missing
//! non-`Option` field is a hard deserialization error, so [`LegacyExternalCrate`]
//! marks it `#[serde(default)]` — the one field that needs it, rather than a
//! pre-pass over the document.
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
use std::path::PathBuf;

/// The attribute representation of formats 48..=53: plain source-form strings.
pub(super) type LegacyAttrs = Vec<String>;

/// The attribute representation of formats 54..=current: already structured.
pub(super) type ModernAttrs = Vec<Attribute>;

/// Lifts whichever attribute representation a format used into the canonical
/// [`Vec<Attribute>`].
///
/// A local trait rather than [`From`] because both `Vec<String>` and
/// `Vec<Attribute>` are foreign types, which the orphan rule puts out of reach.
pub(super) trait IntoAttributes {
    fn into_attributes(self) -> Vec<Attribute>;
}

impl IntoAttributes for ModernAttrs {
    fn into_attributes(self) -> Vec<Attribute> {
        self
    }
}

impl IntoAttributes for LegacyAttrs {
    fn into_attributes(self) -> Vec<Attribute> {
        // `Other` is rustdoc's own variant for an attribute it does not model,
        // and holds the same source/HIR-form string pre-54 `attrs` carried.
        self.into_iter().map(Attribute::Other).collect()
    }
}

/// A [`Crate`] as serialized by formats 48..=60, generic over the attribute
/// representation `A` ([`LegacyAttrs`] below format 54, [`ModernAttrs`] at or
/// above it).
///
/// Only `index` and `external_crates` differ from the canonical type, so every
/// other field lands directly in its final type.
#[derive(Deserialize)]
pub(super) struct LegacyCrate<A> {
    root: Id,
    crate_version: Option<String>,
    includes_private: bool,
    index: FxHashMap<Id, LegacyItem<A>>,
    paths: FxHashMap<Id, ItemSummary>,
    external_crates: FxHashMap<u32, LegacyExternalCrate>,
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
/// apart from `attrs` (whose representation is the parameter `A`) and the two
/// [`Stability`] fields, which carry the pre-61 wire shape.
///
/// `stability` and `const_stability` were added in formats 58 and 59
/// respectively, and `Option` fields are the one kind serde lets a document omit
/// (a missing one deserializes as `None`), so those two need no per-era
/// treatment — only `attrs` genuinely varies within the range.
#[derive(Deserialize)]
struct LegacyItem<A> {
    id: Id,
    crate_id: u32,
    name: Option<String>,
    span: Option<Span>,
    visibility: Visibility,
    docs: Option<String>,
    links: FxHashMap<String, Id>,
    attrs: A,
    deprecation: Option<Deprecation>,
    stability: Option<Box<LegacyStability>>,
    const_stability: Option<Box<LegacyStability>>,
    inner: ItemEnum,
}

/// An [`ExternalCrate`] as serialized by formats 48..=current.
///
/// Identical to the canonical type except that `path` — required from format 57
/// — is tolerated as absent, yielding the empty path for older documents.
#[derive(Deserialize)]
struct LegacyExternalCrate {
    name: String,
    html_root_url: Option<String>,
    #[serde(default)]
    path: PathBuf,
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

impl<A: IntoAttributes> From<LegacyCrate<A>> for Crate {
    fn from(legacy: LegacyCrate<A>) -> Self {
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
            external_crates: external_crates
                .into_iter()
                .map(|(id, ext)| (id, ext.into()))
                .collect(),
            target,
            // Normalized: the document now matches the canonical types, so it
            // reports the canonical version rather than the one it arrived as.
            format_version: rustdoc_types::FORMAT_VERSION,
        }
    }
}

impl<A: IntoAttributes> From<LegacyItem<A>> for Item {
    fn from(legacy: LegacyItem<A>) -> Self {
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
            attrs: attrs.into_attributes(),
            deprecation,
            stability: stability.map(|s| Box::new((*s).into())),
            const_stability: const_stability.map(|s| Box::new((*s).into())),
            inner,
        }
    }
}

impl From<LegacyExternalCrate> for ExternalCrate {
    fn from(legacy: LegacyExternalCrate) -> Self {
        // Exhaustive destructure — see the note in `From<LegacyCrate>`.
        let LegacyExternalCrate {
            name,
            html_root_url,
            path,
        } = legacy;

        ExternalCrate {
            name,
            html_root_url,
            path,
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
