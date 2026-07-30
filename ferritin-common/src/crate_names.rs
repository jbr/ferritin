//! The crates.io namespace, as a local artifact.
//!
//! The full crates.io namespace — every crate's name, default version,
//! download rank and description — is published daily as a pair of zstd
//! artifacts (~2 MB and ~5.5 MB) by <https://github.com/jbr/crate-names>.
//! [`CrateIndex`] fetches them, caches them on disk, revalidates them with a
//! conditional GET (`If-None-Match`), and hands queries to the sans-io
//! [`crate_names`] readers.
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
mod probe;

use crate::string_utils::{case_aware_jaro_winkler, stem};
use anyhow::{Context, Result, anyhow};
pub use crate_names::normalize;
use crate_names::{CrateNames, Descriptions};
use rustc_hash::FxHashMap;
use semver::Version;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
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
            dir: cache_dir.join("crate-names"),
            fetch_lock: async_lock::Mutex::new(()),
            search_descriptions: AtomicBool::new(false),
            state: RwLock::new(State::default()),
        }
    }

    /// Declare that this process will search crate descriptions, so the
    /// [`DescriptionIndex`] should be built as part of loading rather than on
    /// the first query that needs it. Building it costs seconds of CPU over
    /// the whole namespace, which is fine once at startup and once a day after
    /// that, and not fine on a typeahead keystroke.
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

        // Description matching is the server's; a process that hasn't declared
        // it searches descriptions gets name matching alone rather than
        // silently paying seconds to index the namespace. That is the CLI,
        // which reaches this only for crate-name "did you mean".
        let weights = TypeaheadWeights {
            description_match: if self.search_descriptions.load(Ordering::Relaxed) {
                TypeaheadWeights::default().description_match
            } else {
                0.0
            },
            ..TypeaheadWeights::default()
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
        Some(match etags.descriptions.as_deref() {
            Some(descriptions) => format!("{names} {descriptions}"),
            None => names.to_string(),
        })
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

    /// Build the description index into freshly loaded data, if this process
    /// searches descriptions (see [`Self::search_descriptions`]).
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
            let index = DescriptionIndex::build(&loaded.names, &loaded.descriptions);
            let _ = loaded.description_index.set(index);
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

        let loaded = parse(
            &names,
            &descriptions,
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
        meta: &DiskMeta,
    ) -> Result<()> {
        async_fs::create_dir_all(&self.dir)
            .await
            .context("creating crate-names cache directory")?;

        let meta = sonic_rs::to_vec(meta).context("serializing crate-names metadata")?;
        for (path, bytes) in [
            (self.names_path(), names),
            (self.descriptions_path(), descriptions),
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

    /// GET both artifacts conditionally, returning `Ok(None)` when neither has
    /// changed. The two are versioned together and only meaningful together, so
    /// a change in either re-downloads both rather than leaving a half-updated
    /// pair on disk.
    async fn fetch(&self, etags: &Etags, last_modified: Option<u64>) -> Result<Option<Loaded>> {
        let names = self
            .fetch_one(&self.names_url, etags.names.as_deref())
            .await?;
        let descriptions = self
            .fetch_one(&self.descriptions_url, etags.descriptions.as_deref())
            .await?;

        if names.is_none() && descriptions.is_none() {
            self.touch_disk_meta(etags, last_modified).await;
            return Ok(None);
        }

        // One side changed and the other didn't: re-fetch the unchanged side
        // unconditionally rather than pairing new data with a stale buffer we
        // may not still hold (a cold start has no in-memory copy to reuse).
        let names = match names {
            Some(fetched) => fetched,
            None => self
                .fetch_one(&self.names_url, None)
                .await?
                .ok_or_else(|| anyhow!("names artifact reported not-modified without an etag"))?,
        };
        let descriptions = match descriptions {
            Some(fetched) => fetched,
            None => self
                .fetch_one(&self.descriptions_url, None)
                .await?
                .ok_or_else(|| {
                    anyhow!("descriptions artifact reported not-modified without an etag")
                })?,
        };

        let etags = Etags {
            names: names.etag,
            descriptions: descriptions.etag,
        };
        // The pair is published together, seconds apart; take the later stamp as
        // "the pair was current as of", falling back to whichever side carried
        // one. `None.max(Some(x)) == Some(x)`, so this also survives one side
        // omitting the header.
        let last_modified = names.last_modified.max(descriptions.last_modified);
        let loaded = parse(
            &names.bytes,
            &descriptions.bytes,
            etags.clone(),
            last_modified,
        )?;

        if let Err(error) = self
            .store_to_disk(
                &names.bytes,
                &descriptions.bytes,
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

/// Decompress and index a fetched or cached artifact pair.
fn parse(
    names: &[u8],
    descriptions: &[u8],
    etags: Etags,
    last_modified: Option<u64>,
) -> Result<Loaded> {
    Ok(Loaded {
        names: CrateNames::from_zstd(names).context("parsing crate names artifact")?,
        descriptions: Descriptions::from_zstd(descriptions)
            .context("parsing crate descriptions artifact")?,
        etags,
        last_modified,
        token_index: OnceLock::new(),
        trigram_index: OnceLock::new(),
        description_index: OnceLock::new(),
    })
}

/// Additive typeahead scoring weights. A crate's score is
/// `term_match · (query tokens matched) + whole_prefix (for a whole-name-prefix
/// match) + rank · (download rank)`, so matching more of the query dominates,
/// and within a match tier the log-quantized download rank (0..=255, ~8 units
/// per download-doubling) orders similar crates. Tuning knobs — the defaults
/// are the shipped values, and the probe harness in `tests` explores others.
#[derive(Debug, Clone, Copy)]
struct TypeaheadWeights {
    /// Per matched query token. Also the penalty for each *missed* token: a
    /// crate matching all terms of `trillium tokio` outscores a one-term match
    /// unless the popularity gap exceeds this many rank units — at 128,
    /// ~65,000×, a gap the namespace barely contains, so in practice full
    /// matches sort first and this stays additive only in the extremes.
    term_match: f32,
    /// Nudge for a whole-name-prefix match (the name starts with the query,
    /// whitespace folded to `-`) over an interior-token match of the same
    /// term count — ~1.5 download-doublings between similarly popular crates.
    whole_prefix: f32,
    /// Bonus when a *multi-token* query's every token exactly equals one of
    /// the crate's name tokens (no prefixing needed), lifting `tokio-util-*`
    /// over `tokio-utilities` once `util` is fully typed. Single-token queries
    /// are exempt: one token is usually mid-typing, and probing showed the
    /// bonus lifting obscure interior-exact matches (`assert-json-diff` for
    /// `json`) over popular continuations (`jsonwebtoken`). The complete-name
    /// case (`trillium tokio` ≡ `trillium-tokio`) is stronger still: the
    /// service layer hoists it to the front outright.
    all_exact: f32,
    /// Per query token matched in the crate's *description* (see
    /// [`DescriptionIndex`]) and **not** in its name. Credit per token is
    /// `max(name, description)`, never the sum: a token found in both is one
    /// piece of evidence seen twice, and summing it demotes the crate a query
    /// actually names in favor of its neighbors. `serde`'s own description
    /// says "serialization/deserialization framework", never "serde", so
    /// under a summing rule the query `serde` ranked `serde_spanned` and
    /// `serde_urlencoded` — which do say it — above `serde` itself.
    ///
    /// Deliberately a fraction of `term_match`: a description mention is
    /// weaker evidence than a name match, and its job is to reach crates the
    /// name index cannot see at all (`deserialization` → `serde`) rather than
    /// to reorder the ones it can. Zero disables description matching
    /// outright, including building the index.
    description_match: f32,
    /// Per-unit contribution of the download rank.
    rank: f32,
}

impl Default for TypeaheadWeights {
    fn default() -> Self {
        Self {
            term_match: 128.0,
            whole_prefix: 12.0,
            all_exact: 16.0,
            description_match: 96.0,
            rank: 1.0,
        }
    }
}

/// The scoring core of [`CrateIndex::typeahead`], parameterized by weights so
/// the probe harness can explore the space; production callers pass
/// [`TypeaheadWeights::default`].
fn typeahead_scored(
    loaded: &Loaded,
    prefix: &str,
    limit: usize,
    weights: &TypeaheadWeights,
) -> (Vec<CrateEntry>, usize) {
    let token_index = loaded
        .token_index
        .get_or_init(|| TokenIndex::build(&loaded.names));

    let mut query_tokens: Vec<String> = name_tokens(prefix).collect();
    query_tokens.sort_unstable();
    query_tokens.dedup();

    // Per-term credit: each crate is scored by how many distinct query tokens
    // it matched and how well, so a full match ranks above a subset match but
    // both are candidates. Description matching (when enabled) adds candidates
    // the name index cannot reach at all — `deserialization` finds serde.
    let description_index = (weights.description_match > 0.0).then(|| loaded.description_index());
    let mut candidates = match_counts(&query_tokens, token_index, description_index);

    // The crates where every query token matches a name token *exactly* — a
    // finished-typing signal worth a bonus over prefix-only full matches.
    let all_exact = all_exact_indices(&query_tokens, token_index);

    // Whole-name prefix, with query whitespace folded to `-` so a
    // space-separated query still matches a hyphenated name. Such a name
    // contains the entire query, so it credits every term (a token the
    // tokenizer dropped, e.g. a 1-char segment, still matched textually) plus
    // the whole-prefix nudge. Empty only for an all-whitespace query, which
    // then matches nothing.
    let whole_key = prefix.split_whitespace().collect::<Vec<_>>().join("-");
    let whole = if whole_key.is_empty() {
        0..0
    } else {
        loaded.names.prefix_indices(&whole_key)
    };
    for crate_index in whole.clone() {
        let matched = candidates.entry(crate_index as u32).or_default();
        matched.name = matched.name.max(query_tokens.len() as u32);
    }

    let mut scored: Vec<(f32, u32)> = candidates
        .iter()
        .filter_map(|(&crate_index, matched)| {
            let found = loaded.names.entry_at(crate_index as usize)?;
            let whole_bonus = if whole.contains(&(crate_index as usize)) {
                weights.whole_prefix
            } else {
                0.0
            };
            let exact_bonus = if all_exact.contains(&crate_index) {
                weights.all_exact
            } else {
                0.0
            };
            let score = weights.term_match * matched.name as f32
                + weights.description_match * matched.description as f32
                + whole_bonus
                + exact_bonus
                + weights.rank * f32::from(found.rank);
            Some((score, crate_index))
        })
        .collect();
    let total = scored.len();

    // Descending by score, ties broken by name so the order is total and the
    // page is deterministic.
    let ranked = |a: &(f32, u32), b: &(f32, u32)| {
        b.0.total_cmp(&a.0).then_with(|| {
            let a_name = loaded.names.entry_at(a.1 as usize).map(|e| e.name);
            let b_name = loaded.names.entry_at(b.1 as usize).map(|e| e.name);
            a_name.cmp(&b_name)
        })
    };

    // Select the page, then sort only it. Fully sorting is a real cost here,
    // not a micro-optimization: description matching routinely produces tens
    // of thousands of candidates, `rank` is a `u8` so they pile into 256 score
    // buckets, and the name tie-break that separates them costs two artifact
    // lookups per comparison. Sorting all 15k candidates of `random number
    // generator` to show 8 of them was 18 of the query's 28ms.
    if scored.len() > limit {
        scored.select_nth_unstable_by(limit, ranked);
        scored.truncate(limit);
    }
    scored.sort_by(ranked);

    let mut entries: Vec<CrateEntry> = scored
        .into_iter()
        .filter_map(|(_, crate_index)| entry(loaded, loaded.names.entry_at(crate_index as usize)?))
        .collect();

    // When prefix/token matching underfills the page, fill the remaining slots
    // with fuzzy matches — so a typo like `tokoi` still surfaces `tokio`. These
    // sort *after* every prefix/token match by construction (they are only
    // appended), and `total` is lifted to cover them so the caller doesn't read
    // the padded page as truncated.
    let mut total = total;
    if entries.len() < limit {
        let mut seen: HashSet<String> = entries.iter().map(|e| normalize(&e.name)).collect();
        for extra in fuzzy_scored(loaded, prefix, limit) {
            if entries.len() >= limit {
                break;
            }
            if seen.insert(normalize(&extra.name)) {
                entries.push(extra);
            }
        }
        total = total.max(entries.len());
    }

    (entries, total)
}

/// Safety ceiling on how many trigram-overlap candidates are scored with
/// [`case_aware_jaro_winkler`] per fuzzy query. Not a tuning knob: it is set far
/// above any real query's candidate set (the commonest trigrams reach ~13k
/// names) purely to bound a pathological query. Candidates are *not* pre-cut by
/// overlap count below this — doing so drops the true match in a boundary
/// transposition of a short name (`tokoi`/`tokio` share only the very common
/// `tok`), and jaro scoring is cheap enough (~200ns each) to run over the whole
/// natural candidate set. If the ceiling ever bites, the highest-overlap
/// candidates are kept, since a near match cannot share *fewer* trigrams than an
/// unrelated one at equal name length.
const FUZZY_CANDIDATE_CEILING: usize = 20_000;

/// Minimum [`case_aware_jaro_winkler`] similarity for a fuzzy match to be
/// offered. The floor exists so that genuine gibberish yields no suggestions at
/// all, rather than the five least-dissimilar random crates.
const FUZZY_THRESHOLD: f64 = 0.7;

/// Fuzzy crate-name matches for `query`, ranked by similarity then download
/// rank. Candidate generation is a trigram-overlap gather over [`TrigramIndex`]
/// (scored in full, save the [`FUZZY_CANDIDATE_CEILING`] backstop); survivors
/// are scored with [`case_aware_jaro_winkler`] and filtered to
/// [`FUZZY_THRESHOLD`]. Powers both the typeahead fuzzy fill above and
/// crate-name "did you mean" suggestions.
fn fuzzy_scored(loaded: &Loaded, query: &str, limit: usize) -> Vec<CrateEntry> {
    let index = loaded
        .trigram_index
        .get_or_init(|| TrigramIndex::build(&loaded.names));

    let norm = normalize(query);
    let mut grams: Vec<[u8; 3]> = trigrams(&norm).collect();
    grams.sort_unstable();
    grams.dedup();

    // Per-candidate count of how many distinct query trigrams it shares.
    let mut overlap: HashMap<u32, u32> = HashMap::new();
    for gram in &grams {
        for &crate_index in index.indices_with_trigram(gram) {
            *overlap.entry(crate_index).or_insert(0) += 1;
        }
    }

    let mut candidates: Vec<(u32, u32)> = overlap
        .into_iter()
        .map(|(crate_index, count)| (count, crate_index))
        .collect();
    // Only the pathological-query backstop cuts anything; real candidate sets
    // stay well under the ceiling and are scored in full.
    if candidates.len() > FUZZY_CANDIDATE_CEILING {
        candidates.select_nth_unstable_by(FUZZY_CANDIDATE_CEILING, |a, b| b.0.cmp(&a.0));
        candidates.truncate(FUZZY_CANDIDATE_CEILING);
    }

    let mut scored: Vec<(f64, u8, &str, usize)> = candidates
        .into_iter()
        .filter_map(|(_, crate_index)| {
            let found = loaded.names.entry_at(crate_index as usize)?;
            let score = case_aware_jaro_winkler(found.name, query);
            (score >= FUZZY_THRESHOLD).then_some((
                score,
                found.rank,
                found.name,
                crate_index as usize,
            ))
        })
        .collect();

    scored.sort_by(|a, b| {
        b.0.total_cmp(&a.0)
            .then_with(|| b.1.cmp(&a.1))
            .then_with(|| a.2.cmp(b.2))
    });
    scored.truncate(limit);

    scored
        .into_iter()
        .filter_map(|(_, _, _, crate_index)| entry(loaded, loaded.names.entry_at(crate_index)?))
        .collect()
}

/// Character trigrams of a normalized name, as fixed-size keys. A name shorter
/// than three bytes yields a single zero-padded key so it stays matchable
/// (crate names are ASCII after [`normalize`], so `0` never occurs in one).
fn trigrams(norm: &str) -> impl Iterator<Item = [u8; 3]> + '_ {
    let bytes = norm.as_bytes();
    let short = (bytes.len() < 3).then(|| {
        let mut key = [0u8; 3];
        key[..bytes.len()].copy_from_slice(bytes);
        key
    });
    let windows = (bytes.len() >= 3)
        .then(|| bytes.windows(3).map(|w| [w[0], w[1], w[2]]))
        .into_iter()
        .flatten();
    short.into_iter().chain(windows)
}

/// A lazily-built inverted index over the character trigrams of each crate's
/// normalized name, mapping each trigram to the [`CrateNames`] line indices it
/// occurs in. The fuzzy analogue of [`TokenIndex`]: same line-index values, same
/// build-once/drop-on-refresh lifecycle, but keyed by 3-byte character windows
/// so it can find near-misses (`tokoi` → `tokio`) that token prefixes cannot.
#[derive(Debug, Default)]
struct TrigramIndex {
    /// Sorted by trigram, so a lookup is a binary search. Values are sorted,
    /// deduped crate line indices.
    postings: Vec<([u8; 3], Vec<u32>)>,
}

impl TrigramIndex {
    fn build(names: &CrateNames) -> Self {
        let start = Instant::now();
        let mut map: BTreeMap<[u8; 3], Vec<u32>> = BTreeMap::new();
        for index in 0..names.len() {
            let Some(found) = names.entry_at(index) else {
                continue;
            };
            let norm = normalize(found.name);
            for gram in trigrams(&norm) {
                map.entry(gram).or_default().push(index as u32);
            }
        }
        let index = Self {
            postings: map
                .into_iter()
                .map(|(gram, mut indices)| {
                    // A trigram repeated within one name pushes its index
                    // twice adjacently; across names indices ascend. Either way
                    // adjacent-dedup leaves each index once.
                    indices.dedup();
                    (gram, indices)
                })
                .collect(),
        };
        log::debug!(
            "⏱️ built trigram index ({} trigrams) in {:?}",
            index.postings.len(),
            start.elapsed()
        );
        index
    }

    /// The crate line-indices whose name contains `gram` (already sorted and
    /// deduped at build time).
    fn indices_with_trigram(&self, gram: &[u8; 3]) -> &[u32] {
        let start = self.postings.partition_point(|(entry, _)| entry < gram);
        match self.postings.get(start) {
            Some((entry, indices)) if entry == gram => indices,
            _ => &[],
        }
    }
}

/// A lazily-built inverted index over crate-name tokens — the `-`/`_` separated
/// segments of each name — mapping each token to the [`CrateNames`] line indices
/// whose name contains it. Values are line indices, so a lookup refers back into
/// the artifact without copying names. Built on the first typeahead query and
/// dropped with each artifact refresh, so it never goes stale and the CLI —
/// which only calls [`CrateIndex::get`] — never pays to build it.
#[derive(Debug, Default)]
struct TokenIndex {
    /// Sorted by token, so a prefix range is a binary search.
    postings: Vec<(String, Vec<u32>)>,
}

impl TokenIndex {
    fn build(names: &CrateNames) -> Self {
        let mut map: BTreeMap<String, Vec<u32>> = BTreeMap::new();
        for index in 0..names.len() {
            let Some(found) = names.entry_at(index) else {
                continue;
            };
            for token in name_tokens(found.name) {
                map.entry(token).or_default().push(index as u32);
            }
        }
        Self {
            postings: map
                .into_iter()
                .map(|(token, mut indices)| {
                    // Indices were pushed in ascending order, so a token
                    // repeated within one name leaves adjacent dupes.
                    indices.dedup();
                    (token, indices)
                })
                .collect(),
        }
    }

    /// The crate line-indices having exactly this token (already sorted and
    /// deduped at build time).
    fn indices_with_token(&self, token: &str) -> &[u32] {
        let start = self
            .postings
            .partition_point(|(entry, _)| entry.as_str() < token);
        match self.postings.get(start) {
            Some((entry, indices)) if entry == token => indices,
            _ => &[],
        }
    }

    /// The distinct crate line-indices having a token that begins with `prefix`,
    /// sorted and deduped. Iteration stops at the first non-matching token in
    /// the binary-searched range.
    fn indices_with_prefix(&self, prefix: &str) -> Vec<u32> {
        let start = self
            .postings
            .partition_point(|(token, _)| token.as_str() < prefix);

        let mut indices = Vec::new();
        for (token, crate_indices) in &self.postings[start..] {
            if !token.starts_with(prefix) {
                break;
            }
            indices.extend_from_slice(crate_indices);
        }
        indices.sort_unstable();
        indices.dedup();
        indices
    }
}

/// Words shorter than this are not indexed from a description. Two-letter
/// words are almost entirely function words (`in`, `of`, `to`), and the
/// meaningful exceptions people search for (`io`, `os`) are crate *names*,
/// which the name index already covers.
const DESCRIPTION_MIN_CHARS: usize = 3;

/// A stem occurring in more than this fraction of all descriptions is dropped
/// from the index rather than kept: `rust`, `librari`, `implement`, `use` and
/// their kin match so much of the namespace that they only add noise and
/// postings. A frequency cut is preferred to a hand-written stopword list
/// because it adapts to what this corpus actually looks like — crates.io
/// descriptions are not general English — and needs no maintenance.
const DESCRIPTION_MAX_DOCUMENT_FREQUENCY: f32 = 0.05;

/// A lazily-built inverted index over the *stemmed* words of each crate's
/// crates.io description, mapping each stem to the [`CrateNames`] line indices
/// whose description contains it — the same line-index values as
/// [`TokenIndex`], so description matches and name matches score against one
/// candidate map.
///
/// Stemming is what makes this worth having: descriptions are prose, so a
/// query for `deserialize` must reach a description that says
/// `deserialization`, which exact token matching cannot do. Names get no such
/// treatment — a crate name is not an English word, and stemming would turn
/// `serde` into `serd`.
///
/// Same build-once/drop-on-refresh lifecycle as the other side indexes: it is
/// built on the first typeahead query and dropped with each artifact refresh,
/// so it is never stale, and the CLI — which only calls [`CrateIndex::get`] —
/// never pays to build it.
#[derive(Debug, Default)]
struct DescriptionIndex {
    /// Sorted by stem, so a prefix range is a binary search.
    postings: Vec<(String, Vec<u32>)>,
}

impl DescriptionIndex {
    /// Both artifacts are sorted by the same folded name key and the
    /// descriptions are a subset of the names, so one merge walk translates
    /// each description into the names line index it belongs to without
    /// building a 300k-entry name→index map.
    ///
    /// This walks the whole namespace, so it is written to allocate only when
    /// it has to: names are compared folded in place, each word is lowercased
    /// into one reusable buffer, and a stem is turned into a `String` only the
    /// first time it is seen.
    fn build(names: &CrateNames, descriptions: &Descriptions) -> Self {
        let start = Instant::now();
        let mut map: FxHashMap<String, Vec<u32>> = FxHashMap::default();
        let mut word = String::new();

        let mut cursor = 0;
        let mut cursor_name = names.entry_at(0).map(|entry| entry.name);
        for (name, description) in descriptions.iter() {
            while cursor_name.is_some_and(|current| folded_cmp(current, name).is_lt()) {
                cursor += 1;
                cursor_name = names.entry_at(cursor).map(|entry| entry.name);
            }
            if !cursor_name.is_some_and(|current| folded_cmp(current, name).is_eq()) {
                // A description for a crate the names artifact doesn't have.
                // The two are published together so this shouldn't happen, but
                // skipping is the only sane response and keeps the walk in step.
                continue;
            }
            for raw in description.split(|c: char| !c.is_alphanumeric()) {
                if raw.chars().count() < DESCRIPTION_MIN_CHARS {
                    continue;
                }
                word.clear();
                word.extend(raw.chars().flat_map(char::to_lowercase));
                let stemmed = stem(&word);
                match map.get_mut(stemmed.as_ref()) {
                    Some(indices) => indices.push(cursor as u32),
                    None => {
                        map.insert(stemmed.into_owned(), vec![cursor as u32]);
                    }
                }
            }
        }

        let ceiling = (names.len() as f32 * DESCRIPTION_MAX_DOCUMENT_FREQUENCY) as usize;
        let indexed = map.len();
        let mut postings: Vec<(String, Vec<u32>)> = map
            .into_iter()
            .filter_map(|(word, mut indices)| {
                // A stem repeated within one description pushes its index twice
                // adjacently; across crates indices ascend.
                indices.dedup();
                (indices.len() <= ceiling).then_some((word, indices))
            })
            .collect();
        postings.sort_unstable_by(|(a, _), (b, _)| a.cmp(b));

        log::debug!(
            "⏱️ built description index ({} stems, {} dropped above df {ceiling}) in {:?}",
            postings.len(),
            indexed - postings.len(),
            start.elapsed()
        );
        Self { postings }
    }

    /// The distinct crate line-indices whose description contains a stem
    /// beginning with `prefix`, sorted and deduped.
    fn indices_with_prefix(&self, prefix: &str) -> Vec<u32> {
        let start = self
            .postings
            .partition_point(|(word, _)| word.as_str() < prefix);

        let mut indices = Vec::new();
        for (word, crate_indices) in &self.postings[start..] {
            if !word.starts_with(prefix) {
                break;
            }
            indices.extend_from_slice(crate_indices);
        }
        indices.sort_unstable();
        indices.dedup();
        indices
    }
}

/// Order two crate names by the folded key the artifacts are sorted under —
/// ASCII case, with `-` and `_` equivalent — without allocating. Mirrors
/// [`crate_names::normalize`], of which the reader exposes only the allocating
/// form; the merge walk in [`DescriptionIndex::build`] does this ~600k times,
/// which is worth not allocating for.
fn folded_cmp(left: &str, right: &str) -> std::cmp::Ordering {
    fn fold(byte: u8) -> u8 {
        match byte {
            b'_' => b'-',
            other => other.to_ascii_lowercase(),
        }
    }
    left.bytes().map(fold).cmp(right.bytes().map(fold))
}

/// How many distinct query tokens a crate matched, split by where. A token is
/// counted in exactly one of the two — the name if it matched there, the
/// description otherwise — so a crate is never paid twice for one token.
#[derive(Debug, Clone, Copy, Default)]
struct MatchCounts {
    /// Tokens prefixing one of the crate's *name* tokens.
    name: u32,
    /// Tokens found only in the crate's *description*.
    description: u32,
}

/// Count each query token's best match per crate. The tokens are pre-deduped
/// and each index dedupes per token, so counting is exact; an empty query
/// yields no candidates.
///
/// Description tokens are stemmed before lookup so they meet the index on its
/// own terms, and then matched as prefixes so a word still being typed can
/// match. The residual gap is a *partly*-typed word whose stem diverges from
/// the whole word's (`deserializ` does not prefix `deseri`), which the name
/// and fuzzy passes still cover.
fn match_counts(
    query_tokens: &[String],
    names: &TokenIndex,
    descriptions: Option<&DescriptionIndex>,
) -> HashMap<u32, MatchCounts> {
    let mut counts: HashMap<u32, MatchCounts> = HashMap::new();
    for token in query_tokens {
        let named = names.indices_with_prefix(token);
        for &crate_index in &named {
            counts.entry(crate_index).or_default().name += 1;
        }

        let Some(descriptions) = descriptions else {
            continue;
        };
        if token.chars().count() < DESCRIPTION_MIN_CHARS {
            continue;
        }
        for crate_index in descriptions.indices_with_prefix(&stem(token)) {
            // `named` is sorted and deduped, so this is the cheap half of the
            // max: a token already credited to the name is not credited again.
            if named.binary_search(&crate_index).is_err() {
                counts.entry(crate_index).or_default().description += 1;
            }
        }
    }
    counts
}

/// The crates where *every* query token exactly equals one of the name's
/// tokens (the intersection of the exact posting lists, which are sorted).
/// Empty for queries of fewer than two tokens: the bonus is about *combining*
/// terms, and a lone token is usually still being typed (see
/// [`TypeaheadWeights::all_exact`]).
fn all_exact_indices(query_tokens: &[String], index: &TokenIndex) -> HashSet<u32> {
    if query_tokens.len() < 2 {
        return HashSet::new();
    }
    let mut postings = query_tokens
        .iter()
        .map(|token| index.indices_with_token(token));
    let Some(first) = postings.next() else {
        return HashSet::new();
    };
    let mut intersection: HashSet<u32> = first.iter().copied().collect();
    for posting in postings {
        let set: HashSet<u32> = posting.iter().copied().collect();
        intersection.retain(|index| set.contains(index));
    }
    intersection
}

/// Split a crate name into its lowercased alphanumeric segments
/// (`tokio-postgres` -> `tokio`, `postgres`), dropping single characters. The
/// whole-name prefix is handled by the sorted names table, so only interior
/// segments matter here.
///
/// Dropping 1-char segments is what keeps a single-character *query* cheap and
/// sensible now that there is no length floor: it tokenizes to nothing, so it
/// contributes no interior-token candidates and is answered by the whole-name
/// prefix alone. Admitting 1-char tokens would instead fan `s` out over every
/// name containing an `s`-initial segment, for no gain — a lone character is
/// mid-typing, and its whole-name prefix is the useful reading of it.
fn name_tokens(name: &str) -> impl Iterator<Item = String> + '_ {
    name.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|segment| segment.len() >= 2)
        .map(str::to_ascii_lowercase)
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
