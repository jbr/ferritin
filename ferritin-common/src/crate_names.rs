//! The crates.io namespace, as a local artifact.
//!
//! The full crates.io namespace — every crate's name, default version,
//! download rank and description — is published daily as a pair of zstd
//! artifacts (~2 MB and ~5.5 MB) by <https://github.com/jbr/crate-names>.
//! [`CrateIndex`] fetches them, caches them on disk, revalidates them with a
//! conditional GET (`If-None-Match`) once per [`REFRESH_INTERVAL`], and hands
//! queries to the sans-io [`crate_names`] readers.
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
//! Within that bound, freshness is stale-while-revalidate: once data is loaded,
//! queries are answered immediately from it, and at most one caller pays for a
//! revalidation in the background of its own query. Only a cold start (nothing
//! in memory *or* on disk) waits on the network, and fetch failures are
//! remembered briefly so an offline process degrades to fast misses rather than
//! hanging every lookup on a timeout.
//!
//! # Tiers
//!
//! In-memory, then on disk, then the network. The disk tier is what makes this
//! viable for the CLI, whose every invocation is a fresh process: the artifacts
//! are downloaded once and thereafter read from `~/.cargo/rustdoc-json`, so a
//! lookup costs a decompression rather than a request.

#[cfg(test)]
mod probe;

use anyhow::{Context, Result, anyhow};
pub use crate_names::normalize;
use crate_names::{CrateNames, Descriptions};
use semver::Version;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{OnceLock, RwLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use trillium_client::{Client, KnownHeaderName, Status};
use trillium_compression::client::Compression;
use trillium_logger::{Target, client::ClientLogger};
use trillium_redirect::client::FollowRedirects;
use trillium_rustls::RustlsConfig;
use trillium_smol::ClientConfig;

/// How long loaded data is served before a revalidation is attempted. The
/// artifacts only change once a day, so hourly conditional GETs (a 304 in the
/// common case) keep us within an hour of publication for the cost of one
/// round trip an hour, shared across every crate.
const REFRESH_INTERVAL: Duration = Duration::from_secs(60 * 60);

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
}

impl DiskMeta {
    /// How long ago this was last known good. Saturating, so a clock that
    /// jumped backwards reads as "just now" rather than "impossibly stale".
    fn age(&self) -> Duration {
        Duration::from_secs(now_unix().saturating_sub(self.fetched_at))
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
    /// Inverted index over name tokens, built lazily on the first typeahead
    /// query (see [`TokenIndex`]).
    token_index: OnceLock<TokenIndex>,
}

#[derive(Default)]
struct State {
    loaded: Option<Loaded>,
    /// When this process last attempted a fetch (successful or not); drives
    /// both the refresh interval and the failure cooldown. Process-local, so a
    /// CLI invocation always consults [`DiskMeta::age`] instead.
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
    /// Serializes fetches: cold-start callers queue behind one download, and
    /// `try_lock` gives stale-while-revalidate a single revalidator.
    fetch_lock: async_lock::Mutex<()>,
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
    /// `cache_dir` is the docs.rs cache directory; the artifacts live in a
    /// `crate-names` subdirectory of it. Construction is cheap and does no
    /// io — nothing is read or fetched until the first query.
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
            state: RwLock::new(State::default()),
        }
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
        let state = self.state.read().unwrap();
        let loaded = state.loaded.as_ref()?;
        Some(typeahead_scored(
            loaded,
            prefix,
            limit,
            &TypeaheadWeights::default(),
        ))
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

    /// Whether enough time has passed since the last attempt to try again —
    /// [`REFRESH_INTERVAL`] once loaded, [`FAILURE_COOLDOWN`] before.
    fn should_fetch(&self) -> bool {
        let state = self.state.read().unwrap();
        let interval = if state.loaded.is_some() {
            REFRESH_INTERVAL
        } else {
            FAILURE_COOLDOWN
        };
        state
            .attempted_at
            .is_none_or(|attempted| attempted.elapsed() >= interval)
    }

    async fn ensure_fresh(&self) {
        if !self.should_fetch() {
            return;
        }
        if self.state.read().unwrap().loaded.is_some() {
            // Stale-while-revalidate: one caller revalidates, concurrent
            // callers are answered from the stale data immediately.
            if let Some(_guard) = self.fetch_lock.try_lock() {
                self.refresh().await;
            }
        } else {
            // Cold start: queue behind a single load rather than racing.
            let _guard = self.fetch_lock.lock().await;
            if self.should_fetch() {
                self.refresh().await;
            }
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
            let fresh = meta.age() < REFRESH_INTERVAL;
            let mut state = self.state.write().unwrap();
            state.loaded = Some(loaded);
            if fresh {
                state.attempted_at = Some(Instant::now());
                return;
            }
        }

        let etags = {
            let state = self.state.read().unwrap();
            state
                .loaded
                .as_ref()
                .map(|loaded| loaded.etags.clone())
                .unwrap_or_default()
        };

        let outcome = self.fetch(&etags).await;

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

        let loaded = parse(&names, &descriptions, meta.etags.clone())
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
    /// reads the disk copy as fresh instead of revalidating it again. Best
    /// effort: if the metadata can't be rewritten, the only cost is a redundant
    /// conditional GET later.
    async fn touch_disk_meta(&self, etags: &Etags) {
        let meta = DiskMeta {
            etags: etags.clone(),
            fetched_at: now_unix(),
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
    async fn fetch(&self, etags: &Etags) -> Result<Option<Loaded>> {
        let names = self
            .fetch_one(&self.names_url, etags.names.as_deref())
            .await?;
        let descriptions = self
            .fetch_one(&self.descriptions_url, etags.descriptions.as_deref())
            .await?;

        if names.is_none() && descriptions.is_none() {
            self.touch_disk_meta(etags).await;
            return Ok(None);
        }

        // One side changed and the other didn't: re-fetch the unchanged side
        // unconditionally rather than pairing new data with a stale buffer we
        // may not still hold (a cold start has no in-memory copy to reuse).
        let (names, names_etag) = match names {
            Some(fetched) => fetched,
            None => self
                .fetch_one(&self.names_url, None)
                .await?
                .ok_or_else(|| anyhow!("names artifact reported not-modified without an etag"))?,
        };
        let (descriptions, descriptions_etag) = match descriptions {
            Some(fetched) => fetched,
            None => self
                .fetch_one(&self.descriptions_url, None)
                .await?
                .ok_or_else(|| {
                    anyhow!("descriptions artifact reported not-modified without an etag")
                })?,
        };

        let etags = Etags {
            names: names_etag,
            descriptions: descriptions_etag,
        };
        let loaded = parse(&names, &descriptions, etags.clone())?;

        if let Err(error) = self
            .store_to_disk(
                &names,
                &descriptions,
                &DiskMeta {
                    etags,
                    fetched_at: now_unix(),
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
    async fn fetch_one(
        &self,
        url: &str,
        etag: Option<&str>,
    ) -> Result<Option<(Vec<u8>, Option<String>)>> {
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
        let bytes = conn
            .response_body()
            .read_bytes()
            .await
            .with_context(|| format!("reading {url}"))?;
        Ok(Some((bytes, etag)))
    }
}

/// Decompress and index a fetched or cached artifact pair.
fn parse(names: &[u8], descriptions: &[u8], etags: Etags) -> Result<Loaded> {
    Ok(Loaded {
        names: CrateNames::from_zstd(names).context("parsing crate names artifact")?,
        descriptions: Descriptions::from_zstd(descriptions)
            .context("parsing crate descriptions artifact")?,
        etags,
        token_index: OnceLock::new(),
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
    /// Per-unit contribution of the download rank.
    rank: f32,
}

impl Default for TypeaheadWeights {
    fn default() -> Self {
        Self {
            term_match: 128.0,
            whole_prefix: 12.0,
            all_exact: 16.0,
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

    // Name-token matches, with per-term credit: each crate is scored by how
    // many distinct query tokens prefix one of its name tokens, so a full
    // match ranks above a subset match but both are candidates.
    let mut candidates = term_match_counts(&query_tokens, token_index);

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
        let matched = candidates.entry(crate_index as u32).or_insert(0);
        *matched = (*matched).max(query_tokens.len() as u32);
    }

    let mut scored: Vec<(f32, u32)> = candidates
        .iter()
        .filter_map(|(&crate_index, &matched)| {
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
            let score = weights.term_match * matched as f32
                + whole_bonus
                + exact_bonus
                + weights.rank * f32::from(found.rank);
            Some((score, crate_index))
        })
        .collect();
    let total = scored.len();

    scored.sort_by(|a, b| {
        b.0.total_cmp(&a.0).then_with(|| {
            let a_name = loaded.names.entry_at(a.1 as usize).map(|e| e.name);
            let b_name = loaded.names.entry_at(b.1 as usize).map(|e| e.name);
            a_name.cmp(&b_name)
        })
    });
    scored.truncate(limit);

    let entries = scored
        .into_iter()
        .filter_map(|(_, crate_index)| entry(loaded, loaded.names.entry_at(crate_index as usize)?))
        .collect();
    (entries, total)
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

/// For each crate with a name token prefixed by at least one query token, how
/// many distinct query tokens matched (the tokens are pre-deduped, and
/// `indices_with_prefix` dedupes per token, so counting is exact). An empty
/// query yields no candidates.
fn term_match_counts(query_tokens: &[String], index: &TokenIndex) -> HashMap<u32, u32> {
    let mut counts = HashMap::new();
    for token in query_tokens {
        for crate_index in index.indices_with_prefix(token) {
            *counts.entry(crate_index).or_insert(0) += 1;
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
