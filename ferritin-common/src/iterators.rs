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
//! - [`MethodIter`], [`TraitIter`], [`ImplementorIter`]: scan a crate index
//!   for impl blocks targeting a given item. No `Use` involvement.

use crate::doc_ref::DocRef;
use rustdoc_types::{Id, Item, ItemEnum, Type, Use};
use std::collections::hash_map::Values;

pub struct MethodIter<'a> {
    item: DocRef<'a, Item>,
    impl_block_iter: InherentImplBlockIter<'a>,
    current_item_iter: Option<std::slice::Iter<'a, Id>>,
}

impl<'a> MethodIter<'a> {
    pub(crate) fn new(item: DocRef<'a, Item>) -> Self {
        let impl_block_iter = InherentImplBlockIter::new(item);
        Self {
            item,
            impl_block_iter,
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

pub struct TraitIter<'a> {
    item: DocRef<'a, Item>,
    item_iter: Values<'a, Id, Item>,
}
impl<'a> TraitIter<'a> {
    fn new(item: DocRef<'a, Item>) -> Self {
        let item_iter = item.crate_docs().index.values();
        Self { item, item_iter }
    }
}

impl<'a> Iterator for TraitIter<'a> {
    type Item = DocRef<'a, Item>;

    fn next(&mut self) -> Option<Self::Item> {
        for item in &mut self.item_iter {
            if let ItemEnum::Impl(impl_block) = &item.inner
                && let Type::ResolvedPath(path) = &impl_block.for_
                && path.id == self.item.id
                && impl_block.trait_.is_some()
            {
                return Some(self.item.build_ref(item));
            }
        }
        None
    }
}

pub struct ImplementorIter<'a> {
    trait_item: DocRef<'a, Item>,
    item_iter: Values<'a, Id, Item>,
}

impl<'a> ImplementorIter<'a> {
    fn new(trait_item: DocRef<'a, Item>) -> Self {
        let item_iter = trait_item.crate_docs().index.values();
        Self {
            trait_item,
            item_iter,
        }
    }
}

impl<'a> Iterator for ImplementorIter<'a> {
    type Item = DocRef<'a, Item>;

    fn next(&mut self) -> Option<Self::Item> {
        for item in &mut self.item_iter {
            if let ItemEnum::Impl(impl_block) = &item.inner
                && let Some(trait_path) = &impl_block.trait_
                && trait_path.id == self.trait_item.id
                && !impl_block.is_negative
            {
                return Some(self.trait_item.build_ref(item));
            }
        }
        None
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

pub(crate) struct InherentImplBlockIter<'a> {
    item: DocRef<'a, Item>,
    item_iter: Values<'a, Id, Item>,
}

impl<'a> InherentImplBlockIter<'a> {
    pub(crate) fn new(item: DocRef<'a, Item>) -> Self {
        let item_iter = item.crate_docs().index.values();
        Self { item, item_iter }
    }
}

impl<'a> Iterator for InherentImplBlockIter<'a> {
    type Item = DocRef<'a, Item>;

    fn next(&mut self) -> Option<Self::Item> {
        for item in &mut self.item_iter {
            if let ItemEnum::Impl(impl_block) = &item.inner
                && let Type::ResolvedPath(path) = &impl_block.for_
                && path.id == self.item.id
                && impl_block.trait_.is_none()
            {
                return Some(DocRef::new(self.item.navigator(), self.item, item));
            }
        }
        None
    }
}
