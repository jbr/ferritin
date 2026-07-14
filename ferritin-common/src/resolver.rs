//! Resolver — per-query navigation engine over a [`Navigator`].
//!
//! A [`Resolver`] holds a borrow on a [`Navigator`] (the long-lived data store
//! of loaded crates) plus the mutable state of a single navigation: a frame
//! stack used both for cycle detection and for tracking the path traversed.
//!
//! # The cycle problem
//!
//! Path-string resolution can cycle through `Use` re-exports across crate
//! boundaries. The classic case is mutual re-exports between `askama` and
//! `askama_macros`: resolving a Use's source path lands on another Use,
//! whose source path lands on yet another Use, eventually back at the first.
//! The cycle never repeats the same `(parent, target)` pair within a single
//! [`Resolver::find_named`] call, so a guard local to one name lookup misses
//! it. The cycle does, however, repeat the same frame across different
//! lookups in the same chain — which is what the resolver-wide stack catches.
//!
//! # Frame discipline
//!
//! Two kinds of frame are pushed:
//! - **`Some(target)`** while looking up name `target` in some parent module.
//! - **`None`** while following a `Use` to its target.
//!
//! Pushing a frame already on the stack (same item key, same segment) returns
//! `None` from the scoped `with_pushed` guard. The frame is popped when the
//! guard's scope ends, including after a successful resolution — the stack
//! drains to empty between top-level calls.
//!
//! # Semantic path
//!
//! [`Resolver::current_path`] reads the segments off the stack in order, which
//! is the actual public route walked to reach the current item. This is more
//! accurate than `DocRef::discriminated_path` for items reached via
//! re-exports, where the canonical path may go through private modules.

use crate::{
    DocRef, Navigator, RustdocData,
    doc_ref::ParentRef,
    iterators::{LazyChild, LazyChildren},
    string_utils::case_aware_jaro_winkler,
};
use fieldwork::Fieldwork;
use rustdoc_types::{Id, Item, ItemEnum, ItemKind, Use, Visibility};
use semver::VersionReq;

/// Suggestion produced when path resolution fails — a near-miss path the user
/// might have meant.
#[derive(Fieldwork)]
#[fieldwork(get)]
pub struct Suggestion<'a> {
    path: String,
    item: Option<DocRef<'a, Item>>,
    score: f64,
}

impl<'a> Suggestion<'a> {
    /// Crate-name suggestions for a failed crate load, scored against the
    /// requested name.
    pub(crate) fn for_crate_name(navigator: &'a Navigator, requested: &str) -> Vec<Self> {
        navigator
            .list_available_crates()
            .map(|crate_info| Suggestion {
                path: crate_info.name().to_string(),
                item: None,
                score: case_aware_jaro_winkler(crate_info.name(), requested),
            })
            .collect()
    }
}

/// Stable identity for any borrowed item, used as the cycle-detection key.
///
/// Both addresses are stable for the lifetime of the `Navigator`: `crate_docs`
/// because `working_set` is a `FrozenMap` that never moves entries; the inner
/// pointer because each `Item` lives at a fixed address in either the resident
/// `Crate` (cold path) or the append-only `item_cache` (warm path) — a given
/// `Id` is only ever materialized in one of them, so revisiting an item yields
/// the same address. We compare addresses as `usize` so the key stays
/// `Send + Sync` and `Hash`able without unsafe.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) struct ItemKey {
    crate_docs: usize,
    item: usize,
}

impl<'a, T> From<DocRef<'a, T>> for ItemKey {
    fn from(d: DocRef<'a, T>) -> Self {
        Self {
            crate_docs: d.crate_docs() as *const RustdocData as usize,
            item: d.item() as *const T as usize,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
struct Frame {
    item: ItemKey,
    /// `None` while following a `Use`; `Some(name)` while looking up `name`
    /// inside `item`. Owned so the frame can outlive the caller's borrow of
    /// the segment string (e.g. a slice of the user's input `path`).
    segment: Option<String>,
}

/// A navigation engine over a [`Navigator`].
///
/// Construct one per query (e.g. one per top-level command in a `Request`).
/// The frame stack drains to empty between top-level methods, so a single
/// `Resolver` is reusable across many lookups in the same operation.
///
/// Methods that walk through items take `&mut self` because resolution
/// mutates the frame stack. Returned `DocRef`s borrow from the underlying
/// `Navigator`, so they outlive the `Resolver`.
pub struct Resolver<'a> {
    navigator: &'a Navigator,
    stack: Vec<Frame>,
}

impl<'a> Resolver<'a> {
    pub fn new(navigator: &'a Navigator) -> Self {
        Self {
            navigator,
            stack: Vec::new(),
        }
    }

    pub fn navigator(&self) -> &'a Navigator {
        self.navigator
    }

    /// Segments traversed in the current resolution chain. Empty between
    /// top-level calls.
    pub fn current_path(&self) -> impl Iterator<Item = &str> + '_ {
        self.stack.iter().filter_map(|f| f.segment.as_deref())
    }

    /// Push a frame, run `f`, pop. Returns `None` if the frame is already on
    /// the stack (cycle).
    fn with_pushed<R>(
        &mut self,
        item: ItemKey,
        segment: Option<&str>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> Option<R> {
        let frame = Frame {
            item,
            segment: segment.map(str::to_owned),
        };
        if self.stack.contains(&frame) {
            log::trace!("cycle: {frame:?}");
            return None;
        }
        self.stack.push(frame);
        let r = f(self);
        self.stack.pop();
        Some(r)
    }
}

// ---------- top-level path resolution ----------

impl<'a> Resolver<'a> {
    /// Resolve a path like `"std::vec::Vec"` or `"tokio@1::runtime::Runtime"`.
    ///
    /// Primary string entrypoint for any user-supplied crate or item path.
    pub fn resolve_path(
        &mut self,
        mut path: &str,
        suggestions: &mut Vec<Suggestion<'a>>,
    ) -> Option<DocRef<'a, Item>> {
        if let Some(p) = path.strip_prefix("::") {
            path = p;
        }

        let (crate_specifier, path_start_index) = if let Some(first_scope) = path.find("::") {
            (&path[..first_scope], Some(first_scope + 2))
        } else {
            (path, None)
        };

        let (crate_name, version_req) = if let Some(at) = crate_specifier.find("@") {
            (
                &crate_specifier[..at],
                VersionReq::parse(&crate_specifier[at + 1..]).unwrap_or(VersionReq::STAR),
            )
        } else {
            (crate_specifier, VersionReq::STAR)
        };

        let Some(crate_data) = self.navigator.load_crate(crate_name, &version_req) else {
            suggestions.extend(Suggestion::for_crate_name(self.navigator, crate_name));
            return None;
        };

        let item = crate_data.get(self.navigator, crate_data.root_id())?;
        let Some(path_start_index) = path_start_index else {
            return Some(item);
        };

        if let Some(item) = self.find_children_recursive(item, path, path_start_index, suggestions)
        {
            return Some(item);
        }

        let suffix = &path[path_start_index..];

        // Fallback: rustdoc paths map. Catches paths through private modules
        // not visible in the public item tree.
        if let Some(item) = crate_data
            .path_to_id
            .get(suffix)
            .and_then(|id| crate_data.get_item(id))
            .map(|item| DocRef::new(self.navigator, crate_data, item))
        {
            return Some(item);
        }

        // Second fallback: parent in paths map but child orphaned (impl-block
        // items missing from rustdoc's paths). Strip the last segment and
        // walk into the child.
        if let Some(sep) = suffix.rfind("::") {
            let parent_suffix = &suffix[..sep];
            let child_start = path_start_index + sep + 2;
            if let Some(parent_id) = crate_data.path_to_id.get(parent_suffix)
                && let Some(parent_item) = crate_data.get_item(parent_id)
            {
                let parent_ref = DocRef::new(self.navigator, crate_data, parent_item);
                return self.find_children_recursive(parent_ref, path, child_start, suggestions);
            }
        }

        None
    }

    /// Walk path segments starting at `index`, looking up each segment as a
    /// child of the current item.
    fn find_children_recursive(
        &mut self,
        item: DocRef<'a, Item>,
        path: &str,
        index: usize,
        suggestions: &mut Vec<Suggestion<'a>>,
    ) -> Option<DocRef<'a, Item>> {
        let remaining = &path[path.len().min(index)..];
        if remaining.is_empty() {
            return Some(item);
        }
        let segment_end = remaining
            .find("::")
            .map(|x| index + x)
            .unwrap_or(path.len());
        let segment = &path[index..segment_end];
        let next_segment_start = path.len().min(segment_end + 2);

        let (kind_filter, segment_name) = parse_discriminated_segment(segment);

        log::trace!(
            "🔎 searching for {segment_name} (kind={kind_filter:?}) in {} ({:?}) (remaining {})",
            &path[..index],
            item.kind(),
            &path[next_segment_start..]
        );

        if let Some(found) = self.find_named(item, segment_name, |resolver, candidate| {
            if !kind_filter.is_none_or(|k| candidate.kind() == k) {
                return None;
            }
            resolver.find_children_recursive(candidate, path, next_segment_start, suggestions)
        }) {
            return Some(found);
        }

        suggestions.extend(self.generate_suggestions(item, path, index));
        None
    }

    fn generate_suggestions(
        &mut self,
        item: DocRef<'a, Item>,
        path: &str,
        index: usize,
    ) -> Vec<Suggestion<'a>> {
        // Eager-collect — walking children involves Use resolution that needs
        // &mut self, so we can't return a borrowing iterator.
        let prefix = path[..index].to_owned();
        let path_owned = path.to_owned();
        let mut candidates = self.children(item);
        // Trait-declared associated items aren't part of `children()` (see
        // `find_named_dyn`); include them so a mistyped member like
        // `Deref::derefx` suggests the real members rather than only siblings.
        if let ItemEnum::Trait(trait_) = item.inner() {
            candidates.extend(trait_.items.iter().filter_map(|id| item.get(id)));
        }
        candidates
            .into_iter()
            .filter_map(move |item| {
                item.name().and_then(|name| {
                    let full_path = format!("{prefix}{name}");
                    if path_owned.starts_with(&full_path) {
                        None
                    } else {
                        let score = case_aware_jaro_winkler(&path_owned, &full_path);
                        Some(Suggestion {
                            path: full_path,
                            score,
                            item: Some(item),
                        })
                    }
                })
            })
            .collect()
    }
}

// ---------- name lookup with cycle detection ----------

impl<'a> Resolver<'a> {
    /// Find a child of `parent` named `target`. Used for single-segment lookups
    /// (e.g. walking `item_summary.path` to wire up "Defined at" links).
    pub fn find_child(
        &mut self,
        parent: DocRef<'a, Item>,
        target: &str,
    ) -> Option<DocRef<'a, Item>> {
        self.find_named(parent, target, |_, candidate| Some(candidate))
    }

    /// Walk a sequence of name segments from `parent`.
    pub fn find_by_path<'s>(
        &mut self,
        parent: DocRef<'a, Item>,
        segments: impl IntoIterator<Item = &'s str>,
    ) -> Option<DocRef<'a, Item>> {
        let mut current = parent;
        for segment in segments {
            current = self.find_child(current, segment)?;
        }
        Some(current)
    }

    /// Find a child of `parent` with imported name `target`, calling `accept`
    /// on each candidate. The first candidate for which `accept` returns
    /// `Some` wins; `None` continues to the next candidate.
    ///
    /// Two phases: direct children first (real items, then non-glob Uses),
    /// then transitive glob expansion. Methods on struct/enum/union/trait are
    /// considered before children.
    ///
    /// Cycle protection: pushes `(parent, target)` onto the resolver stack;
    /// returns `None` if the frame is already present.
    fn find_named<F>(
        &mut self,
        parent: DocRef<'a, Item>,
        target: &str,
        mut accept: F,
    ) -> Option<DocRef<'a, Item>>
    where
        F: FnMut(&mut Self, DocRef<'a, Item>) -> Option<DocRef<'a, Item>>,
    {
        // Indirect through a dyn-FnMut so the recursive call inside
        // find_named_dyn doesn't re-monomorphize with a fresh `&mut F` on
        // each level (which would blow up rustc's recursion limit).
        self.find_named_dyn(parent, target, &mut accept)
    }

    #[allow(clippy::type_complexity, reason = "it's fine")]
    fn find_named_dyn(
        &mut self,
        parent: DocRef<'a, Item>,
        target: &str,
        accept: &mut dyn FnMut(&mut Self, DocRef<'a, Item>) -> Option<DocRef<'a, Item>>,
    ) -> Option<DocRef<'a, Item>> {
        self.with_pushed(parent.into(), Some(target), |resolver| {
            // Methods first (inherent + provided trait methods on
            // struct/enum/union/trait). They can't be re-exports.
            if matches!(
                parent.inner(),
                ItemEnum::Struct(_) | ItemEnum::Enum(_) | ItemEnum::Union(_) | ItemEnum::Trait(_)
            ) {
                for method in parent.methods() {
                    if method.name() == Some(target)
                        && let Some(found) = accept(resolver, method)
                    {
                        return Some(found);
                    }
                }
            }

            // Trait-declared associated items (required/provided methods, assoc
            // types, assoc consts) live in `Trait.items`, not in impl blocks, so
            // `methods()` above misses them. They're always real items, never
            // re-exports.
            if let ItemEnum::Trait(trait_) = parent.inner() {
                for assoc in trait_.items.iter().filter_map(|id| parent.get(id)) {
                    if assoc.name() == Some(target)
                        && let Some(found) = accept(resolver, assoc.with_parent(parent))
                    {
                        return Some(found);
                    }
                }
            }

            // Phase 1: direct module/enum children, walked in source order.
            //
            // We collect classified candidates into a single ordered Vec so the
            // borrow on `LazyChildren` is dropped before we re-borrow the
            // resolver to resolve Uses. Mixing direct items and non-glob Uses
            // by source order matters: rustdoc emission order encodes the
            // user's intent, so e.g. a `pub use core::primitive::i128 as i128;`
            // before a deprecated `pub mod i128` should win the lookup.
            enum Phase1<'a> {
                Direct(DocRef<'a, Item>),
                NonGlob(DocRef<'a, Use>),
            }
            let mut phase1: Vec<Phase1<'a>> = Vec::new();
            let mut globs = Vec::<DocRef<'a, Use>>::new();
            for child in LazyChildren::new(parent) {
                match child {
                    LazyChild::Item(item) => {
                        if item.name() == Some(target) {
                            phase1.push(Phase1::Direct(item));
                        }
                    }
                    LazyChild::NonGlob { use_item, .. } => {
                        if use_item.item().name == target {
                            phase1.push(Phase1::NonGlob(use_item));
                        }
                    }
                    LazyChild::Glob { use_item, .. } => {
                        globs.push(use_item);
                    }
                }
            }

            for cand in phase1 {
                let resolved = match cand {
                    Phase1::Direct(item) => item,
                    Phase1::NonGlob(use_item) => {
                        let imported_name: &'a str = &use_item.item().name;
                        let Some(target_item) = resolver.follow_use(use_item, parent) else {
                            continue;
                        };
                        target_item.with_name(imported_name)
                    }
                };
                if let Some(found) = accept(resolver, resolved) {
                    return Some(found);
                }
            }

            // Phase 2: glob expansion. The resolver stack already protects
            // against cycles via the `(parent, target)` frame, so the source
            // module's own `find_named` recursion can't loop back here.
            for glob_use in globs {
                if let Some(source_module) = resolver.follow_use(glob_use, parent)
                    && let Some(found) = resolver.find_named_dyn(source_module, target, accept)
                {
                    return Some(found);
                }
            }

            None
        })
        .flatten()
    }
}

// ---------- Use resolution ----------

impl<'a> Resolver<'a> {
    /// Resolve a `Use` to its target item. Tries `use_item.id` first (cheap
    /// local lookup, can't cycle); on miss, falls back to resolving the
    /// source path string against the parent module's path.
    ///
    /// `parent_module` is the module containing the `Use`, used to resolve
    /// `self::`/`super::` prefixes in the source string.
    pub fn follow_use(
        &mut self,
        use_item: DocRef<'a, Use>,
        parent_module: DocRef<'a, Item>,
    ) -> Option<DocRef<'a, Item>> {
        let use_inner = use_item.item();
        // An id present in this crate's index *is* a local item — index
        // membership is the definitive locality test, so this is deterministic,
        // not a guess. If the id is absent from the index it's a foreign
        // reference; `get_path` reads the `paths` summary to cross into the
        // owning crate and resolve there. Only when neither succeeds (no id, or
        // an id that names no reachable item) do we fall through to parsing the
        // `source` string. That string is the last resort precisely because its
        // leading segment can be a local alias: quinn re-exports `quinn_proto`
        // as `proto` and `quinn_udp` as `udp`, so a `source` of
        // `proto::ServerConfig` / `udp` would otherwise load the unrelated
        // `proto` / `udp` crates from crates.io.
        if let Some(id) = use_inner.id {
            if let Some(target) = use_item.crate_docs().get(self.navigator, &id) {
                return Some(target);
            }
            if let Some(target) = self.get_path(parent_module, id) {
                return Some(target);
            }
        }

        self.with_pushed(use_item.into(), None, |resolver| {
            let module_path = parent_module.containing_module_path();
            let rewritten = rewrite_relative_prefix(&module_path, &use_inner.source)?;
            resolver.resolve_path(&rewritten, &mut vec![])
        })
        .flatten()
    }

    /// Resolve a [`LazyChild`] to a concrete item. For non-glob Uses, the
    /// imported name is preserved on the returned `DocRef`.
    pub fn resolve_lazy_child(&mut self, child: LazyChild<'a>) -> Option<DocRef<'a, Item>> {
        match child {
            LazyChild::Item(item) => Some(item),
            LazyChild::NonGlob { use_item, parent } => {
                let name: &'a str = &use_item.item().name;
                self.follow_use(use_item, parent).map(|i| i.with_name(name))
            }
            LazyChild::Glob { use_item, parent } => self.follow_use(use_item, parent),
        }
    }
}

// ---------- child iteration ----------

impl<'a> Resolver<'a> {
    /// Children of `item` with `Use`s resolved and globs expanded. Equivalent
    /// to the old `item.child_items()`.
    pub fn children(&mut self, item: DocRef<'a, Item>) -> Vec<DocRef<'a, Item>> {
        let mut out = Vec::new();
        self.collect_children(item, false, ParentRef::from(item), &mut out);
        out
    }

    /// Children of `item`, but yield `Use` items themselves rather than their
    /// targets. Used by the search indexer to record `use` statements.
    pub fn children_including_uses(&mut self, item: DocRef<'a, Item>) -> Vec<DocRef<'a, Item>> {
        let mut out = Vec::new();
        self.collect_children(item, true, ParentRef::from(item), &mut out);
        out
    }

    /// Resolve a slice of `Id`s in `parent`, expanding `Use` items. Replaces
    /// the old `id_iter`.
    pub fn ids(&mut self, parent: DocRef<'a, Item>, ids: &'a [Id]) -> Vec<DocRef<'a, Item>> {
        let mut out = Vec::new();
        self.collect_ids(parent, ids, false, Some(ParentRef::from(parent)), &mut out);
        out
    }

    /// Like [`Self::ids`] but yields `Use` items themselves instead of their
    /// targets.
    pub fn ids_including_uses(
        &mut self,
        parent: DocRef<'a, Item>,
        ids: &'a [Id],
    ) -> Vec<DocRef<'a, Item>> {
        let mut out = Vec::new();
        self.collect_ids(parent, ids, true, Some(ParentRef::from(parent)), &mut out);
        out
    }

    fn collect_children(
        &mut self,
        item: DocRef<'a, Item>,
        include_uses: bool,
        parent_ref: ParentRef<'a>,
        out: &mut Vec<DocRef<'a, Item>>,
    ) {
        match item.inner() {
            ItemEnum::Module(module) => {
                self.collect_ids(item, &module.items, include_uses, Some(parent_ref), out);
            }
            ItemEnum::Enum(enum_item) => {
                self.collect_ids(
                    item,
                    &enum_item.variants,
                    include_uses,
                    Some(parent_ref),
                    out,
                );
                out.extend(item.methods());
            }
            ItemEnum::Struct(_) | ItemEnum::Union(_) => {
                out.extend(item.methods());
            }
            ItemEnum::Use(use_item) => {
                let use_ref = item.build_ref(use_item);
                self.collect_use_children(
                    use_ref,
                    item,
                    &item.item().visibility,
                    include_uses,
                    out,
                );
            }
            _ => {}
        }
    }

    fn collect_ids(
        &mut self,
        parent: DocRef<'a, Item>,
        ids: &'a [Id],
        include_uses: bool,
        parent_ref: Option<ParentRef<'a>>,
        out: &mut Vec<DocRef<'a, Item>>,
    ) {
        for id in ids {
            let Some(item) = parent.get(id) else { continue };
            if let ItemEnum::Use(use_item) = item.inner() {
                if include_uses {
                    out.push(item);
                    continue;
                }
                let use_ref = item.build_ref(use_item);
                self.collect_use_children(
                    use_ref,
                    parent,
                    &item.item().visibility,
                    include_uses,
                    out,
                );
            } else {
                let item = match parent_ref {
                    Some(p) => item.with_parent(p),
                    None => item,
                };
                out.push(item);
            }
        }
    }

    fn collect_use_children(
        &mut self,
        use_ref: DocRef<'a, Use>,
        parent: DocRef<'a, Item>,
        use_visibility: &'a Visibility,
        include_uses: bool,
        out: &mut Vec<DocRef<'a, Item>>,
    ) {
        let Some(source_item) = self.follow_use(use_ref, parent) else {
            return;
        };
        let use_item = use_ref.item();
        if use_item.is_glob {
            // Glob re-export: the expanded items keep their own visibility. A
            // `pub use foo::*` of items that are themselves `pub` shows them
            // correctly; propagating the glob's visibility onto every
            // descendant (for `pub use private_mod::*`) is left as best-effort.
            match source_item.inner() {
                ItemEnum::Module(module) => {
                    self.collect_ids(source_item, &module.items, include_uses, None, out);
                }
                ItemEnum::Enum(enum_item) => {
                    self.collect_ids(source_item, &enum_item.variants, include_uses, None, out);
                }
                _ => {}
            }
        } else {
            // Non-glob Use: yield the source item with the imported name and the
            // `use`'s visibility (a `pub use` of a private item is publicly
            // reachable). We intentionally don't chain through nested `pub use`s
            // here — if the source is itself a Use, that's what the caller sees.
            // The user can follow the link to walk further.
            out.push(
                source_item
                    .with_name(&use_item.name)
                    .with_visibility(use_visibility),
            );
        }
    }
}

// ---------- id-path resolution ----------

impl<'a> Resolver<'a> {
    /// Resolve an `Id` from `origin`'s crate to a `DocRef`. The id is looked
    /// up in `origin`'s rustdoc paths map; for external-crate ids this
    /// crosses the crate boundary, loading the target crate if needed and
    /// walking its public path.
    pub fn get_path(&mut self, origin: DocRef<'a, Item>, id: Id) -> Option<DocRef<'a, Item>> {
        let item_summary = origin.crate_docs().path_summary(&id)?;
        let crate_ = origin
            .crate_docs()
            .traverse_to_crate_by_id(self.navigator, item_summary.crate_id)?;
        // The `paths` summary gives the item's definition path; resolve it in
        // the target crate's reverse index. This reaches definitions behind
        // private modules that a public-tree walk can't see.
        crate_.lookup_definition_path(
            self.navigator,
            item_summary.path.get(1..).unwrap_or_default(),
            item_summary.kind,
        )
    }

    /// Get an item from a sequence of `Id`s starting at the named crate's
    /// root. Returns the resolved item plus the path of segment names.
    pub fn get_item_from_id_path(
        &mut self,
        crate_name: &str,
        ids: &[u32],
    ) -> Option<(DocRef<'a, Item>, Vec<&'a str>)> {
        let mut path = vec![];
        let crate_docs = self.navigator.load_crate(crate_name, &VersionReq::STAR)?;
        let mut item = crate_docs.get(self.navigator, crate_docs.root_id())?;
        path.push(item.crate_docs().name());
        for id in ids {
            item = item.get(&Id(*id))?;
            if let ItemEnum::Use(use_item) = item.inner() {
                let use_ref = item.build_ref(use_item);
                let parent = item;
                item = self
                    .follow_use(use_ref, parent)
                    .or_else(|| self.resolve_path(&use_item.source, &mut vec![]))?;
                if !use_item.is_glob {
                    item.set_name(&use_item.name);
                }
            } else if let Some(name) = item.name() {
                path.push(name);
            }
        }
        Some((item, path))
    }
}

// ---------- helpers ----------

/// Parse a path segment that may carry a rustdoc kind discriminator prefix,
/// e.g. `"fn@foo"` or `"struct@Vec"`.
fn parse_discriminated_segment(segment: &str) -> (Option<ItemKind>, &str) {
    let Some(at) = segment.find('@') else {
        return (None, segment);
    };
    let (disc, name) = (&segment[..at], &segment[at + 1..]);
    match disc {
        "mod" | "module" => (Some(ItemKind::Module), name),
        "struct" => (Some(ItemKind::Struct), name),
        "enum" => (Some(ItemKind::Enum), name),
        "union" => (Some(ItemKind::Union), name),
        "trait" => (Some(ItemKind::Trait), name),
        "traitalias" => (Some(ItemKind::TraitAlias), name),
        "fn" | "function" | "method" => (Some(ItemKind::Function), name),
        "tyalias" | "typealias" => (Some(ItemKind::TypeAlias), name),
        "type" => (Some(ItemKind::AssocType), name),
        "const" | "constant" => (Some(ItemKind::Constant), name),
        "static" => (Some(ItemKind::Static), name),
        "macro" => (Some(ItemKind::Macro), name),
        "attr" => (Some(ItemKind::ProcAttribute), name),
        "derive" => (Some(ItemKind::ProcDerive), name),
        "prim" | "primitive" => (Some(ItemKind::Primitive), name),
        "field" => (Some(ItemKind::StructField), name),
        "variant" => (Some(ItemKind::Variant), name),
        "value" => (None, name),
        _ => (None, segment),
    }
}

/// Rewrite a relative path prefix (`crate::`, `self::`, `super::…`) against
/// `module_path`, returning the absolute equivalent. Non-prefixed paths are
/// returned unchanged. Returns `None` only when `super::` walks above the
/// crate root.
fn rewrite_relative_prefix(module_path: &[&str], source: &str) -> Option<String> {
    let mut supers = 0;
    let mut rest = source;
    while let Some(tail) = rest.strip_prefix("super::") {
        supers += 1;
        rest = tail;
    }
    if supers > 0 || rest == "super" {
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
