//! Pure iterators over rustdoc items — no `Use` resolution.
//!
//! These iterators don't borrow a [`crate::Resolver`] and don't follow `Use`
//! re-exports. For child iteration that resolves and expands `Use`s, use
//! [`crate::Resolver::children`] / [`crate::Resolver::ids`] instead.
//!
//! What lives here:
//! - [`LazyChildren`] / [`LazyChild`]: classify a module/enum's direct
//!   children into real items, non-glob `Use`s, and glob `Use`s. Resolution
//!   of `Use` source paths happens in `Resolver::resolve_lazy_child`.
//! - [`MethodIter`], [`TraitIter`], [`ImplementorIter`]: walk the impl blocks
//!   targeting a given item, via the precomputed reverse indexes
//!   ([`crate::indexes`]) rather than a whole-index scan. No `Use` involvement.

use crate::doc_ref::DocRef;
use rustdoc_types::{Id, Item, ItemEnum, Use};

/// Walk a precomputed list of impl-block `Id`s, yielding each block as a
/// `DocRef` in `anchor`'s crate. Shared machinery for the three impl iterators.
struct ImplBlockIter<'a> {
    anchor: DocRef<'a, Item>,
    ids: std::vec::IntoIter<Id>,
}

impl<'a> ImplBlockIter<'a> {
    fn new(anchor: DocRef<'a, Item>, ids: Vec<Id>) -> Self {
        Self {
            anchor,
            ids: ids.into_iter(),
        }
    }
}

impl<'a> Iterator for ImplBlockIter<'a> {
    type Item = DocRef<'a, Item>;

    fn next(&mut self) -> Option<Self::Item> {
        for id in &mut self.ids {
            if let Some(item) = self.anchor.get(&id) {
                return Some(item);
            }
        }
        None
    }
}

pub struct MethodIter<'a> {
    item: DocRef<'a, Item>,
    impl_block_iter: ImplBlockIter<'a>,
    current_item_iter: Option<std::slice::Iter<'a, Id>>,
}

impl<'a> MethodIter<'a> {
    pub(crate) fn new(item: DocRef<'a, Item>) -> Self {
        let ids = item.crate_docs().inherent_impl_ids(&item.id);
        Self {
            item,
            impl_block_iter: ImplBlockIter::new(item, ids),
            current_item_iter: None,
        }
    }
}

impl<'a> DocRef<'a, Item> {
    pub fn methods(&self) -> MethodIter<'a> {
        MethodIter::new(*self)
    }

    pub fn traits(&self) -> TraitIter<'a> {
        TraitIter::new(*self)
    }

    /// Lazy classification of direct children. Each yielded [`LazyChild`]
    /// carries the parent module so callers know how to resolve relative
    /// `Use::source` paths. Resolve via
    /// [`crate::Resolver::resolve_lazy_child`].
    pub fn lazy_children(&self) -> LazyChildren<'a> {
        LazyChildren::new(*self)
    }

    /// Iterate impl blocks in this crate that implement this trait.
    pub fn implementors(&self) -> ImplementorIter<'a> {
        ImplementorIter::new(*self)
    }
}

/// A child item where `Use` source resolution is deferred. Yielded by
/// [`LazyChildren`]; resolve via [`crate::Resolver::resolve_lazy_child`].
///
/// `Use` variants carry the parent module so that `self::`/`super::` paths in
/// the Use's source string resolve against the right scope.
#[derive(Debug, Clone, Copy)]
pub enum LazyChild<'a> {
    /// A non-`Use` direct child (module, struct, function, …).
    Item(DocRef<'a, Item>),
    /// A non-glob `Use` item. Its imported name is `use_item.name`; the
    /// pointed-to item is not yet loaded.
    NonGlob {
        use_item: DocRef<'a, Use>,
        parent: DocRef<'a, Item>,
    },
    /// A glob `Use` (`pub use foo::*;`). Has no single name — resolve and walk
    /// the source's children if you want to look through it.
    Glob {
        use_item: DocRef<'a, Use>,
        parent: DocRef<'a, Item>,
    },
}

impl<'a> LazyChild<'a> {
    /// Cheap name access. `None` for glob Uses (no single imported name).
    pub fn name(&self) -> Option<&'a str> {
        match self {
            LazyChild::Item(item) => item.name(),
            LazyChild::NonGlob { use_item, .. } => Some(&use_item.item().name),
            LazyChild::Glob { .. } => None,
        }
    }
}

/// Iterator over a module/enum's direct children, classified into
/// [`LazyChild`] variants. Does not resolve any `Use`s.
pub struct LazyChildren<'a> {
    parent: DocRef<'a, Item>,
    ids: std::slice::Iter<'a, Id>,
}

impl<'a> LazyChildren<'a> {
    pub(crate) fn new(parent: DocRef<'a, Item>) -> Self {
        let ids: &[Id] = match parent.inner() {
            ItemEnum::Module(m) => &m.items,
            ItemEnum::Enum(e) => &e.variants,
            _ => &[],
        };
        Self {
            parent,
            ids: ids.iter(),
        }
    }
}

impl<'a> Iterator for LazyChildren<'a> {
    type Item = LazyChild<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        for id in &mut self.ids {
            let Some(item) = self.parent.get(id) else {
                continue;
            };
            return Some(match item.inner() {
                ItemEnum::Use(use_item) => {
                    let use_ref = item.build_ref(use_item);
                    if use_item.is_glob {
                        LazyChild::Glob {
                            use_item: use_ref,
                            parent: self.parent,
                        }
                    } else {
                        LazyChild::NonGlob {
                            use_item: use_ref,
                            parent: self.parent,
                        }
                    }
                }
                _ => LazyChild::Item(item),
            });
        }
        None
    }
}

/// Iterator over the trait impl blocks targeting a type.
pub struct TraitIter<'a>(ImplBlockIter<'a>);

impl<'a> TraitIter<'a> {
    fn new(item: DocRef<'a, Item>) -> Self {
        Self(ImplBlockIter::new(
            item,
            item.crate_docs().trait_impl_ids(&item.id),
        ))
    }
}

impl<'a> Iterator for TraitIter<'a> {
    type Item = DocRef<'a, Item>;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }
}

/// Iterator over the (non-negative) impl blocks implementing a trait.
pub struct ImplementorIter<'a>(ImplBlockIter<'a>);

impl<'a> ImplementorIter<'a> {
    fn new(trait_item: DocRef<'a, Item>) -> Self {
        Self(ImplBlockIter::new(
            trait_item,
            trait_item.crate_docs().implementor_ids(&trait_item.id),
        ))
    }
}

impl<'a> Iterator for ImplementorIter<'a> {
    type Item = DocRef<'a, Item>;

    fn next(&mut self) -> Option<Self::Item> {
        // The index includes negative impls (`impl !Send for …`); implementor
        // listings exclude them, matching the historical scan's filter.
        self.0.find(
            |item| !matches!(item.inner(), ItemEnum::Impl(impl_block) if impl_block.is_negative),
        )
    }
}

impl<'a> Iterator for MethodIter<'a> {
    type Item = DocRef<'a, Item>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(current_item_iter) = &mut self.current_item_iter {
                for id in current_item_iter {
                    if let Some(item) = self.item.get(id) {
                        return Some(item.with_parent(self.item));
                    }
                }
            }

            if let Some(item) = self.impl_block_iter.next()
                && let ItemEnum::Impl(impl_block) = &item.item().inner
            {
                self.current_item_iter = Some(impl_block.items.iter())
            } else {
                return None;
            }
        }
    }
}
