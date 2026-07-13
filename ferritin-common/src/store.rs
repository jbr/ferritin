//! Store — long-lived, shared, evictable cache of loaded crate data.
//!
//! The [`Store`] owns the source backends and two bounded caches (crate data
//! and search indexes) keyed by [`CrateName`]. Queries never borrow from the
//! Store directly: a per-query [`crate::Navigator`] clones `Arc`s out of it
//! into its own pin map, so eviction here can never invalidate a borrow — a
//! query's pins keep evicted data alive until the query ends. Memory bound =
//! cache cap + in-flight pins.
//!
//! Each cache entry holds an `Arc<OnceLock<..>>` slot, which provides
//! **singleflight** for free: concurrent loaders of the same crate block on
//! `OnceLock::get_or_init` and share the winner's result. Failures are cached
//! **with a TTL** ([`LoadFailure`]): definitive absence (no rustdoc JSON for a
//! release) is remembered for a long time, transient failures (a docs.rs or
//! crates.io blip) only briefly — so a blip no longer poisons a crate for the
//! process lifetime.
//!
//! Eviction runs on insert when the summed entry weights exceed a byte cap
//! (default: unbounded; the server sets caps). The policy is deliberately dumb
//! — least-recently-accessed first, among entries no query currently pins
//! (evicting a pinned entry frees no memory; see [`Entry::is_pinned`]) — but
//! each entry records the metadata (weight, access count, timestamps) a
//! smarter policy will need. Because eviction only runs on insert, a
//! pin-heavy burst can leave the cache over its cap until the next load
//! triggers a sweep; nothing grows in the meantime.

use crate::CrateName;
use crate::RustdocData;
use crate::search::SearchIndex;
use crate::sources::{CrateProvenance, DocsRsSource, LocalSource, Source, StdSource};
use fieldwork::Fieldwork;
use semver::{Version, VersionReq};
use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::{self, Debug};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// How long a definitive "not available" result is remembered. docs.rs gaining
/// JSON for an existing release (or a brand-new crate name appearing on
/// crates.io) is rare enough that an hour of staleness is acceptable.
const NOT_AVAILABLE_TTL: Duration = Duration::from_secs(60 * 60);

/// How long a transient failure (network error) is remembered — long enough to
/// shield the upstream from a retry storm, short enough to recover promptly.
const TRANSIENT_TTL: Duration = Duration::from_secs(30);

/// How long a successful resolution of a floating version req (`*`, `^1`)
/// stays fresh: what "latest" means moves when a new version publishes.
/// Exact reqs never enter the resolution cache — they skip resolution
/// entirely (see [`exact_version`]).
const RESOLUTION_TTL: Duration = Duration::from_secs(15 * 60);

/// Why a load failed, determining how long the failure is cached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadFailure {
    /// The sources definitively do not have this crate (cached with a long TTL).
    NotAvailable,
    /// A source failed transiently, e.g. a network error (cached briefly).
    Transient,
}

impl LoadFailure {
    fn ttl(self) -> Duration {
        match self {
            Self::NotAvailable => NOT_AVAILABLE_TTL,
            Self::Transient => TRANSIENT_TTL,
        }
    }
}

/// The shared once-cell a cache entry resolves through. Cloned out of the map
/// so initialization runs without holding the map lock.
type Slot<T> = Arc<OnceLock<Result<Arc<T>, LoadFailure>>>;

/// One cache entry: the result slot plus the metadata an eviction policy needs.
struct Entry<T> {
    slot: Slot<T>,
    /// Byte-weight proxy, recorded when the load completes (0 until then, and
    /// for failures).
    weight: u64,
    access_count: u64,
    last_access: Instant,
    /// When the slot was created, refreshed when its load completes; TTLs are
    /// measured from here.
    loaded_at: Instant,
}

impl<T> Entry<T> {
    fn new() -> Self {
        let now = Instant::now();
        Self {
            slot: Arc::new(OnceLock::new()),
            weight: 0,
            access_count: 0,
            last_access: now,
            loaded_at: now,
        }
    }

    /// Whether this entry's cached result has outlived its TTL and should be
    /// retried on next access (or dropped by a sweep). Failures expire by
    /// their [`LoadFailure`] TTL; successes only if the cache has a
    /// `positive_ttl`. In-flight entries never expire.
    fn is_expired(&self, positive_ttl: Option<Duration>) -> bool {
        match self.slot.get() {
            Some(Err(failure)) => self.loaded_at.elapsed() >= failure.ttl(),
            Some(Ok(_)) => positive_ttl.is_some_and(|ttl| self.loaded_at.elapsed() >= ttl),
            None => false,
        }
    }

    /// Whether some Navigator currently pins this entry's data (i.e. clones of
    /// the inner `Arc` exist beyond the cache's own). Evicting a pinned entry
    /// frees no memory — the pin keeps the data alive — and just forces a
    /// reload for the next query, so eviction skips these. The count is a
    /// momentary read (a pin could appear right after), but eviction is
    /// heuristic: the failure mode is skipping or keeping one entry per pass.
    fn is_pinned(&self) -> bool {
        matches!(self.slot.get(), Some(Ok(data)) if Arc::strong_count(data) > 1)
    }
}

/// The map half of a [`Cache`], bundled with its sweep trigger so both live
/// under one lock.
struct Entries<K, T> {
    map: HashMap<K, Entry<T>>,
    /// Map size that triggers the next expired-entry sweep. Expired entries
    /// are otherwise only *replaced* on re-access, never removed — a stream of
    /// never-repeated keys (a crawler walking garbage crate names) would grow
    /// the map forever. Doubling after each sweep amortizes the cost to O(1)
    /// per insert.
    sweep_at: usize,
}

const INITIAL_SWEEP_THRESHOLD: usize = 128;

impl<K, T> Default for Entries<K, T> {
    fn default() -> Self {
        Self {
            map: HashMap::default(),
            sweep_at: INITIAL_SWEEP_THRESHOLD,
        }
    }
}

impl<K: Eq + std::hash::Hash, T> Entries<K, T> {
    /// Drop every expired entry and reset the sweep trigger. Returns the
    /// dropped entries so the caller can release them outside the map lock.
    fn sweep(&mut self, positive_ttl: Option<Duration>) -> Vec<Entry<T>> {
        let swept = self
            .map
            .extract_if(|_, entry| entry.is_expired(positive_ttl))
            .map(|(_, entry)| entry)
            .collect();
        self.sweep_at = (self.map.len() * 2).max(INITIAL_SWEEP_THRESHOLD);
        swept
    }
}

/// A bounded, singleflight, TTL-aware cache.
///
/// `K` is the lookup key (crate name today, `(name, version)` and
/// `(name, req)` with versioned keys); values are shared as `Arc<T>`.
struct Cache<K, T> {
    entries: Mutex<Entries<K, T>>,
    /// Byte cap over the sum of entry weights; eviction runs on insert.
    cap: u64,
    /// TTL for successful entries. `None` (crate data) means a success is
    /// cached until evicted — the data for a released version is immutable.
    /// `Some` (resolution of floating version reqs) means success goes stale:
    /// what `*` or `^1` resolves to moves when a new version publishes.
    positive_ttl: Option<Duration>,
}

impl<K, T> Default for Cache<K, T> {
    fn default() -> Self {
        Self {
            entries: Mutex::default(),
            cap: u64::MAX,
            positive_ttl: None,
        }
    }
}

impl<K: Eq + std::hash::Hash + Clone + Debug, T> Cache<K, T> {
    /// The slot for `key`: touches access metadata, replaces an expired
    /// entry, creates the entry if absent, and runs the expired-entry sweep
    /// when the map has grown past its trigger.
    fn slot(&self, key: &K) -> Slot<T> {
        let (slot, swept) = {
            let mut entries = self.entries.lock().unwrap();
            let swept =
                (entries.map.len() >= entries.sweep_at).then(|| entries.sweep(self.positive_ttl));
            let entry = entries.map.entry(key.clone()).or_insert_with(Entry::new);
            if entry.is_expired(self.positive_ttl) {
                *entry = Entry::new();
            }
            entry.access_count += 1;
            entry.last_access = Instant::now();
            (entry.slot.clone(), swept)
        };
        // Swept entries are dropped here, outside the map lock — dropping the
        // last Arc to a RustdocData joins its sidecar write thread, which must
        // not stall unrelated lookups.
        drop(swept);
        slot
    }

    /// Look up `key`, running `init` to load it on a miss. Concurrent callers
    /// for the same key block on the winner's `init` and share its result
    /// (singleflight). After a completed load, `weigh` records the entry's
    /// byte weight and eviction runs if the cache is over its cap.
    fn get_or_load(
        &self,
        key: &K,
        weigh: impl FnOnce(&T) -> u64,
        init: impl FnOnce() -> Result<Arc<T>, LoadFailure>,
    ) -> Result<Arc<T>, LoadFailure> {
        let slot = self.slot(key);
        let mut initialized = false;
        let result = slot
            .get_or_init(|| {
                initialized = true;
                init()
            })
            .clone();
        if initialized {
            let evicted = self.record_load(key, &slot, result.as_deref().ok().map(weigh));
            // Dropped outside the map lock; see `slot` for why.
            drop(evicted);
        }
        result
    }

    /// Record a completed load on `key`'s entry and evict while over the byte
    /// cap. Returns the evicted entries so the caller can drop them after the
    /// map lock is released.
    fn record_load(&self, key: &K, slot: &Slot<T>, weight: Option<u64>) -> Vec<Entry<T>> {
        let mut entries = self.entries.lock().unwrap();
        if let Some(entry) = entries.map.get_mut(key)
            && Arc::ptr_eq(&entry.slot, slot)
        {
            entry.loaded_at = Instant::now();
            entry.weight = weight.unwrap_or(0);
        }

        let mut evicted = Vec::new();
        loop {
            let total: u64 = entries.map.values().map(|entry| entry.weight).sum();
            if total <= self.cap {
                break;
            }
            // Victim: the least-recently-accessed completed, unpinned entry.
            // In-flight loads are skipped (their result would never land
            // anywhere), pinned entries are skipped (evicting them frees no
            // memory, see `is_pinned`), and so is the entry that triggered
            // this pass (evicting it immediately would just thrash).
            let Some(victim) = entries
                .map
                .iter()
                .filter(|(k, entry)| entry.slot.get().is_some() && !entry.is_pinned() && *k != key)
                .min_by_key(|(_, entry)| entry.last_access)
                .map(|(k, _)| k.clone())
            else {
                log::info!(
                    "cache over cap ({total} > {}) but everything is pinned or in flight; \
                     eviction resumes as queries release their pins",
                    self.cap
                );
                break;
            };
            if let Some(entry) = entries.map.remove(&victim) {
                log::info!(
                    "evicting {victim:?} (weight {}, {} accesses, resident {:?}; total {total} > cap {})",
                    entry.weight,
                    entry.access_count,
                    entry.loaded_at.elapsed(),
                    self.cap
                );
                evicted.push(entry);
            }
        }
        evicted
    }
}

/// A docs.rs URL parsed into crate name and version
///
/// Examples:
/// - "https://docs.rs/tokio-macros/2.6.0/x86_64-unknown-linux-gnu/" -> ("tokio-macros", "2.6.0")
/// - "https://docs.rs/serde/1.0.228" -> ("serde", "1.0.228")
pub(crate) fn parse_docsrs_url(url: &str) -> Option<(&str, &str)> {
    let url = url
        .strip_prefix("https://docs.rs/")
        .or_else(|| url.strip_prefix("http://docs.rs/"))?;

    let parts: Vec<&str> = url.split('/').collect();
    if parts.len() >= 2 {
        Some((parts[0], parts[1]))
    } else {
        None
    }
}

/// The output of phase-1 resolution: the exact version to load, plus what is
/// needed to load and attribute it.
#[derive(Debug)]
struct Resolved {
    /// The real crate name (with dashes, as it appears on crates.io).
    name: String,
    version: Version,
    provenance: CrateProvenance,
}

/// The single version a `VersionReq` pins, if it pins exactly one
/// (`=1.2.3`). Such reqs skip resolution: the version *is* the cache key.
/// Note that a bare `1.2.3` parses as caret (a range), so only explicit `=`
/// reqs with all three components qualify.
pub(crate) fn exact_version(req: &VersionReq) -> Option<Version> {
    match &*req.comparators {
        [c] if c.op == semver::Op::Exact => Some(Version {
            major: c.major,
            minor: c.minor?,
            patch: c.patch?,
            pre: c.pre.clone(),
            build: semver::BuildMetadata::EMPTY,
        }),
        _ => None,
    }
}

/// The `=version` req pinning exactly `version` — the inverse of
/// [`exact_version`], so reqs built here take the resolution-skipping fast
/// path in [`Store::load_crate`].
pub(crate) fn exact_req(version: &Version) -> VersionReq {
    VersionReq {
        comparators: vec![semver::Comparator {
            op: semver::Op::Exact,
            major: version.major,
            minor: Some(version.minor),
            patch: Some(version.patch),
            pre: version.pre.clone(),
        }],
    }
}

#[derive(Debug, Clone, Fieldwork)]
#[fieldwork(get, rename_predicates)]
pub struct CrateInfo {
    #[field(copy)]
    pub(crate) provenance: CrateProvenance,
    pub(crate) version: Version,
    pub(crate) description: Option<String>,
    pub(crate) name: String,
    pub(crate) default_crate: bool,
    pub(crate) used_by: Vec<String>,
    pub(crate) json_path: Option<PathBuf>,
}

/// Long-lived, shared documentation store: sources plus bounded caches.
///
/// Sources are checked in this order:
/// 1. std (if crate name matches RUST_CRATES)
/// 2. local (if LocalSource is present and has the crate)
/// 3. docs.rs (if DocsRsSource is present)
///
/// One `Store` (behind an `Arc` where it is shared) serves many short-lived
/// [`crate::Navigator`]s; see the module docs for the eviction/pinning model.
#[derive(Fieldwork)]
#[fieldwork(get, opt_in, with)]
pub struct Store {
    #[field]
    std_source: Option<StdSource>,
    #[field]
    docsrs_source: Option<DocsRsSource>,
    #[field]
    local_source: Option<LocalSource>,

    /// Resolution cache: which exact version a (name, req) request means, or
    /// why it couldn't be resolved ("crate doesn't exist" negative-caches
    /// here). Keyed by the canonicalized *requested* name; unbounded by
    /// weight (entries are tiny — the sweep keeps garbage names from
    /// accumulating), with a positive TTL so floating reqs track releases.
    resolutions: Cache<(CrateName<'static>, VersionReq), Resolved>,

    /// Evictable crate-data cache, keyed by resolved name and exact version
    /// ("no rustdoc JSON for this release" negative-caches here). Values are
    /// handed out as `Arc`s that Navigators pin per query.
    crates: Cache<(CrateName<'static>, Version), RustdocData>,

    /// Evictable search-index cache, same keys and mechanism as `crates`.
    search_indexes: Cache<(CrateName<'static>, Version), SearchIndex>,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            std_source: None,
            docsrs_source: None,
            local_source: None,
            resolutions: Cache {
                positive_ttl: Some(RESOLUTION_TTL),
                ..Cache::default()
            },
            crates: Cache::default(),
            search_indexes: Cache::default(),
        }
    }
}

impl Debug for Store {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Store")
            .field("std_source", &self.std_source)
            .field("docsrs_source", &self.docsrs_source)
            .field("local_source", &self.local_source)
            .finish()
    }
}

impl Store {
    /// Cap the crate-data cache at `cap` bytes of summed entry weights
    /// (weight proxy: the crate's JSON file size). Unbounded by default.
    pub fn with_weight_cap(mut self, cap: u64) -> Self {
        self.crates.cap = cap;
        self
    }

    /// Cap the search-index cache at `cap` bytes of summed entry weights
    /// (weight proxy: the on-disk index file size). Unbounded by default.
    pub fn with_search_weight_cap(mut self, cap: u64) -> Self {
        self.search_indexes.cap = cap;
        self
    }

    /// The configured sources in lookup priority order.
    fn sources(&self) -> impl Iterator<Item = &dyn Source> {
        [
            self.std_source.as_ref().map(|s| s as &dyn Source),
            self.local_source.as_ref().map(|s| s as &dyn Source),
            self.docsrs_source.as_ref().map(|s| s as &dyn Source),
        ]
        .into_iter()
        .flatten()
    }

    /// List all available crate names from all sources
    /// Returns crate names from std library and local workspace/dependencies
    pub fn list_available_crates(&self) -> impl Iterator<Item = &CrateInfo> {
        self.sources().flat_map(|source| source.list_available())
    }

    /// Look up a crate by name, returning canonical name and metadata.
    /// Tries sources in priority order; the first definitive hit wins. A
    /// transient source failure defers to later sources and only surfaces as
    /// `Err` when no source had the crate.
    pub fn lookup_crate<'a>(
        &'a self,
        name: &str,
        version: &VersionReq,
    ) -> anyhow::Result<Option<Cow<'a, CrateInfo>>> {
        log::info!("Resolving {name:?}, version {version}");
        let mut deferred_error = None;
        for source in self.sources() {
            match source.lookup(name, version) {
                Ok(Some(info)) => return Ok(Some(info)),
                Ok(None) => {}
                Err(e) => deferred_error = Some(e),
            }
        }
        match deferred_error {
            Some(e) => Err(e),
            None => Ok(None),
        }
    }

    /// Get the project root path if a local context exists
    pub fn project_root(&self) -> Option<&std::path::Path> {
        self.local_source.as_ref().map(|p| p.project_root())
    }

    pub fn canonicalize(&self, name: &str) -> CrateName<'static> {
        self.sources()
            .find_map(|source| source.canonicalize(name))
            .unwrap_or_else(|| CrateName::from(String::from(name)))
    }

    /// Load `requested_name` at `version_req` through the two cache layers:
    /// resolution (which exact version the req means) and data (that
    /// version's rustdoc JSON), each singleflight with negative TTLs and
    /// eviction on insert. `crate_name` must be the canonicalized form of
    /// `requested_name` (the resolution-cache key).
    ///
    /// Returns the exact version alongside the data — the data-cache key the
    /// caller's pin is for, which the data's self-reported version cannot
    /// stand in for (it can be absent).
    ///
    /// An exact req (`=1.2.3`) skips resolution entirely: the version is the
    /// data key. Anything else resolves first, so the data cache is keyed by
    /// *resolved* name — aliases that resolution collapses (the local `crate`
    /// shorthand) share one data entry with their target.
    pub(crate) fn load_crate(
        &self,
        crate_name: &CrateName<'static>,
        requested_name: &str,
        version_req: &VersionReq,
    ) -> Result<(Version, Arc<RustdocData>), LoadFailure> {
        let (name, version, provenance_hint) = if let Some(version) = exact_version(version_req) {
            (crate_name.clone(), version, None)
        } else {
            let resolved = self.resolve(crate_name, requested_name, version_req)?;
            (
                CrateName::from(resolved.name.clone()),
                resolved.version.clone(),
                Some(resolved.provenance),
            )
        };

        let data = self.crates.get_or_load(
            &(name.clone(), version.clone()),
            |data| data.fs_path().metadata().map_or(0, |m| m.len()),
            || self.load_data(&name, &version, provenance_hint),
        )?;
        Ok((version, data))
    }

    /// Look up a search index by resolved crate name and exact version,
    /// running `build` on a miss with the same singleflight/negative-TTL/
    /// eviction treatment as crate data.
    pub(crate) fn search_index(
        &self,
        key: &(CrateName<'static>, Version),
        build: impl FnOnce() -> Result<Arc<SearchIndex>, LoadFailure>,
    ) -> Result<Arc<SearchIndex>, LoadFailure> {
        self.search_indexes
            .get_or_load(key, |index| index.disk_weight(), build)
    }

    /// Phase 1 for a resolution-cache miss: the source-cascade metadata
    /// lookup, reduced to what the data layer needs.
    fn resolve(
        &self,
        crate_name: &CrateName<'static>,
        requested_name: &str,
        version_req: &VersionReq,
    ) -> Result<Arc<Resolved>, LoadFailure> {
        self.resolutions.get_or_load(
            &(crate_name.clone(), version_req.clone()),
            |_| 0,
            || match self.lookup_crate(requested_name, version_req) {
                Ok(Some(info)) => {
                    let resolved = Resolved {
                        name: info.name.clone(),
                        version: info.version.clone(),
                        provenance: info.provenance,
                    };
                    log::info!("Resolved {requested_name}@{version_req} -> {resolved:?}");
                    Ok(Arc::new(resolved))
                }
                Ok(None) => Err(LoadFailure::NotAvailable),
                Err(e) => {
                    log::warn!("transient failure resolving {requested_name}: {e:?}");
                    Err(LoadFailure::Transient)
                }
            },
        )
    }

    /// Phase 2 for a data-cache miss: load `name` at exactly `version` from
    /// the sources and prepare it for publication.
    fn load_data(
        &self,
        name: &CrateName<'static>,
        version: &Version,
        provenance_hint: Option<CrateProvenance>,
    ) -> Result<Arc<RustdocData>, LoadFailure> {
        log::info!("Loading {name}@{version}");

        let start = Instant::now();
        let result = self.load_from_sources(name.as_ref(), version, provenance_hint);
        log::debug!("⏱️ Total load time for {name}: {:?}", start.elapsed());

        match result {
            Ok(Some(mut data)) => {
                // Build reverse path index before publishing
                data.build_path_index();

                Ok(Arc::new(data))
            }
            Ok(None) => Err(LoadFailure::NotAvailable),
            Err(e) => {
                log::warn!("transient failure loading {name}: {e:?}");
                Err(LoadFailure::Transient)
            }
        }
    }

    /// Try loading from the appropriate source based on lookup result
    fn load_from_sources(
        &self,
        crate_name: &str,
        version: &Version,
        provenance_hint: Option<CrateProvenance>,
    ) -> anyhow::Result<Option<RustdocData>> {
        match provenance_hint {
            Some(CrateProvenance::Std) => {
                log::debug!("loading from std");
                match &self.std_source {
                    Some(source) => source.load(crate_name, version),
                    None => Ok(None),
                }
            }
            Some(CrateProvenance::Workspace | CrateProvenance::LocalDependency) => {
                log::debug!("loading from local");
                match &self.local_source {
                    Some(source) => source.load(crate_name, version),
                    None => Ok(None),
                }
            }
            Some(CrateProvenance::DocsRs) => {
                log::debug!("loading from docs.rs");
                match &self.docsrs_source {
                    Some(source) => source.load(crate_name, version),
                    None => Ok(None),
                }
            }
            None => {
                log::debug!("No provenance hint available, cascading load for {crate_name}");
                let mut deferred_error = None;
                for source in self.sources() {
                    match source.load(crate_name, version) {
                        Ok(Some(data)) => return Ok(Some(data)),
                        Ok(None) => {}
                        Err(e) => deferred_error = Some(e),
                    }
                }
                match deferred_error {
                    Some(e) => Err(e),
                    None => Ok(None),
                }
            }
        }
    }
}

// Compile-time assertion that Store can be shared across threads (serve.rs
// state, TUI request thread).
#[allow(dead_code)]
const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}

    const fn check_store_thread_safety() {
        assert_send_sync::<Store>();
    }
};

#[cfg(test)]
mod cache_tests {
    use super::*;

    fn load_ok(value: &str) -> Result<Arc<String>, LoadFailure> {
        Ok(Arc::new(value.to_string()))
    }

    #[test]
    fn singleflight_caches_first_result() {
        let cache = Cache::<CrateName<'static>, String>::default();
        let name = CrateName::from("a".to_string());
        let first = cache
            .get_or_load(&name, |_| 1, || load_ok("first"))
            .unwrap();
        let second = cache
            .get_or_load(&name, |_| 1, || panic!("must not reload"))
            .unwrap();
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn negative_entries_are_cached_until_expiry() {
        let cache = Cache::<CrateName<'static>, String>::default();
        let name = CrateName::from("a".to_string());
        let failure = cache
            .get_or_load(&name, |_| 1, || Err(LoadFailure::Transient))
            .unwrap_err();
        assert_eq!(failure, LoadFailure::Transient);

        // Still cached: the loader must not run again.
        cache
            .get_or_load(&name, |_| 1, || panic!("negative entry must be cached"))
            .unwrap_err();

        // Age the entry past its TTL; the next access retries.
        cache
            .entries
            .lock()
            .unwrap()
            .map
            .get_mut(&name)
            .unwrap()
            .loaded_at -= TRANSIENT_TTL;
        let value = cache
            .get_or_load(&name, |_| 1, || load_ok("retried"))
            .unwrap();
        assert_eq!(*value, "retried");
    }

    #[test]
    fn eviction_skips_pinned_entries() {
        let cache = Cache::<CrateName<'static>, String> {
            cap: 25,
            ..Default::default()
        };
        let a = CrateName::from("a".to_string());
        let b = CrateName::from("b".to_string());
        let c = CrateName::from("c".to_string());
        let d = CrateName::from("d".to_string());

        let pin_a = cache.get_or_load(&a, |_| 10, || load_ok("a")).unwrap();
        cache.get_or_load(&b, |_| 10, || load_ok("b")).unwrap();
        // Inserting `c` goes over cap. `a` is least recently accessed but
        // pinned, so `b` is evicted instead.
        cache.get_or_load(&c, |_| 10, || load_ok("c")).unwrap();
        {
            let entries = cache.entries.lock().unwrap();
            assert!(entries.map.contains_key(&a));
            assert!(!entries.map.contains_key(&b));
            assert!(entries.map.contains_key(&c));
        }

        // Once the pin is released, the next over-cap insert can evict `a`.
        drop(pin_a);
        cache.get_or_load(&d, |_| 10, || load_ok("d")).unwrap();
        let entries = cache.entries.lock().unwrap();
        assert!(!entries.map.contains_key(&a));
        assert!(entries.map.contains_key(&c));
        assert!(entries.map.contains_key(&d));
    }

    #[test]
    fn eviction_removes_least_recently_accessed_when_over_cap() {
        let cache = Cache::<CrateName<'static>, String> {
            cap: 25,
            ..Default::default()
        };
        let a = CrateName::from("a".to_string());
        let b = CrateName::from("b".to_string());
        let c = CrateName::from("c".to_string());

        cache.get_or_load(&a, |_| 10, || load_ok("a")).unwrap();
        cache.get_or_load(&b, |_| 10, || load_ok("b")).unwrap();
        // Touch `a` so `b` is the least recently accessed.
        cache.get_or_load(&a, |_| 10, || unreachable!()).unwrap();
        // Inserting `c` pushes the total to 30 > 25, evicting `b`.
        cache.get_or_load(&c, |_| 10, || load_ok("c")).unwrap();

        let entries = cache.entries.lock().unwrap();
        assert!(entries.map.contains_key(&a));
        assert!(!entries.map.contains_key(&b));
        assert!(entries.map.contains_key(&c));
    }
}
