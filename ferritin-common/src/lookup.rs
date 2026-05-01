//! Name-based child lookup, lazy with respect to `Use` resolution.
//!
//! See [`crate::iterators::LazyChild`] for the underlying iteration model.
//! [`find_named`] is the entry point: it walks a module/enum's direct children
//! first (matching by name without resolving Uses), then expands glob Uses one
//! step at a time, guarded against cycles.

use crate::DocRef;
use crate::iterators::{LazyChild, LazyChildren};
use rustdoc_types::{Item, ItemEnum};
use std::cell::RefCell;
use std::collections::HashSet;

/// Callback contract for [`for_each_named`]. Return `Continue` to keep walking
/// (e.g. when a candidate didn't have the path-suffix the caller wanted),
/// `Stop(value)` to halt and propagate the value up.
pub(crate) enum Step<T> {
    Continue,
    Stop(T),
}

/// Find a child of `parent` whose imported name is `target`.
///
/// Phases:
/// 1. Walk direct children. Match against names of non-`Use` items and
///    non-glob `Use` items (using `use_item.name`, no source resolution).
///    First match wins.
/// 2. If no phase-1 match, walk glob `Use` items: resolve each glob's source
///    one step and recurse with the same target name.
///
/// The phase ordering means self-referential non-glob Uses
/// (`pub use self::error;` in `std::io` while the real `error` submodule is
/// also a child) never trigger source resolution at all — the real submodule
/// is found in phase 1.
///
/// Glob expansion is the only path that can re-enter; the [`GlobGuard`]
/// thread-local breaks cycles like `pub use a::*;` in `m`, `pub use m::*;` in
/// `a`, by keying on `(module item address, target name)`.
/// Find the first child of `parent` named `target`, or `None`.
///
/// Equivalent to `for_each_named(.., |c| Stop(c))` — see [`for_each_named`]
/// for the multi-candidate variant used by path traversal.
pub(crate) fn find_named<'a>(parent: DocRef<'a, Item>, target: &str) -> Option<DocRef<'a, Item>> {
    for_each_named(parent, target, Step::Stop)
}

/// Walk every direct or transitively-glob-imported child named `target`,
/// invoking `visit` on each. Stops at the first `Stop(value)` callback result.
///
/// This exists because path traversal needs to try *all* candidates with a
/// matching name: e.g. when a name resolves to both a re-export Use and the
/// real submodule, only one of them may actually contain the next path
/// segment, so the caller must be allowed to `Continue` past a candidate
/// whose recursion failed.
///
/// Two phases:
/// 1. Direct children: yield non-`Use` items and non-glob `Use`-resolved
///    items whose imported name matches. Self-referential non-glob Uses
///    (`pub use self::error;` alongside the real `error` submodule) don't
///    cycle: the real submodule is yielded as a phase-1 candidate.
/// 2. Glob Uses: resolve and recurse with the same target, guarded against
///    re-entry on `(parent, target)`.
pub(crate) fn for_each_named<'a, T>(
    parent: DocRef<'a, Item>,
    target: &str,
    mut visit: impl FnMut(DocRef<'a, Item>) -> Step<T>,
) -> Option<T> {
    visit_named(parent, target, &mut visit)
}

fn visit_named<'a, T>(
    parent: DocRef<'a, Item>,
    target: &str,
    visit: &mut dyn FnMut(DocRef<'a, Item>) -> Step<T>,
) -> Option<T> {
    // Struct/enum/union/trait associated items live in impl blocks. They
    // can't be re-exports, so name resolution here is direct.
    if matches!(
        parent.inner(),
        ItemEnum::Struct(_) | ItemEnum::Enum(_) | ItemEnum::Union(_) | ItemEnum::Trait(_)
    ) {
        for method in parent.methods() {
            if method.name() == Some(target)
                && let Step::Stop(v) = visit(method)
            {
                return Some(v);
            }
        }
    }

    // Phase 1: direct module/enum children.
    let mut globs: Vec<LazyChild<'a>> = Vec::new();
    for child in LazyChildren::new(parent) {
        match child {
            LazyChild::Item(item) => {
                if item.name() == Some(target)
                    && let Step::Stop(v) = visit(item)
                {
                    return Some(v);
                }
            }
            LazyChild::NonGlob { use_item, .. } => {
                if use_item.item().name == target
                    && let Some(resolved) = child.resolve()
                    && let Step::Stop(v) = visit(resolved)
                {
                    return Some(v);
                }
            }
            LazyChild::Glob { .. } => {
                globs.push(child);
            }
        }
    }

    // Phase 2: glob expansion, guarded against (parent, target) re-entry.
    let _guard = GlobGuard::enter(parent.item(), target)?;
    for glob in globs {
        let Some(source_module) = glob.resolve() else {
            continue;
        };
        if let Some(v) = visit_named(source_module, target, visit) {
            return Some(v);
        }
    }

    None
}

/// Re-entry guard for glob expansion in [`find_named`]. Keys on the parent
/// item's address (stable for the duration of the call — items are owned by
/// `Navigator::working_set`) plus the target name bytes.
pub(crate) struct GlobGuard {
    key: (usize, Vec<u8>),
}

thread_local! {
    static IN_FLIGHT: RefCell<HashSet<(usize, Vec<u8>)>> = RefCell::new(HashSet::new());
}

impl GlobGuard {
    pub(crate) fn enter(item: &Item, target: &str) -> Option<Self> {
        let key = (item as *const Item as usize, target.as_bytes().to_vec());
        let inserted = IN_FLIGHT.with(|s| s.borrow_mut().insert(key.clone()));
        inserted.then_some(Self { key })
    }
}

impl Drop for GlobGuard {
    fn drop(&mut self) {
        IN_FLIGHT.with(|s| {
            s.borrow_mut().remove(&self.key);
        });
    }
}
