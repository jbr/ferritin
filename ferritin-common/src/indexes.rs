//! Derived reverse indexes over a crate's impl blocks.
//!
//! rustdoc JSON stores impl blocks flat in the item index, keyed only by their
//! own `Id` — finding "the impls targeting this type" or "the impls of this
//! trait" is a whole-index scan. These maps precompute both directions once, so
//! the scan iterators in [`crate::iterators`] become point lookups and the warm
//! path never materializes the full index (see `full_index`'s removal).
//!
//! Built in one O(n) pass: on the cold path from the resident `Crate`, and at
//! sidecar-write time so the warm path can look them up directly in the
//! memory-mapped archive without deserializing them.

use rustc_hash::FxHashMap;
use rustdoc_types::{Crate, Id, ItemEnum, StructKind, Type};

/// Reverse indexes from impl targets to impl-block `Id`s.
///
/// Only `Type::ResolvedPath` targets are keyed (matching the historical scan
/// behavior — impls for primitives, references, and tuples have no target `Id`
/// to key on). Each `Vec` is sorted so iteration order is deterministic rather
/// than inheriting `FxHashMap` iteration order.
#[derive(Debug, Default, rkyv::Archive, rkyv::Serialize)]
pub(crate) struct DerivedIndexes {
    /// Type `Id` → impl blocks with `trait_: None` targeting it.
    pub(crate) inherent_impls: FxHashMap<Id, Vec<Id>>,
    /// Type `Id` → impl blocks with `trait_: Some(_)` targeting it.
    pub(crate) trait_impls: FxHashMap<Id, Vec<Id>>,
    /// Trait `Id` → impl blocks implementing it (negative impls included;
    /// filtered at read time where callers want them excluded).
    pub(crate) implementors: FxHashMap<Id, Vec<Id>>,
    /// Associated item / variant / field `Id` → its containing item.
    ///
    /// Impl-block members map to the impl's *target type* (the `for_`
    /// `ResolvedPath` id, not the impl block), enum variants to their enum,
    /// plain-struct fields to their struct. This is what lets URL generation
    /// place an item reached with no traversal context (e.g. a prose intra-doc
    /// link to [`Vec::push`]) on its parent's page.
    ///
    /// Members of blanket impls are deliberately absent: rustdoc expands a
    /// blanket (`impl<T> TryInto<U> for T`) into per-type impl blocks with
    /// concrete `for_` types that all *share* the same member `Item`s, so a
    /// shared member has no single containing type. In practice that expansion
    /// is the only source of shared membership (verified against `alloc`:
    /// zero members appear in more than one non-blanket impl).
    pub(crate) parents: FxHashMap<Id, Id>,
}

impl DerivedIndexes {
    /// Build all four maps in one pass over the crate's item index.
    pub(crate) fn build(krate: &Crate) -> Self {
        let mut indexes = Self::default();
        // (child, parent, owner) triples; `owner` (the containing impl/enum/
        // struct id) tie-breaks the rare shared-child collision so the winning
        // parent doesn't depend on map iteration order.
        let mut parent_triples: Vec<(Id, Id, Id)> = Vec::new();
        for (id, item) in &krate.index {
            match &item.inner {
                ItemEnum::Impl(impl_) => {
                    let target = match &impl_.for_ {
                        Type::ResolvedPath(path) => Some(path.id),
                        _ => None,
                    };
                    if let Some(trait_) = &impl_.trait_ {
                        indexes.implementors.entry(trait_.id).or_default().push(*id);
                        if let Some(target) = target {
                            indexes.trait_impls.entry(target).or_default().push(*id);
                        }
                    } else if let Some(target) = target {
                        indexes.inherent_impls.entry(target).or_default().push(*id);
                    }
                    // Parenting peels references, but the impl *indexes* above do
                    // not: `impl Read for &File` documents its methods on `File`'s
                    // page (so `File` is their parent), yet the impl itself is not
                    // an impl *of* `File` and must not appear in its impl lists.
                    if let Some(parent) = impl_target_parent(&impl_.for_)
                        && impl_.blanket_impl.is_none()
                    {
                        parent_triples
                            .extend(impl_.items.iter().map(|child| (*child, parent, *id)));
                    }
                }
                ItemEnum::Enum(enum_) => {
                    parent_triples.extend(enum_.variants.iter().map(|child| (*child, *id, *id)));
                }
                ItemEnum::Struct(struct_) => {
                    if let StructKind::Plain { fields, .. } = &struct_.kind {
                        parent_triples.extend(fields.iter().map(|child| (*child, *id, *id)));
                    }
                }
                // A trait's own members are declared in the trait body, not inside
                // an impl, so the `Impl` arm above never sees them. Without this,
                // `Read::read` has no parent — which costs it both its in-app path
                // and its `#method.read` URL fragment, leaving intra-doc links to a
                // trait's own methods pointing at the bare crate root.
                ItemEnum::Trait(trait_) => {
                    parent_triples.extend(trait_.items.iter().map(|child| (*child, *id, *id)));
                }
                ItemEnum::Union(union_) => {
                    parent_triples.extend(union_.fields.iter().map(|child| (*child, *id, *id)));
                }
                _ => {}
            }
        }
        parent_triples.sort_unstable_by_key(|(child, _, owner)| (child.0, owner.0));
        for (child, parent, _) in parent_triples {
            indexes.parents.entry(child).or_insert(parent);
        }
        for map in [
            &mut indexes.inherent_impls,
            &mut indexes.trait_impls,
            &mut indexes.implementors,
        ] {
            for ids in map.values_mut() {
                ids.sort_unstable_by_key(|id| id.0);
            }
        }
        indexes
    }
}

/// The type whose *page* documents an impl's members — the parent to attribute
/// them to.
///
/// This is not the same question as "what type is this an impl of". rustdoc renders
/// `impl Read for &File` on `File`'s page, hanging `#method.read` off it, so the
/// members' parent is `File` even though the impl targets `&File`. Peeling
/// references is what lets those members have a path and a precise URL at all;
/// without it they fall back to the bare crate root.
///
/// Targets that resolve to no named type (slices, tuples, primitives, `dyn Trait`)
/// have no page to hang members on, and correctly get no parent.
fn impl_target_parent(for_: &Type) -> Option<Id> {
    match for_ {
        Type::ResolvedPath(path) => Some(path.id),
        Type::BorrowedRef { type_, .. } | Type::RawPointer { type_, .. } => {
            impl_target_parent(type_)
        }
        _ => None,
    }
}
