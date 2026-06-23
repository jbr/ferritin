//! CLI surface for kind filtering.
//!
//! `Kind` is the typed `--kind` argument (a `clap::ValueEnum`); its only job is
//! to *build* a [`DisplayPredicate`] closure. The listing code never sees this
//! enum — it only sees the erased predicate — so the predicate stays the
//! general internal contract while the enum gives agents and `--help` a small,
//! first-try-correct surface. Other narrowing terms (`as Trait`, name
//! substrings) can produce their own predicates later and compose by `&&`.

use crate::format_context::DisplayPredicate;
use rustdoc_types::ItemKind;

/// A category of item to keep when listing. Mirrors the kinds rustdoc
/// distinguishes, collapsing the three macro-flavored kinds under `Macro`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum Kind {
    Struct,
    Enum,
    Trait,
    #[value(alias = "fn")]
    Function,
    Constant,
    Static,
    #[value(alias = "mod")]
    Module,
    Union,
    Macro,
    #[value(alias = "typealias")]
    Type,
    Variant,
}

impl Kind {
    fn matches(self, kind: ItemKind) -> bool {
        use ItemKind::*;
        match self {
            Kind::Struct => kind == Struct,
            Kind::Enum => kind == Enum,
            Kind::Trait => kind == Trait,
            Kind::Function => kind == Function,
            Kind::Constant => kind == Constant,
            Kind::Static => kind == Static,
            Kind::Module => kind == Module,
            Kind::Union => kind == Union,
            Kind::Macro => matches!(kind, Macro | ProcAttribute | ProcDerive),
            Kind::Type => kind == TypeAlias,
            Kind::Variant => kind == Variant,
        }
    }
}

/// Build a display predicate that keeps only items matching one of `kinds`.
/// Returns `None` for an empty selection (i.e. "show everything"), so callers
/// don't pay for a closure when no filter was requested.
pub(crate) fn predicate(kinds: &[Kind]) -> Option<DisplayPredicate> {
    if kinds.is_empty() {
        return None;
    }
    let kinds = kinds.to_vec();
    Some(Box::new(move |item| {
        kinds.iter().any(|k| k.matches(item.kind()))
    }))
}
