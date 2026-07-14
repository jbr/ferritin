//! Bidirectional translation between items and their rendered documentation URLs.
//!
//! [`generate_docsrs_url`] maps a resolved [`DocRef`](ferritin_common::DocRef) to
//! the docs.rs (or doc.rust-lang.org) page documenting it. [`DocsRsLink::parse`]
//! goes the other way, turning such a URL back into a path our lookup engine
//! accepts — which is what lets documentation that hardcodes docs.rs links,
//! rather than intra-doc links, still navigate in-app.
//!
//! Both directions read the same [`SIGILS`] table, so the page-filename
//! vocabulary (`struct.Foo.html`, `derive.Bar.html`) cannot drift between them.

mod generate;
mod parse;

pub(crate) use generate::{crate_base_url, generate_docsrs_url};
pub(crate) use parse::{DocsRsLink, resolve_relative};
use rustdoc_types::ItemKind;

/// rustdoc names an item's page `{sigil}.{Name}.html`. This is the full set of
/// sigils it emits, and the kind each denotes.
///
/// Modules are absent because their page is a directory index (`{name}/index.html`),
/// as are the kinds that get no page of their own — variants, fields, and associated
/// items are documented in a fragment on their parent's page, addressed by anchor
/// rather than filename.
const SIGILS: &[(&str, ItemKind)] = &[
    ("struct", ItemKind::Struct),
    ("enum", ItemKind::Enum),
    ("trait", ItemKind::Trait),
    ("traitalias", ItemKind::TraitAlias),
    ("fn", ItemKind::Function),
    ("type", ItemKind::TypeAlias),
    ("constant", ItemKind::Constant),
    ("static", ItemKind::Static),
    ("union", ItemKind::Union),
    ("macro", ItemKind::Macro),
    ("attr", ItemKind::ProcAttribute),
    ("derive", ItemKind::ProcDerive),
    ("primitive", ItemKind::Primitive),
    ("keyword", ItemKind::Keyword),
    ("foreigntype", ItemKind::ExternType),
];

/// The page-filename sigil for `kind`, or `None` for kinds rustdoc gives no page.
pub(crate) fn sigil_for_kind(kind: ItemKind) -> Option<&'static str> {
    SIGILS.iter().find(|(_, k)| *k == kind).map(|(s, _)| *s)
}

/// The kind a page-filename sigil denotes, or `None` if it isn't one rustdoc emits.
pub(crate) fn kind_for_sigil(sigil: &str) -> Option<ItemKind> {
    SIGILS.iter().find(|(s, _)| *s == sigil).map(|(_, k)| *k)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every sigil round-trips, and `macro`/`attr`/`derive` stay distinct — the
    /// three proc-macro kinds once all generated `macro.{name}.html`, which 404s
    /// on docs.rs for attributes and derives.
    #[test]
    fn sigils_round_trip() {
        for (sigil, kind) in SIGILS {
            assert_eq!(kind_for_sigil(sigil), Some(*kind));
            assert_eq!(sigil_for_kind(*kind), Some(*sigil));
        }
    }

    #[test]
    fn kinds_without_pages_have_no_sigil() {
        for kind in [
            ItemKind::Module,
            ItemKind::Variant,
            ItemKind::StructField,
            ItemKind::AssocConst,
            ItemKind::AssocType,
            ItemKind::Impl,
            ItemKind::Use,
        ] {
            assert_eq!(sigil_for_kind(kind), None);
        }
    }
}
