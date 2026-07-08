//! Navigator — a query's pinned view of the shared [`Store`].
//!
//! A `Navigator` is created per query (per CLI invocation, per HTTP request,
//! per TUI session) and holds `Arc` pins on every crate and search index the
//! query touches. The pin maps are `elsa::FrozenMap`s of `Arc`s: append-only,
//! so every `&RustdocData` or [`crate::DocRef`] borrow handed out stays valid
//! for the Navigator's lifetime even if the Store evicts the entry — the pin
//! keeps it alive. All path/name/Use resolution lives on [`crate::Resolver`],
//! which borrows a `Navigator`.

use crate::CrateName;
use crate::RustdocData;
use crate::search::SearchIndex;
use crate::store::{CrateInfo, Store};
use elsa::sync::FrozenMap;
use semver::VersionReq;
use std::borrow::Cow;
use std::fmt;
use std::fmt::Debug;
use std::sync::Arc;

/// A per-query view of a [`Store`]: resolution goes through the Store's
/// caches, and everything the query touches is pinned here so borrows are
/// stable for the query's lifetime.
pub struct Navigator {
    store: Arc<Store>,

    /// Crate data pinned by this query.
    ///
    /// This map is append-only and its values are `Arc`s (`StableDeref`), so
    /// all `&'a RustdocData` and `DocRef<'a>` borrows handed out are stable for
    /// the Navigator's lifetime. Keyed by the canonicalized requested name.
    pub(crate) working_set: FrozenMap<CrateName<'static>, Arc<RustdocData>>,

    /// Search indexes pinned by this query; same model as `working_set`.
    pub(crate) search_indexes: FrozenMap<CrateName<'static>, Arc<SearchIndex>>,
}

impl Debug for Navigator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Navigator")
            .field("store", &self.store)
            .finish()
    }
}

impl Default for Navigator {
    /// A Navigator over its own empty `Store` — no sources. Mostly useful in
    /// tests; real construction sites share one Store across Navigators.
    fn default() -> Self {
        Self::new(Arc::new(Store::default()))
    }
}

impl Navigator {
    pub fn new(store: Arc<Store>) -> Self {
        Self {
            store,
            working_set: FrozenMap::new(),
            search_indexes: FrozenMap::new(),
        }
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    /// The local source, if configured. See [`Store::local_source`].
    pub fn local_source(&self) -> Option<&crate::sources::LocalSource> {
        self.store.local_source()
    }

    /// List all available crate names from all sources
    pub fn list_available_crates(&self) -> impl Iterator<Item = &CrateInfo> {
        self.store.list_available_crates()
    }

    /// Look up a crate by name, returning canonical name and metadata.
    ///
    /// A lossy view of [`Store::lookup_crate`]: transient source failures are
    /// logged and reported as `None`, which is fine for the display call sites
    /// this serves — cacheability decisions go through [`Self::load_crate`].
    pub fn lookup_crate<'a>(
        &'a self,
        name: &str,
        version: &VersionReq,
    ) -> Option<Cow<'a, CrateInfo>> {
        self.store
            .lookup_crate(name, version)
            .map_err(|e| log::warn!("transient failure resolving {name}: {e:?}"))
            .ok()
            .flatten()
    }

    /// Get the project root path if a local context exists
    pub fn project_root(&self) -> Option<&std::path::Path> {
        self.store.project_root()
    }

    pub fn canonicalize(&self, name: &str) -> CrateName<'static> {
        self.store.canonicalize(name)
    }

    /// Load a crate by name and optional version
    ///
    /// If version is None:
    /// - First checks external crate names from loaded crates
    /// - For local context crates: use the locked version from Cargo.lock
    /// - For arbitrary crates: use "latest"
    ///
    /// Returns None if the crate cannot be found in any source. The loaded
    /// data is pinned in this Navigator, so the borrow survives Store
    /// eviction.
    pub fn load_crate(&self, name: &str, version_req: &VersionReq) -> Option<&RustdocData> {
        let crate_name = self.canonicalize(name);
        if let Some(data) = self.working_set.get(&crate_name) {
            return Some(data);
        }

        let data = self.store.load_crate(&crate_name, name, version_req).ok()?;
        Some(self.working_set.insert(crate_name, data))
    }
}

// Compile-time assertions that Navigator is thread-safe
// This is required for multi-threaded interactive TUI
#[allow(dead_code)]
const _: () = {
    const fn assert_send<T: Send>() {}
    const fn assert_sync<T: Sync>() {}

    const fn check_navigator_thread_safety() {
        assert_send::<Navigator>();
        assert_sync::<Navigator>();
    }
};
