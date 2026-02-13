use crate::doc_ref::DocRef;
use fieldwork::Fieldwork;
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

    pub fn child_items(&self) -> ChildItems<'a> {
        ChildItems::new(*self)
    }
}

impl<'a, T> DocRef<'a, T> {
    pub fn id_iter(&self, ids: &'a [Id]) -> IdIter<'a, T> {
        IdIter::new(*self, ids)
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

impl<'a> Iterator for MethodIter<'a> {
    type Item = DocRef<'a, Item>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(current_item_iter) = &mut self.current_item_iter {
                for id in current_item_iter {
                    if let Some(item) = self.item.get(id) {
                        return Some(item);
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
pub struct IdIter<'a, T> {
    item: DocRef<'a, T>,
    id_iter: std::slice::Iter<'a, Id>,
    glob_iter: Option<Box<IdIter<'a, Item>>>,
    #[field(with)]
    include_use: bool,
}

impl<'a, T> IdIter<'a, T> {
    pub(crate) fn new(item: DocRef<'a, T>, ids: &'a [Id]) -> Self {
        Self {
            item,
            id_iter: ids.iter(),
            glob_iter: None,
            include_use: false,
        }
    }
}

impl<'a, T> Iterator for IdIter<'a, T> {
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

                        let source_item = use_item
                            .id
                            .and_then(|id| item.crate_docs().get(item.navigator(), &id))
                            .or_else(|| {
                                item.navigator().resolve_path(&use_item.source, &mut vec![])
                            })?;

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
                    return Some(item);
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
    Module(IdIter<'a, Item>),
    Use(Option<DocRef<'a, Use>>, Option<IdIter<'a, Item>>, bool),
    Enum(IdIter<'a, Item>, MethodIter<'a>),
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
                            use_item
                                .navigator()
                                .resolve_path(&use_item.source, &mut vec![])
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
        match &item.item().inner {
            ItemEnum::Module(module) => Self::Module(item.id_iter(&module.items)),
            ItemEnum::Enum(enum_item) => {
                Self::Enum(item.id_iter(&enum_item.variants), item.methods())
            }
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
