//! The crates.io namespace, as a local artifact.
//!
//! The full crates.io namespace — every crate's name, default version,
//! download rank, description, and declared keywords/categories — is
//! published daily as a triple of zstd artifacts (~2 MB, ~5.5 MB, and ~2 MB)
//! by <https://github.com/jbr/crate-names>. [`CrateIndex`] fetches them,
//! caches them on disk, revalidates them with a conditional GET
//! (`If-None-Match`), and hands queries to the sans-io [`crate_names`]
//! readers.
//!
//! Two callers share one index:
//!
//! - Version resolution ([`crate::sources::DocsRsSource`]) reads a crate's default version and
//!   description from it instead of asking the crates.io API. This is the whole point of the
//!   artifact: one download answers every crate, where the API answers one crate per request.
//! - The server's crate-name typeahead reads the prefix ranges.
//!
//! # Freshness
//!
//! The artifact is rebuilt once a day, so answers derived from it can be up to
//! ~24h behind crates.io — a crate released this morning may still resolve to
//! yesterday's version, and one first published today is absent entirely.
//! Callers must treat a miss as "unknown", not "no such crate", and fall back
//! to the crates.io API; see [`DocsRsClient::resolve`](crate::sources::DocsRsSource).
//!
//! Within that bound, refresh is kept off the request path. A query never
//! fetches once data is loaded; it only reads what is in memory. Freshness is
//! someone else's job:
//!
//! - A long-lived server runs [`CrateIndex::run_periodic_refresh`] as a detached task, which loads
//!   the data once at startup and thereafter revalidates it with a conditional GET (a 304 in the
//!   common case) scheduled from the artifact's `Last-Modified` — once a day, timed to just after
//!   the next expected publish rather than polled hourly (see [`CrateIndex::refresh_delay`]). Every
//!   request is answered from whatever that task last loaded — within [`WATCH_INTERVAL`] of the
//!   artifact, itself up to a day behind crates.io.
//! - A short-lived CLI process runs no such task. Its first query cold-starts, loading from disk
//!   and revalidating then only if a new daily build is already due, and the process exits before
//!   that data could go stale in memory.
//!
//! Either way, only a genuine cold start (nothing in memory *or* on disk) waits
//! on the network, and cold-start failures are remembered briefly so an offline
//! process degrades to fast misses rather than hanging every lookup on a
//! timeout.
//!
//! # Tiers
//!
//! In-memory, then on disk, then the network. The disk tier is what makes this
//! viable for the CLI, whose every invocation is a fresh process: the artifacts
//! are downloaded once and thereafter read from the `crate-names` directory
//! under the ferritin cache root (see [`crate::ferritin_home`]), so a lookup
//! costs a decompression rather than a request.

#[cfg(test)]
mod battery;
#[cfg(test)]
mod probe;
mod search;

use anyhow::{Context, Result, anyhow};
pub use crate_names::normalize;
use crate_names::{CrateNames, Descriptions, Facets};
use search::*;
use semver::Version;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{
        OnceLock, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use trillium_client::{Client, KnownHeaderName, Status};
use trillium_compression::client::Compression;
use trillium_logger::{Target, client::ClientLogger};
use trillium_redirect::client::FollowRedirects;
use trillium_rustls::RustlsConfig;
use trillium_smol::{ClientConfig, async_io::Timer};

/// Expected wall-clock time between artifact publishes. The crate-names
/// workflow rebuilds once a day, so a copy we hold stays current until about
/// this long after its `Last-Modified`, and the next revalidation is *scheduled*
/// for then rather than polled for every hour. See
/// [`CrateIndex::run_periodic_refresh`].
const PUBLISH_PERIOD: Duration = Duration::from_secs(24 * 60 * 60);

/// How often to re-check once a rebuild is overdue: we are past the expected
/// publish but the artifact we hold has not changed yet (the workflow ran late,
/// or its response carried no `Last-Modified` to schedule from). Short enough to
/// catch a late publish promptly, long enough that a skipped day costs a handful
/// of conditional GETs rather than one an hour.
const WATCH_INTERVAL: Duration = Duration::from_secs(30 * 60);

/// After a failed fetch with nothing loaded, how long lookups fail fast before
/// the next network attempt.
const FAILURE_COOLDOWN: Duration = Duration::from_secs(60);

/// One crate as the artifact knows it: the name as spelled on crates.io, its
/// default version, and its description if it has one.
///
/// Owned rather than borrowed from the artifact buffer so it can cross the
/// lock boundary.
#[derive(Debug, Clone)]
pub struct CrateEntry {
    /// The name as spelled on crates.io, which is not necessarily how the
    /// caller spelled it — lookups fold `-`/`_` and case.
    pub name: String,
    /// The crates.io "default version": the version crates.io itself presents,
    /// typically the highest stable non-yanked release. The same field the
    /// crates.io API returns as `default_version`.
    pub version: Version,
    /// `None` for the crates that have no description on crates.io — they are
    /// simply absent from the descriptions artifact.
    pub description: Option<String>,
    /// Log-quantized all-time downloads. Ordering by it is meaningful,
    /// arithmetic on it is not.
    pub rank: u8,
}

/// The artifacts' upstream ETags, which together identify every answer derived
/// from them: the data changes only when the artifacts are replaced, and they
/// are replaced only when these change.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Etags {
    names: Option<String>,
    descriptions: Option<String>,
    /// `#[serde(default)]` so a sidecar written before the facets artifact
    /// existed still parses; the `None` makes the next fetch unconditional.
    #[serde(default)]
    facets: Option<String>,
}

/// The on-disk sidecar recording what the cached artifact files are and when
/// they were last known good, so a fresh process can skip the network and a
/// stale one can revalidate rather than re-download.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct DiskMeta {
    etags: Etags,
    /// Unix seconds at the last successful fetch *or revalidation*. A 304
    /// refreshes this without rewriting the artifacts.
    fetched_at: u64,
    /// The artifact's upstream `Last-Modified` as unix seconds — when this build
    /// was published — which is what the next revalidation is scheduled from.
    /// `None` when the response carried no parseable `Last-Modified` (or the
    /// sidecar predates this field), in which case we fall back to a
    /// [`WATCH_INTERVAL`] cadence off `fetched_at`.
    #[serde(default)]
    last_modified: Option<u64>,
}

impl DiskMeta {
    /// How long ago this was last known good. Saturating, so a clock that
    /// jumped backwards reads as "just now" rather than "impossibly stale".
    fn age(&self) -> Duration {
        Duration::from_secs(now_unix().saturating_sub(self.fetched_at))
    }

    /// Unix seconds at which the held artifact is due for revalidation: one
    /// [`PUBLISH_PERIOD`] after it was published, or — lacking a `Last-Modified`
    /// to anchor to — one [`WATCH_INTERVAL`] after we last confirmed it.
    fn due_at(&self) -> u64 {
        match self.last_modified {
            Some(published) => published.saturating_add(PUBLISH_PERIOD.as_secs()),
            None => self.fetched_at.saturating_add(WATCH_INTERVAL.as_secs()),
        }
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

struct Loaded {
    names: CrateNames,
    descriptions: Descriptions,
    facets: Facets,
    etags: Etags,
    /// The loaded build's `Last-Modified` as unix seconds — when it was
    /// published — which [`CrateIndex::refresh_delay`] schedules the next
    /// revalidation from. `None` if the response carried no parseable one.
    last_modified: Option<u64>,
    /// Inverted index over name tokens, built lazily on the first typeahead
    /// query (see [`TokenIndex`]).
    token_index: OnceLock<TokenIndex>,
    /// Inverted index over character trigrams, built lazily on the first
    /// *fuzzy* query (see [`TrigramIndex`]). Separate from `token_index`
    /// because most typeahead queries are answered by prefix/token matching
    /// alone and never pay for it.
    trigram_index: OnceLock<TrigramIndex>,
    /// Inverted index over the stemmed words of each crate's description
    /// (see [`DescriptionIndex`]). Built eagerly by
    /// [`CrateIndex::index_descriptions`] where descriptions are searched, and
    /// on first use otherwise.
    description_index: OnceLock<DescriptionIndex>,
    /// Inverted index over the stemmed declared keywords of each crate (see
    /// [`KeywordIndex`]). Same eager-where-searched lifecycle as
    /// `description_index`, though far cheaper to build.
    keyword_index: OnceLock<KeywordIndex>,
}

impl Loaded {
    /// The description index, building it if this is the first use. Costly
    /// enough that a server should have built it off the request path already
    /// (see [`CrateIndex::index_descriptions`]); this is the fallback, not the
    /// intended path.
    fn description_index(&self) -> &DescriptionIndex {
        self.description_index
            .get_or_init(|| DescriptionIndex::build(&self.names, &self.descriptions))
    }

    /// The keyword index, building it if this is the first use.
    fn keyword_index(&self) -> &KeywordIndex {
        self.keyword_index
            .get_or_init(|| KeywordIndex::build(&self.names, &self.facets))
    }
}

#[derive(Default)]
struct State {
    loaded: Option<Loaded>,
    /// When this process last attempted a cold-start fetch (successful or not),
    /// driving the [`FAILURE_COOLDOWN`] fast-fail. Process-local and only
    /// consulted before anything is loaded; scheduling once loaded is anchored
    /// to the artifact's `Last-Modified` instead (see [`DiskMeta::due_at`],
    /// [`CrateIndex::refresh_delay`]).
    attempted_at: Option<Instant>,
}

/// A local, queryable copy of the crates.io namespace. See the [module
/// docs](self).
pub struct CrateIndex {
    client: Client,
    names_url: String,
    descriptions_url: String,
    facets_url: String,
    /// Where the artifact files and their [`DiskMeta`] live.
    dir: PathBuf,
    /// Serializes fetches: concurrent cold-start callers queue behind one
    /// download, and the background refresher takes it so its revalidation never
    /// races a cold start.
    fetch_lock: async_lock::Mutex<()>,
    /// Whether this process searches crate descriptions, and so needs the
    /// [`DescriptionIndex`] built eagerly. See [`Self::search_descriptions`].
    search_descriptions: AtomicBool,
    state: RwLock<State>,
}

impl std::fmt::Debug for CrateIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CrateIndex")
            .field("dir", &self.dir)
            .field("loaded", &self.state.read().unwrap().loaded.is_some())
            .finish_non_exhaustive()
    }
}

impl CrateIndex {
    /// `cache_dir` is the ferritin cache root (see [`crate::ferritin_home`]);
    /// the artifacts live in a `crate-names` subdirectory of it. Construction
    /// is cheap and does no io — nothing is read or fetched until the first
    /// query.
    pub fn new(cache_dir: &Path) -> Self {
        let client = Client::new(RustlsConfig::<ClientConfig>::default())
            .with_handler((
                ClientLogger::new().with_target(Target::Logger(log::Level::Info)),
                Compression::new(),
                FollowRedirects::new(),
            ))
            .with_timeout(Duration::from_secs(30))
            .with_default_header(KnownHeaderName::UserAgent, crate::FERRITIN_USER_AGENT);

        Self {
            client,
            // The canonical artifact deployment; overridable (e.g. to point at
            // a local file server or a mirror).
            names_url: std::env::var("FERRITIN_CRATE_NAMES_URL")
                .unwrap_or_else(|_| crate_names::NAMES_URL_V2.into()),
            descriptions_url: std::env::var("FERRITIN_CRATE_DESCRIPTIONS_URL")
                .unwrap_or_else(|_| crate_names::DESCRIPTIONS_URL_V2.into()),
            facets_url: std::env::var("FERRITIN_CRATE_FACETS_URL")
                .unwrap_or_else(|_| crate_names::FACETS_URL_V1.into()),
            dir: cache_dir.join("crate-names"),
            fetch_lock: async_lock::Mutex::new(()),
            search_descriptions: AtomicBool::new(false),
            state: RwLock::new(State::default()),
        }
    }

    /// Declare that this process will search crate descriptions and declared
    /// keywords, so the [`DescriptionIndex`] and [`KeywordIndex`] should be
    /// built as part of loading rather than on the first query that needs
    /// them. Building them costs seconds of CPU over the whole namespace,
    /// which is fine once at startup and once a day after that, and not fine
    /// on a typeahead keystroke.
    ///
    /// Off by default because only the server searches descriptions: the CLI
    /// uses this index solely to resolve versions ([`Self::get`]), and would
    /// pay the whole cost for nothing. Set it *before* the first load — on an
    /// index that has already loaded, it takes effect at the next refresh, and
    /// queries until then fall back to building on demand.
    pub fn search_descriptions(&self) {
        self.search_descriptions.store(true, Ordering::Relaxed);
    }

    /// The crate spelled `name`, folding `-`/`_` and case the way crates.io
    /// does. `None` means the artifact does not have it — which, given the
    /// artifact's ~24h lag, means "not as of yesterday", not "does not exist".
    pub async fn get(&self, name: &str) -> Option<CrateEntry> {
        self.ensure_fresh().await;
        let state = self.state.read().unwrap();
        let loaded = state.loaded.as_ref()?;
        entry(loaded, loaded.names.get(name)?)
    }

    /// The top `limit` crates matching `prefix`, plus the total number that
    /// match. The query is tokenized, and a crate matches if any query token
    /// prefixes one of its name tokens — so `postgres` reaches
    /// `tokio-postgres`. Matching is additive rather than conjunctive: each
    /// matched query token adds [`TERM_MATCH_SCORE`], so for `trillium router`
    /// the full match `trillium-router` outscores the one-term `trillium-http`,
    /// which still appears further down. Download rank breaks ties within a
    /// match tier, and a large enough popularity gap can cross tiers; see
    /// [`TypeaheadWeights`].
    ///
    /// `None` means no data is available at all: nothing in memory, nothing on
    /// disk, and the network unreachable.
    pub async fn typeahead(&self, prefix: &str, limit: usize) -> Option<(Vec<CrateEntry>, usize)> {
        self.ensure_fresh().await;

        // Description and keyword matching are the server's; a process that
        // hasn't declared it searches descriptions gets name matching alone
        // rather than silently paying seconds to index the namespace. That is
        // the CLI, which reaches this only for crate-name "did you mean".
        let weights = if self.search_descriptions.load(Ordering::Relaxed) {
            TypeaheadWeights::default()
        } else {
            TypeaheadWeights {
                description_match: 0.0,
                keyword_match: 0.0,
                ..TypeaheadWeights::default()
            }
        };

        let state = self.state.read().unwrap();
        let loaded = state.loaded.as_ref()?;
        Some(typeahead_scored(loaded, prefix, limit, &weights))
    }

    /// An opaque identity for the currently loaded data, for callers deriving
    /// cache validators from artifact-backed answers. It changes exactly when
    /// the artifacts do.
    ///
    /// Not itself a well-formed ETag — it concatenates the two upstream ones —
    /// so hash it, don't emit it. `None` before the first successful load, or
    /// if the artifact server sent no ETag: we decline to invent an identity we
    /// cannot verify. Read *after* a query, once [`Self::ensure_fresh`] has run,
    /// so it describes the data that query actually saw.
    pub fn identity(&self) -> Option<String> {
        let state = self.state.read().unwrap();
        let etags = &state.loaded.as_ref()?.etags;
        let names = etags.names.as_deref()?;
        let mut identity = names.to_string();
        // Every artifact that can change an answer must move the identity —
        // a facets-only rebuild reorders keyword-matched results just as a
        // descriptions change reorders description-matched ones.
        for etag in [etags.descriptions.as_deref(), etags.facets.as_deref()]
            .into_iter()
            .flatten()
        {
            identity.push(' ');
            identity.push_str(etag);
        }
        Some(identity)
    }

    /// Whether enough time has passed since the last failed cold-start attempt
    /// to try loading again. Only consulted before anything is loaded — once
    /// data is in memory the query path stops fetching entirely (see
    /// [`Self::ensure_fresh`]), so this need only rate-limit the retry after a
    /// cold start found nothing in memory, nothing on disk, and the network
    /// down.
    fn should_fetch(&self) -> bool {
        let state = self.state.read().unwrap();
        state
            .attempted_at
            .is_none_or(|attempted| attempted.elapsed() >= FAILURE_COOLDOWN)
    }

    /// Load the data into memory if it is not there yet, and no more. Once
    /// loaded, the query path never touches the network again: a short-lived
    /// CLI process serves what it cold-started with, and a long-lived server
    /// keeps its copy current out of band via [`Self::run_periodic_refresh`].
    /// Only a genuine cold start — nothing in memory — reaches the network here,
    /// and it does so behind the lock so concurrent first queries load once.
    async fn ensure_fresh(&self) {
        if self.state.read().unwrap().loaded.is_some() {
            return;
        }
        if !self.should_fetch() {
            return;
        }
        let _guard = self.fetch_lock.lock().await;
        // Re-check under the lock: a concurrent cold start — or the background
        // refresher — may have loaded the data while we queued for it.
        if self.state.read().unwrap().loaded.is_none() && self.should_fetch() {
            self.refresh().await;
        }
    }

    /// Revalidate the in-memory data against the artifact server once, behind
    /// the fetch lock so it never races a cold-start [`Self::ensure_fresh`]. A
    /// long-lived process calls this on an interval via
    /// [`Self::run_periodic_refresh`]; the first call also performs the initial
    /// load (from disk, or the network if there is no disk copy).
    pub async fn refresh_once(&self) {
        let _guard = self.fetch_lock.lock().await;
        self.refresh().await;
    }

    /// Keep the in-memory data fresh for the lifetime of a long-lived process:
    /// load it once immediately, then revalidate on the [`Self::refresh_delay`]
    /// cadence — once a day, timed to the artifact's publication rather than
    /// polled hourly. Never returns — spawn it as a detached task. This is what
    /// moves refresh off the request path: with a refresher running, queries
    /// only ever read the loaded data, and the daily rebuild is picked up by
    /// this task rather than by whichever request happened to notice.
    ///
    /// The CLI does not run this. Each short-lived invocation instead
    /// cold-starts on its first query and revalidates a stale disk copy then.
    pub async fn run_periodic_refresh(&self) {
        loop {
            self.refresh_once().await;
            let delay = self.refresh_delay();
            log::debug!("next crate-names revalidation in {delay:?}");
            Timer::after(delay).await;
        }
    }

    /// How long until the loaded artifact's next revalidation, from its
    /// `Last-Modified`: the daily rebuild is due one [`PUBLISH_PERIOD`] after the
    /// build we hold, so we sleep until then and spend one conditional GET a day
    /// instead of one an hour. Once that moment has passed — a rebuild is
    /// overdue, or we never had a `Last-Modified` to anchor to — we poll at
    /// [`WATCH_INTERVAL`] until the new artifact lands. Capped at
    /// [`PUBLISH_PERIOD`] so even a bogus far-future timestamp rechecks daily.
    fn refresh_delay(&self) -> Duration {
        let last_modified = self
            .state
            .read()
            .unwrap()
            .loaded
            .as_ref()
            .and_then(|loaded| loaded.last_modified);
        let Some(published) = last_modified else {
            return WATCH_INTERVAL;
        };
        let due = published.saturating_add(PUBLISH_PERIOD.as_secs());
        let now = now_unix();
        if now >= due {
            WATCH_INTERVAL
        } else {
            Duration::from_secs(due - now).min(PUBLISH_PERIOD)
        }
    }

    /// Bring the in-memory data up to date from disk and, if that is not fresh
    /// enough, from the network. Failures are logged and recorded, never
    /// propagated: stale data keeps serving, and an empty index reports misses.
    async fn refresh(&self) {
        // The disk tier: for a CLI process, "nothing in memory" is the normal
        // state, and the artifacts it needs are usually already sitting in the
        // cache directory. A fresh copy there means no network at all.
        if self.state.read().unwrap().loaded.is_none()
            && let Some((loaded, meta)) = self.load_from_disk().await
        {
            let fresh = now_unix() < meta.due_at();
            let loaded = self.indexed(loaded).await;
            let mut state = self.state.write().unwrap();
            state.loaded = Some(loaded);
            if fresh {
                state.attempted_at = Some(Instant::now());
                return;
            }
        }

        let (etags, last_modified) = {
            let state = self.state.read().unwrap();
            state
                .loaded
                .as_ref()
                .map(|loaded| (loaded.etags.clone(), loaded.last_modified))
                .unwrap_or_default()
        };

        let outcome = self.fetch(&etags, last_modified).await;

        // Indexing before taking the write lock, not after: it is seconds of
        // CPU, and holding the lock across it would stall every query on a
        // refresh. The new data simply becomes visible a little later, fully
        // built, and until then queries keep reading the previous copy.
        let outcome = match outcome {
            Ok(Some(loaded)) => Ok(Some(self.indexed(loaded).await)),
            other => other,
        };

        let mut state = self.state.write().unwrap();
        state.attempted_at = Some(Instant::now());
        match outcome {
            Ok(Some(loaded)) => {
                log::info!(
                    "loaded {} crate names and {} descriptions from {}",
                    loaded.names.len(),
                    loaded.descriptions.len(),
                    self.names_url
                );
                state.loaded = Some(loaded);
            }
            Ok(None) => log::debug!("crate-names artifacts unchanged (304)"),
            Err(error) => log::warn!("failed to refresh crate-names artifacts: {error:#}"),
        }
    }

    /// Build the description and keyword indexes into freshly loaded data, if
    /// this process searches descriptions (see [`Self::search_descriptions`]).
    ///
    /// Indexing the whole namespace's prose is seconds of CPU — far too much
    /// to put on a query, and too much to run on an async worker — so it goes
    /// to a blocking thread, and it happens *here*, between loading and
    /// publishing, so the data is never visible in a half-built state. A
    /// process that never searches descriptions (the CLI) skips it entirely
    /// and pays nothing.
    async fn indexed(&self, loaded: Loaded) -> Loaded {
        if !self.search_descriptions.load(Ordering::Relaxed) {
            return loaded;
        }
        trillium_smol::async_global_executor::spawn_blocking(move || {
            loaded.description_index();
            loaded.keyword_index();
            loaded
        })
        .await
    }

    fn names_path(&self) -> PathBuf {
        self.dir.join(crate_names::NAMES_FILE_V2)
    }

    fn descriptions_path(&self) -> PathBuf {
        self.dir.join(crate_names::DESCRIPTIONS_FILE_V2)
    }

    fn facets_path(&self) -> PathBuf {
        self.dir.join(crate_names::FACETS_FILE_V1)
    }

    fn meta_path(&self) -> PathBuf {
        self.dir.join("meta.json")
    }

    /// Read the artifacts previously written by [`Self::store_to_disk`]. Any
    /// failure — missing, torn, or unparseable — is a plain miss: the network
    /// tier will refill them.
    async fn load_from_disk(&self) -> Option<(Loaded, DiskMeta)> {
        let start = Instant::now();
        let meta: DiskMeta =
            sonic_rs::serde::from_slice(&async_fs::read(self.meta_path()).await.ok()?).ok()?;
        let names = async_fs::read(self.names_path()).await.ok()?;
        let descriptions = async_fs::read(self.descriptions_path()).await.ok()?;
        // Absent on a cache written before the facets artifact existed; the
        // miss falls through to a full network refill, once.
        let facets = async_fs::read(self.facets_path()).await.ok()?;

        let loaded = parse(
            &names,
            &descriptions,
            &facets,
            meta.etags.clone(),
            meta.last_modified,
        )
        .inspect_err(|error| log::warn!("discarding cached crate-names artifacts: {error:#}"))
        .ok()?;

        log::debug!(
            "⏱️ loaded {} crate names from {} in {:?} ({:?} old)",
            loaded.names.len(),
            self.dir.display(),
            start.elapsed(),
            meta.age()
        );
        Some((loaded, meta))
    }

    /// Write the artifact bytes and their metadata via temp file + atomic
    /// rename, so a concurrent process can never observe a torn file. The temp
    /// name carries our PID to avoid colliding with a concurrent writer.
    async fn store_to_disk(
        &self,
        names: &[u8],
        descriptions: &[u8],
        facets: &[u8],
        meta: &DiskMeta,
    ) -> Result<()> {
        async_fs::create_dir_all(&self.dir)
            .await
            .context("creating crate-names cache directory")?;

        let meta = sonic_rs::to_vec(meta).context("serializing crate-names metadata")?;
        for (path, bytes) in [
            (self.names_path(), names),
            (self.descriptions_path(), descriptions),
            (self.facets_path(), facets),
            (self.meta_path(), &meta[..]),
        ] {
            let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
            async_fs::write(&tmp, bytes)
                .await
                .with_context(|| format!("writing {}", tmp.display()))?;
            async_fs::rename(&tmp, &path)
                .await
                .with_context(|| format!("committing {}", path.display()))?;
        }
        Ok(())
    }

    /// Record a revalidation that found nothing changed, so the *next* process
    /// reads the disk copy as fresh instead of revalidating it again. Preserves
    /// the artifact's `Last-Modified` — a 304 doesn't change when it was
    /// published — so the schedule stays anchored. Best effort: if the metadata
    /// can't be rewritten, the only cost is a redundant conditional GET later.
    async fn touch_disk_meta(&self, etags: &Etags, last_modified: Option<u64>) {
        let meta = DiskMeta {
            etags: etags.clone(),
            fetched_at: now_unix(),
            last_modified,
        };
        let write = async {
            let path = self.meta_path();
            let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
            async_fs::write(&tmp, sonic_rs::to_vec(&meta)?).await?;
            async_fs::rename(&tmp, &path).await?;
            anyhow::Ok(())
        };
        if let Err(error) = write.await {
            log::debug!("could not refresh crate-names metadata: {error:#}");
        }
    }

    /// GET all three artifacts conditionally, returning `Ok(None)` when none
    /// has changed. The triple is versioned together and only meaningful
    /// together, so a change in any re-downloads all three rather than
    /// leaving a half-updated set on disk.
    async fn fetch(&self, etags: &Etags, last_modified: Option<u64>) -> Result<Option<Loaded>> {
        let names = self
            .fetch_one(&self.names_url, etags.names.as_deref())
            .await?;
        let descriptions = self
            .fetch_one(&self.descriptions_url, etags.descriptions.as_deref())
            .await?;
        let facets = self
            .fetch_one(&self.facets_url, etags.facets.as_deref())
            .await?;

        if names.is_none() && descriptions.is_none() && facets.is_none() {
            self.touch_disk_meta(etags, last_modified).await;
            return Ok(None);
        }

        let names = self.ensure_fetched(&self.names_url, names).await?;
        let descriptions = self
            .ensure_fetched(&self.descriptions_url, descriptions)
            .await?;
        let facets = self.ensure_fetched(&self.facets_url, facets).await?;

        let etags = Etags {
            names: names.etag,
            descriptions: descriptions.etag,
            facets: facets.etag,
        };
        // The triple is published together, seconds apart; take the latest
        // stamp as "the set was current as of", falling back to whichever side
        // carried one. `None.max(Some(x)) == Some(x)`, so this also survives a
        // side omitting the header.
        let last_modified = names
            .last_modified
            .max(descriptions.last_modified)
            .max(facets.last_modified);
        let loaded = parse(
            &names.bytes,
            &descriptions.bytes,
            &facets.bytes,
            etags.clone(),
            last_modified,
        )?;

        if let Err(error) = self
            .store_to_disk(
                &names.bytes,
                &descriptions.bytes,
                &facets.bytes,
                &DiskMeta {
                    etags,
                    fetched_at: now_unix(),
                    last_modified,
                },
            )
            .await
        {
            // A read-only or full cache directory costs us the disk tier, not
            // the answer: this process has the data in memory either way.
            log::warn!("could not cache crate-names artifacts: {error:#}");
        }

        Ok(Some(loaded))
    }

    /// The already-fetched artifact — or, when its conditional GET said
    /// not-modified while a sibling artifact changed, a second, unconditional
    /// fetch. Re-fetching beats pairing new data with a stale buffer we may
    /// not still hold (a cold start has no in-memory copy to reuse).
    async fn ensure_fetched(&self, url: &str, fetched: Option<Fetched>) -> Result<Fetched> {
        match fetched {
            Some(fetched) => Ok(fetched),
            None => self
                .fetch_one(url, None)
                .await?
                .ok_or_else(|| anyhow!("{url} reported not-modified to an unconditional request")),
        }
    }

    /// GET one artifact, returning `Ok(None)` on a 304 Not Modified.
    async fn fetch_one(&self, url: &str, etag: Option<&str>) -> Result<Option<Fetched>> {
        let mut request = self.client.get(url);
        if let Some(etag) = etag {
            request = request.with_request_header(KnownHeaderName::IfNoneMatch, etag.to_owned());
        }
        let conn = request.await.with_context(|| format!("fetching {url}"))?;

        if conn.status() == Some(Status::NotModified) {
            return Ok(None);
        }

        let mut conn = conn
            .success()
            .map_err(|error| anyhow!("fetching {url} failed: {error}"))?;
        let etag = conn
            .response_headers()
            .get_str(KnownHeaderName::Etag)
            .map(str::to_owned);
        let last_modified = conn
            .response_headers()
            .get_str(KnownHeaderName::LastModified)
            .and_then(parse_http_date);
        let bytes = conn
            .response_body()
            .read_bytes()
            .await
            .with_context(|| format!("reading {url}"))?;
        Ok(Some(Fetched {
            bytes,
            etag,
            last_modified,
        }))
    }
}

/// One artifact fetched from the network: its bytes and the response's cache
/// validators. `etag` feeds the next `If-None-Match`; `last_modified` (unix
/// seconds) anchors the refresh schedule (see [`CrateIndex::refresh_delay`]).
struct Fetched {
    bytes: Vec<u8>,
    etag: Option<String>,
    last_modified: Option<u64>,
}

/// Parse an HTTP-date header value (`Last-Modified`) to unix seconds, or `None`
/// if it is malformed or predates the epoch.
fn parse_http_date(value: &str) -> Option<u64> {
    let time = httpdate::parse_http_date(value).ok()?;
    Some(time.duration_since(UNIX_EPOCH).ok()?.as_secs())
}

/// Decompress and index a fetched or cached artifact triple.
fn parse(
    names: &[u8],
    descriptions: &[u8],
    facets: &[u8],
    etags: Etags,
    last_modified: Option<u64>,
) -> Result<Loaded> {
    Ok(Loaded {
        names: CrateNames::from_zstd(names).context("parsing crate names artifact")?,
        descriptions: Descriptions::from_zstd(descriptions)
            .context("parsing crate descriptions artifact")?,
        facets: Facets::from_zstd(facets).context("parsing crate facets artifact")?,
        etags,
        last_modified,
        token_index: OnceLock::new(),
        trigram_index: OnceLock::new(),
        description_index: OnceLock::new(),
        keyword_index: OnceLock::new(),
    })
}

/// Join a names entry to its description and owned form. A version the artifact
/// spells in a way semver won't parse drops the crate from the results
/// entirely — better to fall through to the crates.io API than to answer wrong.
fn entry(loaded: &Loaded, found: crate_names::Entry<'_>) -> Option<CrateEntry> {
    Some(CrateEntry {
        name: found.name.to_string(),
        version: Version::parse(found.version)
            .inspect_err(|error| {
                log::warn!(
                    "crate-names artifact has unparseable version {:?} for {}: {error}",
                    found.version,
                    found.name
                );
            })
            .ok()?,
        description: loaded.descriptions.get(found.name).map(str::to_string),
        rank: found.rank,
    })
}
