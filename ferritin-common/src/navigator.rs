//! Navigator — a query's pinned view of the shared [`Store`].
//!
//! A `Navigator` is created per query (per CLI invocation, per HTTP request,
//! per TUI session) and holds `Arc` pins on every crate and search index the
//! query touches. The pin maps are `elsa::FrozenMap`s of `Arc`s: append-only,
//! so every `&RustdocData` or [`crate::DocRef`] borrow handed out stays valid
//! for the Navigator's lifetime even if the Store evicts the entry — the pin
//! keeps it alive. All path/name/Use resolution lives on [`crate::Resolver`],
//! which borrows a `Navigator`.

use crate::{
    CrateName, RustdocData,
    search::SearchIndex,
    store::{CrateInfo, Store, exact_req, exact_version},
    string_utils::case_aware_jaro_winkler,
};
use elsa::sync::FrozenMap;
use semver::{Version, VersionReq};
use std::{
    borrow::Cow,
    fmt,
    fmt::Debug,
    sync::{Arc, OnceLock},
};

/// How many crates.io candidates [`Navigator::classify_missing_crate`] pulls
/// from the namespace index — headroom over the handful ultimately surfaced,
/// since the caller re-ranks and truncates.
const CRATE_INDEX_FETCH: usize = 10;

/// The outcome of a crate-level lookup miss, resolved against the crates.io
/// namespace index by [`Navigator::classify_missing_crate`].
#[derive(Debug, Default)]
pub struct MissingCrate {
    /// The crates.io-canonical name, set when the crate exists but its docs
    /// couldn't be loaded (docs.rs has no rustdoc JSON) — a real crate we can't
    /// serve, distinct from a typo. When set, `suggestions` is empty.
    pub exists_as: Option<String>,
    /// Crate-name "did you mean" candidates as `(name, similarity)`, drawn from
    /// the whole namespace and left for the caller to merge, rank, and truncate.
    pub suggestions: Vec<(String, f64)>,
}

/// A per-query view of a [`Store`]: resolution goes through the Store's
/// caches, and everything the query touches is pinned here so borrows are
/// stable for the query's lifetime.
pub struct Navigator {
    store: Arc<Store>,

    /// Crate data pinned by this query.
    ///
    /// This map is append-only and its values are `Arc`s (`StableDeref`), so
    /// all `&'a RustdocData` and `DocRef<'a>` borrows handed out are stable for
    /// the Navigator's lifetime. Keyed by the canonicalized requested name —
    /// one version per name per query (first pin wins).
    pub(crate) working_set: FrozenMap<CrateName<'static>, Arc<RustdocData>>,

    /// The exact version each `working_set` pin was loaded at — the Store's
    /// data-cache key, which the data's self-reported version can't stand in
    /// for (it can be absent). Populated by every successful `load_crate`.
    pub(crate) pinned_versions: FrozenMap<CrateName<'static>, Box<Version>>,

    /// Search indexes pinned by this query; same model as `working_set`.
    pub(crate) search_indexes: FrozenMap<CrateName<'static>, Arc<SearchIndex>>,

    /// The first crate this query loaded: the version authority for
    /// cross-crate traversal (see [`Navigator::built_against`]).
    entrypoint: OnceLock<CrateName<'static>>,
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
            pinned_versions: FrozenMap::new(),
            search_indexes: FrozenMap::new(),
            entrypoint: OnceLock::new(),
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

    /// The crates.io namespace index, if a docs.rs source is configured — the
    /// same [`CrateIndex`](crate::crate_names::CrateIndex) version resolution
    /// and the server's typeahead read. `None` for a store with no docs.rs
    /// source (e.g. a purely local or std-only one).
    pub fn crate_index(&self) -> Option<&crate::crate_names::CrateIndex> {
        self.store
            .docsrs_source()
            .map(|source| source.client().crate_index().as_ref())
    }

    /// Classify a crate-level lookup miss (nothing loaded for `requested`)
    /// against the crates.io namespace index: whether the crate exists but its
    /// docs couldn't be served, and the crate-name "did you mean" candidates.
    ///
    /// Async because it reads the index, which may revalidate the artifact. It
    /// deliberately does **not** block: a synchronous caller drives it with
    /// [`block_on`](crate::block_on) at its own boundary, so no hidden block
    /// lives on the resolution path. `MissingCrate::default` (no crate, no
    /// suggestions) when there is no index to consult.
    pub async fn classify_missing_crate(&self, requested: &str) -> MissingCrate {
        let Some(index) = self.crate_index() else {
            return MissingCrate::default();
        };

        // A crate the index knows is real — docs.rs simply has no rustdoc JSON
        // for it. That is not a typo, so it carries no suggestions.
        if let Some(entry) = index.get(requested).await {
            return MissingCrate {
                exists_as: Some(entry.name),
                suggestions: Vec::new(),
            };
        }

        // Otherwise, near-miss crate names from the whole namespace (prefix and
        // token matches, plus fuzzy fill), scored the same way the resolver
        // scores its locally-listable suggestions so the two rank together.
        let suggestions = match index.typeahead(requested, CRATE_INDEX_FETCH).await {
            Some((entries, _)) => entries
                .into_iter()
                .map(|entry| {
                    let score = case_aware_jaro_winkler(&entry.name, requested);
                    (entry.name, score)
                })
                .collect(),
            None => Vec::new(),
        };
        MissingCrate {
            exists_as: None,
            suggestions,
        }
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
            if let Some(requested) = exact_version(version_req)
                && self.pinned_versions.get(&crate_name) != Some(&requested)
            {
                log::debug!(
                    "{crate_name}: requested ={requested} but the query already pinned {:?}; \
                     serving the pin (one version per name per query)",
                    self.pinned_versions.get(&crate_name),
                );
            }
            return Some(data);
        }

        let (version, data) = self.store.load_crate(&crate_name, name, version_req).ok()?;
        let _ = self.entrypoint.set(crate_name.clone());
        // Version before data: both maps are first-insert-wins, so inserting
        // in this order guarantees any visible pin has a recorded version.
        self.pinned_versions
            .insert(crate_name.clone(), Box::new(version));
        Some(self.working_set.insert(crate_name, data))
    }

    /// The exact version `name`'s pin was loaded at, if this query has loaded
    /// it.
    pub fn pinned_version(&self, name: &CrateName<'static>) -> Option<&Version> {
        self.pinned_versions.get(name)
    }

    /// Pin a synthetic crate directly, bypassing the Store. Records a version
    /// so the "any visible pin has a recorded version" invariant holds for
    /// test navigators too.
    #[cfg(test)]
    pub(crate) fn pin_for_test(&self, name: &str, data: RustdocData) {
        let crate_name = CrateName::from(name.to_string());
        let version = data.version().cloned().unwrap_or(Version::new(0, 0, 0));
        self.pinned_versions
            .insert(crate_name.clone(), Box::new(version));
        self.working_set.insert(crate_name, Arc::new(data));
    }

    /// Entrypoint version authority: the real name and exact version the
    /// query's entrypoint crate was built against for `name`, if the
    /// entrypoint's build graph pins one. Cross-crate traversal consults this
    /// before the referencing crate's own `html_root_url`, keeping every
    /// crate the entrypoint knows at the entrypoint's version regardless of
    /// traversal order.
    pub(crate) fn built_against(&self, name: &str) -> Option<(&str, VersionReq)> {
        let entrypoint = self.working_set.get(self.entrypoint.get()?)?;
        let info = entrypoint.built_against(&CrateName::from(name.to_string()))?;
        Some((info.name(), exact_req(info.version())))
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
