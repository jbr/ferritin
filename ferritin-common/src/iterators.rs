use crate::Navigator;
use crate::doc_ref::{DocRef, ParentRef};
use fieldwork::Fieldwork;
use rustdoc_types::{Id, Item, ItemEnum, Type, Use};
use std::collections::hash_map::Values;

/// Resolve a `Use::source` path via `Navigator::resolve_path`, rewriting any
/// leading relative prefix (`crate::`, `self::`, `super::super::…`) against the
/// containing module's path so it becomes an absolute path that `resolve_path`
/// can handle.
///
/// Rustdoc emits `Use::source` verbatim from the Rust source:
/// - `pub use crate::foo::Bar;`     → `"crate::foo::Bar"`
/// - `pub use self::Bar;`           → `"self::Bar"`
/// - `pub use super::super::Bar;`   → `"super::super::Bar"`
///
/// Passing any of those to `resolve_path` untreated makes it try to load a
/// crate literally named `"crate"`, `"self"`, or `"super"`. This helper
/// substitutes the correct prefix using the containing module's path so, e.g.,
/// `self::Bar` in module `home::inner` becomes `home::inner::Bar`.
///
/// `module_path` is the fully-qualified path of the module containing the use,
/// with the crate name as the first segment (e.g. `["home", "inner"]`). For a
/// use at the crate root, pass `["home"]`.
///
/// Returns `None` when too many `super::` prefixes would walk above the crate
/// root.
fn resolve_use_source<'a>(
    navigator: &'a Navigator,
    module_path: &[&str],
    source: &str,
) -> Option<DocRef<'a, Item>> {
    let rewritten = rewrite_relative_prefix(module_path, source)?;
    navigator.resolve_path(&rewritten, &mut vec![])
}

/// Rewrite a relative path prefix against `module_path`, returning the
/// absolute equivalent. Returns `None` only when `super::` prefixes exceed the
/// depth of `module_path`. Non-prefixed paths are returned unchanged (as owned
/// strings so the return type is uniform).
fn rewrite_relative_prefix(module_path: &[&str], source: &str) -> Option<String> {
    // Strip any number of leading `super::` segments, counting them.
    let mut supers = 0;
    let mut rest = source;
    while let Some(tail) = rest.strip_prefix("super::") {
        supers += 1;
        rest = tail;
    }
    if supers > 0 || rest == "super" {
        // A trailing bare `super` counts as one more level.
        let supers = if rest == "super" { supers + 1 } else { supers };
        let tail = if rest == "super" { "" } else { rest };
        let remaining = module_path.len().checked_sub(supers)?;
        if remaining == 0 {
            return None;
        }
        let prefix = module_path[..remaining].join("::");
        return Some(if tail.is_empty() {
            prefix
        } else {
            format!("{prefix}::{tail}")
        });
    }

    if let Some(tail) = source.strip_prefix("self::") {
        return Some(format!("{}::{}", module_path.join("::"), tail));
    }
    if source == "self" {
        return Some(module_path.join("::"));
    }

    if let Some(tail) = source.strip_prefix("crate::") {
        let crate_name = module_path.first().copied()?;
        return Some(format!("{crate_name}::{tail}"));
    }
    if source == "crate" {
        return module_path.first().copied().map(String::from);
    }

    Some(source.to_owned())
}

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

    pub fn child_items(&self) -> ChildItems<'a> {
        ChildItems::new(*self)
    }

    /// Iterate children without resolving `Use` sources. Each yielded
    /// [`LazyChild`] exposes a cheap `name()` (reading `use_item.name` directly
    /// for non-glob Uses, the item's own name otherwise) and an explicit
    /// `resolve()` that performs the source lookup only when the caller asks.
    ///
    /// Use this for name-based lookups (`find_named`, path traversal). Use
    /// [`Self::child_items`] when you need the resolved items unconditionally
    /// (display, indexing).
    pub fn lazy_children(&self) -> LazyChildren<'a> {
        LazyChildren::new(*self)
    }
}

impl<'a> DocRef<'a, Item> {
    pub fn id_iter(&self, ids: &'a [Id]) -> IdIter<'a> {
        IdIter::new(*self, ids)
    }
}

/// A child item where `Use` source resolution is deferred. Yielded by
/// [`LazyChildren`]; resolve only when the name (or other cheap predicate)
/// indicates the item is interesting.
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
            // Deref `DocRef<Use>` to its inner `&'a Use` to get a `&'a str`
            // (not borrowed from the temporary DocRef value).
            LazyChild::NonGlob { use_item, .. } => Some(&use_item.item().name),
            LazyChild::Glob { .. } => None,
        }
    }

    /// Resolve to a concrete item. For `NonGlob`, follows the Use's source
    /// (preferring `use_item.id` to avoid a string-based path lookup) and
    /// preserves the imported name. For `Glob`, returns the source module
    /// itself; the caller walks *its* children. Returns `None` if the source
    /// can't be resolved.
    pub fn resolve(self) -> Option<DocRef<'a, Item>> {
        match self {
            LazyChild::Item(item) => Some(item),
            LazyChild::NonGlob { use_item, parent } => {
                let name: &'a str = &use_item.item().name;
                resolve_use_target(use_item, parent).map(|item| item.with_name(name))
            }
            LazyChild::Glob { use_item, parent } => resolve_use_target(use_item, parent),
        }
    }
}

/// Resolve a `Use` to its target item, using `use_item.id` first and falling
/// back to a path-based lookup against `parent`'s containing module path
/// (so `self::`/`super::` resolve correctly).
fn resolve_use_target<'a>(
    use_item: DocRef<'a, Use>,
    parent: DocRef<'a, Item>,
) -> Option<DocRef<'a, Item>> {
    let use_inner = use_item.item();
    use_inner
        .id
        .and_then(|id| use_item.crate_docs().get(use_item.navigator(), &id))
        .or_else(|| {
            let module_path = parent.containing_module_path();
            resolve_use_source(use_item.navigator(), &module_path, &use_inner.source)
        })
}

/// Lazy iterator over a module/enum's direct children. Does not expand glob
/// Uses or resolve non-glob Uses; see [`LazyChild`] for what each yielded
/// variant carries.
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

impl<'a> DocRef<'a, Item> {
    /// Iterate impl blocks in this crate that implement this trait.
    pub fn implementors(&self) -> ImplementorIter<'a> {
        ImplementorIter::new(*self)
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

#[derive(Debug, Fieldwork)]
pub struct IdIter<'a> {
    item: DocRef<'a, Item>,
    id_iter: std::slice::Iter<'a, Id>,
    glob_iter: Option<Box<IdIter<'a>>>,
    #[field(with)]
    include_use: bool,
    #[field(with(vis = "pub(crate)", option_set_some, into))]
    parent: Option<ParentRef<'a>>,
}

impl<'a> IdIter<'a> {
    pub(crate) fn new(item: DocRef<'a, Item>, ids: &'a [Id]) -> Self {
        Self {
            item,
            id_iter: ids.iter(),
            glob_iter: None,
            include_use: false,
            parent: None,
        }
    }
}

impl<'a> Iterator for IdIter<'a> {
    type Item = DocRef<'a, Item>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(glob_iter) = self.glob_iter.as_mut() {
                if let Some(item) = glob_iter.next() {
                    return Some(item);
                } else {
                    self.glob_iter = None;
                }
            }

            for id in &mut self.id_iter {
                if let Some(item) = self.item.get(id) {
                    if let ItemEnum::Use(use_item) = item.inner() {
                        if self.include_use {
                            return Some(item);
                        }

                        let Some(source_item) = use_item
                            .id
                            .and_then(|id| item.crate_docs().get(item.navigator(), &id))
                            .or_else(|| {
                                // `self.item` is the module containing these use items,
                                // so its path is what `self::` / `super::` are relative to.
                                let module_path = self.item.containing_module_path();
                                resolve_use_source(item.navigator(), &module_path, &use_item.source)
                            })
                        else {
                            // One unresolvable re-export shouldn't abort the whole iteration;
                            // skip it and keep yielding the remaining siblings.
                            continue;
                        };

                        if use_item.is_glob {
                            self.glob_iter = match source_item.inner() {
                                ItemEnum::Module(module) => {
                                    Some(Box::new(source_item.id_iter(&module.items)))
                                }
                                ItemEnum::Enum(enum_item) => {
                                    Some(Box::new(source_item.id_iter(&enum_item.variants)))
                                }
                                _ => None,
                            };

                            break;
                        } else {
                            return Some(source_item.with_name(&use_item.name));
                        }
                    }
                    return Some(match self.parent {
                        Some(parent) => item.with_parent(parent),
                        None => item,
                    });
                }
            }
            if self.glob_iter.is_none() {
                break;
            }
        }

        None
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

pub enum ChildItems<'a> {
    AssociatedMethods(MethodIter<'a>),
    Module(IdIter<'a>),
    Use(Option<DocRef<'a, Use>>, Option<IdIter<'a>>, bool),
    Enum(IdIter<'a>, MethodIter<'a>),
    None,
}

impl<'a> Iterator for ChildItems<'a> {
    type Item = DocRef<'a, Item>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self {
                ChildItems::AssociatedMethods(method_iter) => return method_iter.next(),
                ChildItems::Module(id_iter) => return id_iter.next(),
                ChildItems::Enum(id_iter, method_iter) => {
                    return id_iter.next().or_else(|| method_iter.next());
                }
                ChildItems::Use(_, Some(id_iter), _) => return id_iter.next(),
                ChildItems::Use(use_item_option @ Some(_), id_iter @ None, include_use) => {
                    let use_item = use_item_option.take()?;

                    let name = use_item.use_name();

                    let source_item = use_item
                        .id
                        .and_then(|id| use_item.get(&id))
                        .or_else(|| {
                            // ChildItems::Use doesn't track the use's parent module, so
                            // `self::`/`super::` resolution is degraded here — we fall
                            // back to the crate root. In practice this branch is only
                            // taken when someone calls `.child_items()` directly on a
                            // Use item (rare); the common module-iteration path uses
                            // `IdIter` with accurate module context.
                            let crate_name = use_item.crate_docs().name();
                            resolve_use_source(
                                use_item.navigator(),
                                &[crate_name],
                                &use_item.source,
                            )
                        })?
                        .with_name(name);

                    if use_item.is_glob {
                        match source_item.inner() {
                            ItemEnum::Module(module) => {
                                *id_iter = Some(
                                    source_item
                                        .id_iter(&module.items)
                                        .with_include_use(*include_use),
                                );
                            }
                            ItemEnum::Enum(enum_item) => {
                                *id_iter = Some(
                                    source_item
                                        .id_iter(&enum_item.variants)
                                        .with_include_use(*include_use),
                                );
                            }
                            _ => {
                                return None;
                            }
                        }
                    } else if let ItemEnum::Use(ui) = source_item.inner()
                        && !*include_use
                    {
                        *use_item_option = Some(source_item.build_ref(ui));
                    } else {
                        return Some(source_item);
                    }
                }

                ChildItems::Use(_, _, _) => return None,

                ChildItems::None => return None,
            }
        }
    }
}

impl<'a> ChildItems<'a> {
    pub(crate) fn new(item: DocRef<'a, Item>) -> Self {
        let parent = ParentRef::from(item);
        match &item.item().inner {
            ItemEnum::Module(module) => {
                Self::Module(item.id_iter(&module.items).with_parent(parent))
            }
            ItemEnum::Enum(enum_item) => Self::Enum(
                item.id_iter(&enum_item.variants).with_parent(parent),
                item.methods(),
            ),
            ItemEnum::Struct(_) => Self::AssociatedMethods(item.methods()),
            ItemEnum::Use(use_item) => ChildItems::Use(Some(item.build_ref(use_item)), None, false),
            _ => Self::None,
        }
    }

    pub(crate) fn with_use(self) -> Self {
        match self {
            ChildItems::AssociatedMethods(method_iter) => {
                ChildItems::AssociatedMethods(method_iter)
            }
            ChildItems::Module(id_iter) => ChildItems::Module(id_iter.with_include_use(true)),
            ChildItems::Enum(id_iter, method_iter) => {
                ChildItems::Enum(id_iter.with_include_use(true), method_iter)
            }
            ChildItems::Use(item, Some(id_iter), _) => {
                ChildItems::Use(item, Some(id_iter.with_include_use(true)), true)
            }
            ChildItems::Use(item, None, _) => ChildItems::Use(item, None, true),
            ChildItems::None => ChildItems::None,
        }
    }
}
