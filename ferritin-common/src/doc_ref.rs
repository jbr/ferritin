use crate::{Navigator, RustdocData};
use fieldwork::Fieldwork;
use rustdoc_types::{Id, Item, ItemEnum, ItemKind, ItemSummary, MacroKind, ProcMacro, Use};
use std::{
    fmt::{self, Debug, Display, Formatter},
    ops::Deref,
};

#[derive(Fieldwork)]
#[fieldwork(get, option_set_some)]
pub struct DocRef<'a, T> {
    crate_docs: &'a RustdocData,
    item: &'a T,
    navigator: &'a Navigator,

    #[field(get = false, with, set)]
    name: Option<&'a str>,
}

// Equality based on item pointer and crate provenance
impl<'a, T> PartialEq for DocRef<'a, T> {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.item, other.item) && std::ptr::eq(self.crate_docs, other.crate_docs)
    }
}

impl<'a, T> Eq for DocRef<'a, T> {}

impl<'a, T> From<&DocRef<'a, T>> for &'a RustdocData {
    fn from(value: &DocRef<'a, T>) -> Self {
        value.crate_docs
    }
}
impl<'a, T> From<DocRef<'a, T>> for &'a RustdocData {
    fn from(value: DocRef<'a, T>) -> Self {
        value.crate_docs
    }
}

impl<'a, T> Deref for DocRef<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.item
    }
}

impl<'a, T> DocRef<'a, T> {
    pub fn build_ref<U>(&self, inner: &'a U) -> DocRef<'a, U> {
        DocRef::new(self.navigator, self.crate_docs, inner)
    }

    pub fn get_path(&self, id: Id) -> Option<DocRef<'a, Item>> {
        self.crate_docs.get_path(self.navigator, id)
    }
}

impl<'a> DocRef<'a, Item> {
    pub fn name(&self) -> Option<&'a str> {
        self.name
            .or(self.item.name.as_deref())
            .or(self.summary().and_then(|x| x.path.last().map(|y| &**y)))
    }

    pub fn inner(&self) -> &'a ItemEnum {
        &self.item.inner
    }

    pub fn path(&self) -> Option<Path<'a>> {
        self.crate_docs().path(&self.id)
    }

    pub fn summary(&self) -> Option<&'a ItemSummary> {
        self.crate_docs().paths.get(&self.id)
    }

    pub fn find_child(&self, child_name: &str) -> Option<DocRef<'a, Item>> {
        self.child_items()
            .find(|c| c.name().is_some_and(|n| n == child_name))
    }

    pub(crate) fn find_by_path<'b>(
        &self,
        mut iter: impl Iterator<Item = &'b String>,
    ) -> Option<DocRef<'a, Item>> {
        let Some(next) = iter.next() else {
            return Some(*self);
        };

        for child in self.child_items() {
            if let Some(name) = child.name()
                && name == next
            {
                return child.find_by_path(iter);
            }
        }

        None
    }

    pub fn kind(&self) -> ItemKind {
        match self.item.inner {
            ItemEnum::Module(_) => ItemKind::Module,
            ItemEnum::ExternCrate { .. } => ItemKind::ExternCrate,
            ItemEnum::Use(_) => ItemKind::Use,
            ItemEnum::Union(_) => ItemKind::Union,
            ItemEnum::Struct(_) => ItemKind::Struct,
            ItemEnum::StructField(_) => ItemKind::StructField,
            ItemEnum::Enum(_) => ItemKind::Enum,
            ItemEnum::Variant(_) => ItemKind::Variant,
            ItemEnum::Function(_) => ItemKind::Function,
            ItemEnum::Trait(_) => ItemKind::Trait,
            ItemEnum::TraitAlias(_) => ItemKind::TraitAlias,
            ItemEnum::Impl(_) => ItemKind::Impl,
            ItemEnum::TypeAlias(_) => ItemKind::TypeAlias,
            ItemEnum::Constant { .. } => ItemKind::Constant,
            ItemEnum::Static(_) => ItemKind::Static,
            ItemEnum::ExternType => ItemKind::ExternType,
            ItemEnum::ProcMacro(ProcMacro {
                kind: MacroKind::Attr,
                ..
            }) => ItemKind::ProcAttribute,
            ItemEnum::ProcMacro(ProcMacro {
                kind: MacroKind::Derive,
                ..
            }) => ItemKind::ProcDerive,
            ItemEnum::Macro(_)
            | ItemEnum::ProcMacro(ProcMacro {
                kind: MacroKind::Bang,
                ..
            }) => ItemKind::Macro,
            ItemEnum::Primitive(_) => ItemKind::Primitive,
            ItemEnum::AssocConst { .. } => ItemKind::AssocConst,
            ItemEnum::AssocType { .. } => ItemKind::AssocType,
        }
    }
}

impl<'a, T> Clone for DocRef<'a, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<'a, T> Copy for DocRef<'a, T> {}

impl<'a, T: Debug> Debug for DocRef<'a, T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("DocRef")
            .field("crate_docs", &self.crate_docs)
            .field("item", &self.item)
            .finish_non_exhaustive()
    }
}

impl<'a, T> DocRef<'a, T> {
    pub(crate) fn new(
        navigator: &'a Navigator,
        crate_docs: impl Into<&'a RustdocData>,
        item: &'a T,
    ) -> Self {
        let crate_docs = crate_docs.into();
        Self {
            navigator,
            crate_docs,
            item,
            name: None,
        }
    }

    pub fn get(&self, id: &Id) -> Option<DocRef<'a, Item>> {
        self.crate_docs.get(self.navigator, id)
    }
}

impl<'a> DocRef<'a, Use> {
    pub fn name(self) -> &'a str {
        self.name.unwrap_or(&self.item.name)
    }
}

#[derive(Debug)]
pub struct Path<'a>(&'a [String]);

impl<'a> From<&'a ItemSummary> for Path<'a> {
    fn from(value: &'a ItemSummary) -> Self {
        Self(&value.path)
    }
}

impl<'a> IntoIterator for Path<'a> {
    type Item = &'a str;

    type IntoIter = Box<dyn Iterator<Item = Self::Item> + 'a>;

    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.0.iter().map(|x| &**x))
    }
}

impl Display for Path<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        for (i, segment) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str("::")?;
            }
            f.write_str(segment)?;
        }
        Ok(())
    }
}

// Compile-time thread-safety assertions for DocRef
//
// DocRef holds references (&'a T, &'a Navigator, &'a RustdocData) which are Send
// when the referenced types are Sync. This is critical for the threading model:
// DocRef can be sent between threads in scoped thread scenarios.
#[allow(dead_code)]
const _: () = {
    const fn assert_send<T: Send>() {}
    const fn assert_sync<T: Sync>() {}

    // DocRef<'a, Item> must be Send (can cross thread boundaries in scoped threads)
    const fn check_doc_ref_send() {
        assert_send::<DocRef<'_, rustdoc_types::Item>>();
    }

    // DocRef<'a, Item> must be Sync (multiple threads can hold &DocRef safely)
    const fn check_doc_ref_sync() {
        assert_sync::<DocRef<'_, rustdoc_types::Item>>();
    }
};
