use crate::{
    Navigator, RustdocData, navigator::parse_docsrs_url, rustdoc_data::kind_discriminator,
};
use fieldwork::Fieldwork;
use rustdoc_types::{
    ExternalCrate, Id, Item, ItemEnum, ItemKind, ItemSummary, MacroKind, ProcMacro, Use,
};

/// A lightweight, `Copy` reference to a parent item set during tree traversal.
///
/// Stored on [`DocRef`] to enable [`DocRef::discriminated_path`] for items absent from
/// rustdoc's `paths` map (e.g. inherent methods; rust-lang/rust#152511). One level is
/// sufficient because only impl-block items are orphaned, and their parents (structs,
/// enums, traits) always have an `ItemSummary`.
#[derive(Copy, Clone, Debug)]
pub(crate) struct ParentRef<'a> {
    pub(crate) crate_docs: &'a RustdocData,
    pub(crate) item: &'a Item,
    /// The name override from the parent's [`DocRef`], if it was set (e.g. by a re-export).
    pub(crate) name: Option<&'a str>,
}

impl<'a> From<DocRef<'a, Item>> for ParentRef<'a> {
    fn from(d: DocRef<'a, Item>) -> Self {
        ParentRef {
            crate_docs: d.crate_docs,
            item: d.item,
            name: d.name,
        }
    }
}
use semver::VersionReq;
use std::{
    fmt::{self, Debug, Display, Formatter},
    hash::{Hash, Hasher},
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

    /// Parent item set during tree traversal; used by [`DocRef::discriminated_path`] as a
    /// fallback for items absent from rustdoc's `paths` map (rust-lang/rust#152511).
    #[field(get = false, with(vis = "pub(crate)", option_set_some, into))]
    parent: Option<ParentRef<'a>>,
}

// Equality based on item pointer and crate provenance
impl<'a, T> PartialEq for DocRef<'a, T> {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.item, other.item) && std::ptr::eq(self.crate_docs, other.crate_docs)
    }
}

impl<'a, T> Eq for DocRef<'a, T> {}

impl Hash for DocRef<'_, Item> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.crate_docs.name().hash(state);
        self.id.hash(state);
    }
}

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

    /// The fully-qualified path of this item's module, for use in resolving
    /// relative path prefixes (`self::`, `super::`, `crate::`) in `Use` sources
    /// and intra-doc links.
    ///
    /// For a module item, returns the module's own path (e.g. `["home", "inner"]`
    /// for a module `home::inner`, or `["home"]` for a root module).
    /// For other items in the paths map, returns the containing module path
    /// (the ItemSummary path with the last segment dropped).
    /// For items absent from the paths map (e.g. `Use` items, which never have
    /// summaries), falls back to `[crate_name]` — correct for `crate::` but a
    /// degraded best-effort for `self::`/`super::`. Callers that need accurate
    /// self/super resolution for a `Use` must obtain the path from the use's
    /// *parent* module instead.
    pub fn containing_module_path(&self) -> Vec<&'a str> {
        if let Some(summary) = self.summary() {
            return match self.kind() {
                ItemKind::Module => summary.path.iter().map(String::as_str).collect(),
                _ => summary
                    .path
                    .iter()
                    .take(summary.path.len().saturating_sub(1))
                    .map(String::as_str)
                    .collect(),
            };
        }
        vec![self.crate_docs().name()]
    }

    /// Find a direct or transitively glob-imported child by name.
    ///
    /// Two-phase walk: first matches against direct children's names without
    /// resolving any `Use` source (cheap; handles `pub use self::error;`-style
    /// self-loops because the real `error` submodule is a direct child and is
    /// found before the Use is ever inspected). Only if no direct match exists
    /// do we look through glob Uses — and even then only one step at a time,
    /// guarded against re-entering the same (module, name) pair.
    pub fn find_child(&self, child_name: &str) -> Option<DocRef<'a, Item>> {
        crate::lookup::find_named(*self, child_name)
    }

    pub fn find_by_path<'b>(
        &self,
        mut iter: impl Iterator<Item = &'b String>,
    ) -> Option<DocRef<'a, Item>> {
        let Some(next) = iter.next() else {
            return Some(*self);
        };
        let child = self.find_child(next)?;
        child.find_by_path(iter)
    }

    /// Returns the fully-qualified, kind-discriminated path for this item, suitable for
    /// round-tripping through `Navigator::resolve_path`.
    ///
    /// For example, a `Vec` struct in `std::vec` returns `"std::vec::struct@Vec"`, and the
    /// `vec` module itself returns `"std::mod@vec"`. The crate name is included as the first
    /// segment; the discriminator (`kind@`) appears only on the final segment.
    ///
    /// Uses `crate_docs().name()` (the Navigator's canonical crate name) rather than
    /// `ItemSummary::path[0]` (which rustdoc normalizes to underscores) so that the
    /// generated path round-trips correctly through `Navigator::resolve_path`.
    ///
    /// Returns `None` if the item has no `ItemSummary` entry in the crate's paths map.
    pub fn discriminated_path(&self) -> Option<String> {
        if let Some(summary) = self.summary() {
            // Fast path: use the ItemSummary path directly.
            // path[0] is the crate name as rustdoc sees it (underscored); use the Navigator's
            // canonical name instead so the result can be fed back into resolve_path.
            let path = &summary.path;
            let tail = path.get(1..)?;
            let disc = kind_discriminator(self.kind());
            let crate_name = self.crate_docs().name();
            return match tail {
                [] => Some(format!("{crate_name}::{disc}@{}", path[0])),
                [.., last] => {
                    let prefix = tail[..tail.len() - 1].join("::");
                    if prefix.is_empty() {
                        Some(format!("{crate_name}::{disc}@{last}"))
                    } else {
                        Some(format!("{crate_name}::{prefix}::{disc}@{last}"))
                    }
                }
            };
        }

        // Fallback for items absent from rustdoc's paths map (e.g. inherent methods;
        // rust-lang/rust#152511). Requires a parent set during tree traversal.
        let parent_ref = self.parent?;
        let disc = kind_discriminator(self.kind());
        let name = self.item.name.as_deref()?;

        // Prefer a path without a discriminator on the parent segment (simpler output).
        // The unqualified key is only present in path_to_id when there is no collision at
        // that path, so its presence is a reliable signal that we can omit the discriminator.
        if let Some(parent_summary) = parent_ref.crate_docs.paths.get(&parent_ref.item.id)
            && let Some(tail) = parent_summary.path.get(1..)
        {
            let parent_key = tail.join("::");
            if parent_ref.crate_docs.path_to_id.contains_key(&parent_key) {
                let crate_name = parent_ref.crate_docs.name();
                let parent_path = if parent_key.is_empty() {
                    crate_name.to_string()
                } else {
                    format!("{crate_name}::{parent_key}")
                };
                return Some(format!("{parent_path}::{disc}@{name}"));
            }
        }

        // Collision at the parent level: fall back to the fully-discriminated parent path.
        let parent = DocRef::new(self.navigator, parent_ref.crate_docs, parent_ref.item);
        let parent = match parent_ref.name {
            Some(n) => parent.with_name(n),
            None => parent,
        };
        let parent_path = parent.discriminated_path()?;
        Some(format!("{parent_path}::{disc}@{name}"))
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
            parent: None,
        }
    }

    pub fn get(&self, id: &Id) -> Option<DocRef<'a, Item>> {
        self.crate_docs.get(self.navigator, id)
    }
}

impl<'a> DocRef<'a, Use> {
    pub fn use_name(self) -> &'a str {
        self.name.unwrap_or(&self.item.name)
    }
}

impl<'a> DocRef<'a, ItemSummary> {
    /// Get the external crate this item summary refers to, if any.
    /// Returns None if crate_id == 0 (same crate).
    pub fn external_crate(&self) -> Option<DocRef<'a, ExternalCrate>> {
        if self.crate_id == 0 {
            return None;
        }

        let external = self.crate_docs().external_crates.get(&self.crate_id)?;
        Some(self.build_ref(external))
    }
}

impl<'a> DocRef<'a, ExternalCrate> {
    /// Get the canonical name of this external crate.
    /// Parses html_root_url if available, falls back to the name field.
    pub fn crate_name(&self) -> &'a str {
        if let Some(url) = &self.item.html_root_url
            && let Some((name, _)) = parse_docsrs_url(url)
        {
            return name;
        }
        &self.item.name
    }

    /// Load the RustdocData for this external crate.
    pub fn load(&self) -> Option<&'a RustdocData> {
        let name = self.crate_name();
        let version_req = if let Some(url) = &self.item.html_root_url {
            parse_docsrs_url(url)
                .and_then(|(_, version)| VersionReq::parse(&format!("={version}")).ok())
                .unwrap_or(VersionReq::STAR)
        } else {
            VersionReq::STAR
        };

        self.navigator().load_crate(name, &version_req)
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
