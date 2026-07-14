//! Crate-name typeahead backed by the `crate-names` artifact.
//!
//! The full crates.io namespace (name, default version, download rank for
//! every crate) is published daily as a ~2 MB zstd artifact by
//! <https://github.com/jbr/crate-names>. [`TypeaheadService`] wraps a
//! `trillium_client::Client` that fetches it lazily on first use and
//! revalidates it with a conditional GET (`If-None-Match`) once per
//! [`REFRESH_INTERVAL`], handing queries to the sans-io
//! [`crate_names::CrateNames`] reader.
//!
//! Freshness semantics are stale-while-revalidate: once data is loaded,
//! queries are always answered immediately from it, and at most one request
//! pays for a revalidation in the background of its own query. Only a cold
//! start (or a server that has never successfully fetched) waits on the
//! network, and fetch failures are remembered briefly so an offline
//! `ferritin serve` degrades to fast 503s rather than hanging every
//! keystroke on a timeout.

use anyhow::{Context, Result, anyhow};
use crate_names::CrateNames;
use std::{
    sync::RwLock,
    time::{Duration, Instant},
};
use trillium_client::{Client, KnownHeaderName, Status};
use trillium_compression::client::Compression;
use trillium_logger::{Target, client::ClientLogger};
use trillium_redirect::client::FollowRedirects;
use trillium_rustls::RustlsConfig;
use trillium_smol::ClientConfig;

/// How long loaded data is served before a revalidation is attempted. The
/// artifact only changes once a day, so hourly conditional GETs (a 304 in
/// the common case) keep us within one hour of publication for free.
const REFRESH_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// After a failed fetch with nothing loaded, how long typeahead requests
/// fail fast (503) before the next network attempt.
const FAILURE_COOLDOWN: Duration = Duration::from_secs(60);

/// An owned typeahead result, decoupled from the artifact buffer's lifetime
/// so it can cross the lock boundary.
#[derive(Debug, Clone)]
pub(crate) struct TypeaheadEntry {
    pub(crate) name: String,
    pub(crate) version: String,
}

/// The top-ranked matches plus the exact number of crates matching the
/// prefix. `total` is free to compute (the prefix range is two binary
/// searches), so it is always exact — `entries.len() < total` means
/// truncation occurred.
#[derive(Debug)]
pub(crate) struct TypeaheadResults {
    pub(crate) entries: Vec<TypeaheadEntry>,
    pub(crate) total: usize,
}

struct Loaded {
    names: CrateNames,
    /// The artifact's ETag at fetch time, replayed as `If-None-Match`.
    etag: Option<String>,
}

#[derive(Default)]
struct State {
    loaded: Option<Loaded>,
    /// When we last attempted a fetch (successful or not); drives both the
    /// refresh interval and the failure cooldown.
    attempted_at: Option<Instant>,
}

/// Shared server state answering crate-name typeahead queries.
pub(crate) struct TypeaheadService {
    client: Client,
    url: String,
    /// The standard library crates, resolved once at startup. They are not on
    /// crates.io, so the artifact cannot know about them, but ferritin serves
    /// their documentation — see [`Self::std_matches`].
    std_crates: Vec<TypeaheadEntry>,
    /// Serializes fetches: cold-start requests queue behind one download,
    /// and `try_lock` gives stale-while-revalidate a single revalidator.
    fetch_lock: async_lock::Mutex<()>,
    state: RwLock<State>,
}

impl std::fmt::Debug for TypeaheadService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TypeaheadService")
            .field("url", &self.url)
            .finish_non_exhaustive()
    }
}

impl TypeaheadService {
    /// `std_crates` are the standard library crates this server can actually
    /// serve, with the toolchain's version — resolved at startup so that
    /// answering a query never has to reach for the [`Store`](ferritin_common::Store).
    pub(crate) fn new(std_crates: Vec<TypeaheadEntry>) -> Self {
        let client = Client::new(RustlsConfig::<ClientConfig>::default())
            .with_handler((
                ClientLogger::new().with_target(Target::Logger(log::Level::Info)),
                Compression::new(),
                FollowRedirects::new(),
            ))
            .with_timeout(Duration::from_secs(30))
            .with_default_header(
                KnownHeaderName::UserAgent,
                concat!("ferritin/", env!("CARGO_PKG_VERSION")),
            );

        Self {
            client,
            // The canonical artifact deployment; overridable (e.g. to point
            // at a local file server or a mirror).
            url: std::env::var("FERRITIN_CRATE_NAMES_URL")
                .unwrap_or_else(|_| crate_names::NAMES_URL_V2.into()),
            std_crates,
            fetch_lock: async_lock::Mutex::new(()),
            state: RwLock::new(State::default()),
        }
    }

    /// The standard library crates whose names start with `prefix`, folded the
    /// same way the artifact folds names, so `Std` and `proc-macro` match too.
    ///
    /// These are prepended to the crates.io results rather than ranked among
    /// them: `std` has no download count to rank by, and someone typing `std`
    /// on a Rust documentation site does not mean `stdweb`.
    fn std_matches(&self, prefix: &str) -> Vec<TypeaheadEntry> {
        let key = crate_names::normalize(prefix);
        self.std_crates
            .iter()
            .filter(|entry| crate_names::normalize(&entry.name).starts_with(&key))
            .cloned()
            .collect()
    }

    /// The top `limit` crates by download rank whose names start with
    /// `prefix`, plus the total match count. `None` means no data is
    /// available (cold start fetch failed or hasn't succeeded yet) — the
    /// endpoint maps it to a 503.
    pub(crate) async fn typeahead(&self, prefix: &str, limit: usize) -> Option<TypeaheadResults> {
        self.ensure_fresh().await;
        let state = self.state.read().unwrap();
        let loaded = state.loaded.as_ref()?;
        let mut total = loaded.names.count(prefix);
        let mut entries: Vec<TypeaheadEntry> = loaded
            .names
            .typeahead(prefix, limit)
            .into_iter()
            .map(|entry| TypeaheadEntry {
                name: entry.name.into(),
                version: entry.version.into(),
            })
            .collect();
        // An exact match always sorts first, regardless of rank: typing
        // `trillium` must offer `trillium` ahead of the more-downloaded
        // `trillium-http`.
        if let Some(position) = entries.iter().position(|entry| entry.name == prefix) {
            entries[..=position].rotate_right(1);
        } else if let Some(exact) = loaded.names.get(prefix) {
            entries.insert(
                0,
                TypeaheadEntry {
                    name: exact.name.into(),
                    version: exact.version.into(),
                },
            );
            entries.truncate(limit);
        }

        // The std crates are absent from the artifact but present on this
        // server, so they are added here rather than being matched by the
        // binary search. They count toward `total` for the same reason.
        let std_matches = self.std_matches(prefix);
        if !std_matches.is_empty() {
            total += std_matches.len();
            entries.splice(0..0, std_matches);
            entries.truncate(limit);
        }
        Some(TypeaheadResults { entries, total })
    }

    /// Whether enough time has passed since the last fetch attempt to try
    /// again — [`REFRESH_INTERVAL`] once loaded, [`FAILURE_COOLDOWN`] before.
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
            // Stale-while-revalidate: one request revalidates, concurrent
            // requests are answered from the stale data immediately.
            if let Some(_guard) = self.fetch_lock.try_lock() {
                self.refresh().await;
            }
        } else {
            // Cold start: queue behind a single download rather than racing.
            let _guard = self.fetch_lock.lock().await;
            if self.should_fetch() {
                self.refresh().await;
            }
        }
    }

    /// Fetch (conditionally) and swap in the result. Failures are logged and
    /// recorded, never propagated: stale data keeps serving, and an empty
    /// service reports unavailable.
    async fn refresh(&self) {
        let etag = {
            let state = self.state.read().unwrap();
            state.loaded.as_ref().and_then(|loaded| loaded.etag.clone())
        };
        let outcome = self.fetch(etag).await;
        let mut state = self.state.write().unwrap();
        state.attempted_at = Some(Instant::now());
        match outcome {
            Ok(Some(loaded)) => {
                log::info!(
                    "loaded {} crate names from {}",
                    loaded.names.len(),
                    self.url
                );
                state.loaded = Some(loaded);
            }
            Ok(None) => log::debug!("crate names artifact unchanged (304)"),
            Err(error) => log::warn!("failed to refresh crate names artifact: {error:#}"),
        }
    }

    /// GET the artifact, returning `Ok(None)` on a 304 Not Modified.
    async fn fetch(&self, etag: Option<String>) -> Result<Option<Loaded>> {
        let mut request = self.client.get(&*self.url);
        if let Some(etag) = &etag {
            request = request.with_request_header(KnownHeaderName::IfNoneMatch, etag.clone());
        }
        let conn = request.await.context("fetching crate names artifact")?;

        if conn.status() == Some(Status::NotModified) {
            return Ok(None);
        }

        let mut conn = conn
            .success()
            .map_err(|error| anyhow!("crate names artifact fetch failed: {error}"))?;
        let etag = conn
            .response_headers()
            .get_str(KnownHeaderName::Etag)
            .map(str::to_owned);
        let bytes = conn
            .response_body()
            .read_bytes()
            .await
            .context("reading crate names artifact")?;
        let names = CrateNames::from_zstd(&bytes).context("parsing crate names artifact")?;
        Ok(Some(Loaded { names, etag }))
    }
}
