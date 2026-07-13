use crate::RustdocData;
use crate::sources::CrateProvenance;
use anyhow::{Context, Result, anyhow};
use fieldwork::Fieldwork;
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use trillium_client::{Client, HeaderValue, KnownHeaderName, Status};
use trillium_client_retry::RetryHandler;
use trillium_compression::client::Compression;
use trillium_logger::{Target, client::ClientLogger};
use trillium_redirect::client::FollowRedirects;
use trillium_rustls::RustlsConfig;
use trillium_smol::ClientConfig;

pub const FERRITIN_USER_AGENT: &str = concat!("ferritin/", env!("CARGO_PKG_VERSION"));

#[derive(Deserialize)]
struct CratesIoResponse {
    #[serde(rename = "crate")]
    krate: CrateMetadata,
    versions: Vec<CrateVersion>,
}

#[derive(Deserialize, Debug)]
struct CrateMetadata {
    pub(super) name: String,
    pub(super) default_version: Version,
    pub(super) description: String,
}

#[derive(Deserialize, Debug)]
struct CrateVersion {
    pub(super) num: Version,
}

/// Minimum supported format version (inclusive)
const MIN_FORMAT_VERSION: u32 = 55;

/// How long a cached crates.io version lookup stays fresh. crates.io sends no
/// cache headers on its API, and the fact we extract — a crate's version list —
/// only changes when a release is published, so a generous TTL is safe and stops
/// us from re-hitting the API on every CLI invocation.
const VERSION_CACHE_TTL_SECS: u64 = 30 * 60;

/// Cached projection of a crates.io metadata lookup — only what version
/// resolution needs (the version list, the default/latest, the description), not
/// the full API body. Persisted per-crate on disk so sequential CLI invocations,
/// each its own short-lived process, share one lookup instead of each re-hitting
/// crates.io (which would otherwise be one uncached request per invocation).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedMetadata {
    name: String,
    default_version: Version,
    description: String,
    /// The full version list, populated only once a range lookup has needed it.
    /// `None` means only the default (latest) version is known so far — a bare
    /// `crate` lookup never fetches the whole list.
    versions: Option<Vec<Version>>,
    /// Unix seconds at fetch time, for TTL freshness.
    fetched_at: u64,
}

impl CachedMetadata {
    fn is_fresh(&self) -> bool {
        now_unix().saturating_sub(self.fetched_at) < VERSION_CACHE_TTL_SECS
    }

    /// Whether this entry can answer a lookup: fresh, and — when the caller needs
    /// the full version list — actually carrying it.
    fn satisfies(&self, need_versions: bool) -> bool {
        self.is_fresh() && (!need_versions || self.versions.is_some())
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Client for fetching rustdoc JSON from docs.rs
#[derive(Debug, Fieldwork)]
pub struct DocsRsClient {
    http_client: Client,
    #[field(get)]
    cache_dir: PathBuf,
}

#[derive(Debug)]
pub(super) struct ResolvedMetadata {
    pub(super) name: String,
    pub(super) version: Version,
    pub(super) description: String,
}

impl DocsRsClient {
    fn user_agent() -> HeaderValue {
        use std::sync::LazyLock;
        static USER_AGENT: LazyLock<HeaderValue> = LazyLock::new(|| {
            let s: &'static str =
                format!("{FERRITIN_USER_AGENT} {}", trillium_client::USER_AGENT).leak();
            s.into()
        });
        USER_AGENT.clone()
    }

    /// Create a new docs.rs client with the specified cache directory
    pub fn new(cache_dir: PathBuf) -> Result<Self> {
        let http_client = Client::new(RustlsConfig::<ClientConfig>::default())
            .with_handler((
                ClientLogger::new().with_target(Target::Logger(log::Level::Info)),
                Compression::new(),
                FollowRedirects::new(),
                RetryHandler::new(),
            ))
            // A ceiling against hung connections, not an expected duration —
            // it spans the whole conn including the response body, and a JSON
            // download (redirect to static.docs.rs + a multi-MB body) needs
            // far more than a metadata lookup. 2s proved too tight for real
            // downloads on the public server's first cold fetches.
            .with_timeout(Duration::from_secs(30))
            .with_default_header(KnownHeaderName::UserAgent, Self::user_agent());

        Ok(Self {
            http_client,
            cache_dir,
        })
    }

    pub(super) async fn resolve(
        &self,
        crate_name: &str,
        version_req: &VersionReq,
    ) -> Result<Option<ResolvedMetadata>> {
        let Some(CachedMetadata {
            name,
            default_version,
            description,
            versions,
            ..
        }) = self
            .cached_metadata(crate_name, version_req != &VersionReq::STAR)
            .await?
        else {
            return Ok(None);
        };

        // Resolve "latest" to a specific version. The default (latest) satisfies
        // any request that matches it, so only a request that excludes the latest
        // needs the full list — which is exactly when `cached_metadata` fetched it.
        let version = if version_req.matches(&default_version) {
            Some(default_version)
        } else {
            versions
                .into_iter()
                .flatten()
                .filter(|version| version_req.matches(version))
                .max()
        };

        Ok(version.map(|version| ResolvedMetadata {
            name,
            version,
            description,
        }))
    }

    /// Fetch rustdoc JSON for a crate, checking cache first
    ///
    /// Returns:
    /// - Ok(Some(data)) if the crate was found (cached or fetched)
    /// - Ok(None) if docs.rs doesn't have this crate/version
    /// - Err(e) for network errors, parse errors, etc.
    pub async fn get_crate(
        &self,
        crate_name: &str,
        version: &Version,
    ) -> Result<Option<RustdocData>> {
        log::debug!("DocsRsClient::get_crate('{}', {:?})", crate_name, version);

        // Check cache first (now that we have a specific version)
        if let Some(cached) = self.load_from_cache(crate_name, version).await? {
            return Ok(Some(cached));
        }

        // Fetch from docs.rs. The suffix-less URL serves whatever format the
        // release was built with; we read the actual `format_version` from the
        // JSON below and let `load_and_normalize` parse it (formats newer than
        // the rustdoc-types we build against are additive, so they deserialize
        // directly). This replaces a historical probe of exact-format URLs in
        // descending order — one request instead of up to seven.
        let url = format!("https://docs.rs/crate/{crate_name}/{version}/json");
        let Some(bytes) = self.fetch_bytes(url).await? else {
            return Ok(None);
        };

        // Decompress
        let json = self.decompress_zstd(&bytes)?;

        // Extract metadata from JSON before normalizing — via targeted byte
        // scans, not a serde parse of the whole document (skipping the deep
        // `index` recurses per nesting layer; typenum overflows the stack).
        let format_version = crate::conversions::peek_format_version(&json)
            .context("Failed to read format_version from rustdoc JSON")?;
        let crate_version = crate::conversions::peek_crate_version(&json);

        // A build older than the conversions floor is a definitive absence
        // ("no rustdoc JSON we can read exists for this release"), not a parse
        // error — the same outcome the old exact-format probe produced by
        // finding none of its URLs.
        if format_version < MIN_FORMAT_VERSION {
            log::info!(
                "{crate_name}@{version} only has format {format_version} (< {MIN_FORMAT_VERSION}); treating as unavailable"
            );
            return Ok(None);
        }

        let Some(crate_version) = crate_version else {
            return Ok(None);
        };

        log::info!("Fetched crate {crate_name}@{crate_version}, format version {format_version}");

        // Save raw JSON to cache (indexed by source format version)
        let fs_path = self
            .save_to_cache(crate_name, &crate_version, format_version, &json)
            .await?;

        // Normalize to current format version
        let crate_data = crate::conversions::load_and_normalize(&json, Some(format_version))
            .context("Failed to normalize rustdoc JSON")?;

        // Build RustdocData (also writes the rkyv sidecar for next time)
        let data = RustdocData::from_crate(
            crate_data,
            crate_name.to_string(),
            CrateProvenance::DocsRs,
            fs_path,
            Some(crate_version),
        );

        Ok(Some(data))
    }

    /// Resolve crates.io metadata through the per-crate on-disk version cache
    /// (the tier that survives across CLI processes) before touching the
    /// network. In-process caching lives above this client, in the Store's
    /// resolution cache. `need_versions` requires the full version list, not
    /// just the latest — a fresh entry that only knows the latest can't
    /// answer a range lookup, so it falls through.
    ///
    /// Returns `Ok(None)` if crates.io doesn't have the crate.
    async fn cached_metadata(
        &self,
        crate_name: &str,
        need_versions: bool,
    ) -> Result<Option<CachedMetadata>> {
        if let Some(entry) = self.disk_lookup(crate_name).await?
            && entry.satisfies(need_versions)
        {
            return Ok(Some(entry));
        }

        let Some(entry) = self.fetch_metadata(crate_name, need_versions).await? else {
            return Ok(None);
        };
        self.disk_store(crate_name, &entry).await?;
        Ok(Some(entry))
    }

    /// On-disk location of a crate's cached version metadata.
    fn version_cache_path(&self, crate_name: &str) -> PathBuf {
        self.cache_dir
            .join("crates-io-versions")
            .join(format!("{crate_name}.json"))
    }

    /// Read a crate's version metadata from the on-disk tier. A missing file is a
    /// plain miss; a corrupt or partial file is treated as a miss too, never a
    /// hard error — the network tier will refill it.
    async fn disk_lookup(&self, crate_name: &str) -> Result<Option<CachedMetadata>> {
        let path = self.version_cache_path(crate_name);
        match async_fs::read(&path).await {
            Ok(bytes) => Ok(sonic_rs::serde::from_slice(&bytes).ok()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e).context("Failed to read version cache"),
        }
    }

    /// Write a crate's version metadata to the on-disk tier via a temp file +
    /// atomic rename, so a concurrent CLI invocation can never observe a torn
    /// file. The temp name carries our PID to avoid colliding with a concurrent
    /// writer of the same crate.
    async fn disk_store(&self, crate_name: &str, entry: &CachedMetadata) -> Result<()> {
        let path = self.version_cache_path(crate_name);
        if let Some(parent) = path.parent() {
            async_fs::create_dir_all(parent)
                .await
                .context("Failed to create version cache directory")?;
        }

        let json = sonic_rs::to_string(entry).context("Failed to serialize version cache")?;
        let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
        async_fs::write(&tmp, json)
            .await
            .context("Failed to write version cache temp file")?;
        async_fs::rename(&tmp, &path)
            .await
            .context("Failed to commit version cache file")?;
        Ok(())
    }

    /// The network tier: fetch crate metadata from the crates.io API and project
    /// it to a [`CachedMetadata`]. Returns `Ok(None)` on a 404.
    async fn fetch_metadata(
        &self,
        crate_name: &str,
        need_versions: bool,
    ) -> Result<Option<CachedMetadata>> {
        let include = if need_versions {
            "versions"
        } else {
            "default_version"
        };

        let url = format!("https://crates.io/api/v1/crates/{crate_name}?include={include}");

        log::debug!("Fetching crate metadata from crates.io: {url}");

        let conn = self.http_client.get(url).await?;

        // Check if we got a 404 (crate not found)
        if let Some(Status::NotFound) = conn.status() {
            return Ok(None);
        }

        let mut conn = conn
            .success()
            .map_err(|e| anyhow!("Failed to query crates.io: {}", e))?;

        // Read and parse response
        let bytes = conn
            .response_body()
            .read_bytes()
            .await
            .context("Failed to read crates.io response")?;

        let CratesIoResponse { krate, versions } =
            sonic_rs::serde::from_slice(&bytes).context("Failed to parse crates.io response")?;

        Ok(Some(CachedMetadata {
            name: krate.name,
            default_version: krate.default_version,
            description: krate.description,
            // Only trust the list as complete when we actually asked for it.
            versions: need_versions.then(|| versions.into_iter().map(|v| v.num).collect()),
            fetched_at: now_unix(),
        }))
    }

    /// Construct the cache file path for a crate
    ///
    /// Cache is organized by source format version (from docs.rs), not normalized version.
    /// This allows us to update normalization logic without re-fetching.
    fn cache_path(
        &self,
        crate_name: &str,
        version: &Version,
        source_format_version: u32,
    ) -> PathBuf {
        self.cache_dir
            .join(source_format_version.to_string())
            .join(crate_name)
            .join(format!("{version}.json"))
    }

    /// Format-version directories currently present in the cache, newest first.
    ///
    /// The cache is laid out as `cache_dir/{format_version}/...`, so the set of
    /// supported formats is whatever directories exist — including formats newer
    /// than [`FORMAT_VERSION`] that were fetched via the latest-format fallback.
    /// Directories older than [`MIN_FORMAT_VERSION`] (which we can't normalize)
    /// are skipped.
    fn cached_formats(&self) -> Vec<u32> {
        let mut formats: Vec<u32> = std::fs::read_dir(&self.cache_dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| entry.file_name().to_str()?.parse::<u32>().ok())
            .filter(|format| *format >= MIN_FORMAT_VERSION)
            .collect();
        formats.sort_unstable_by(|a, b| b.cmp(a));
        formats
    }

    /// Load from cache if available and valid
    ///
    /// Tries to find the crate in cache across different format versions.
    /// The cached JSON is normalized to the current format version on read.
    async fn load_from_cache(
        &self,
        crate_name: &str,
        version: &Version,
    ) -> Result<Option<RustdocData>> {
        // Try format versions in descending order (prefer newer versions).
        // We enumerate the format directories actually present rather than a
        // fixed range so that formats newer than the one we were built against
        // (fetched via the latest-format fallback) are still found on read.
        for source_format in self.cached_formats() {
            let path = self.cache_path(crate_name, version, source_format);

            if !path.exists() {
                continue;
            }

            log::info!(
                "Found cached file with format version {}: {}",
                source_format,
                path.display()
            );

            // Warm path: memory-map the rkyv sidecar instead of parsing JSON.
            if let Some(data) = RustdocData::try_from_sidecar(
                &path,
                crate_name.to_string(),
                CrateProvenance::LocalDependency,
                None,
            ) {
                return Ok(Some(data));
            }

            let start = Instant::now();
            let json = async_fs::read(&path)
                .await
                .context("Failed to read cached file")?;
            let read_elapsed = start.elapsed();
            log::debug!(
                "⏱️ Read {} ({:.2} MB) in {:?}",
                crate_name,
                json.len() as f64 / 1_000_000.0,
                read_elapsed
            );

            // Normalize to current format version
            let start = Instant::now();
            let crate_data = crate::conversions::load_and_normalize(&json, Some(source_format))
                .context("Failed to normalize cached JSON")?;
            let parse_elapsed = start.elapsed();
            log::debug!("⏱️ Parsed {} in {:?}", crate_name, parse_elapsed);

            let version = crate_data
                .crate_version
                .as_ref()
                .and_then(|v| Version::parse(v).ok());

            let data = RustdocData::from_crate(
                crate_data,
                crate_name.to_string(),
                CrateProvenance::LocalDependency,
                path,
                version,
            );

            return Ok(Some(data));
        }

        Ok(None)
    }

    /// GET a docs.rs JSON URL, returning the raw (zstd-compressed) body.
    ///
    /// Returns Ok(None) if the URL is a 404 (crate/version/format not found),
    /// Err for other failures.
    async fn fetch_bytes(&self, url: String) -> Result<Option<Vec<u8>>> {
        let conn = self.http_client.get(url).await?;

        // Check if we got a 404 (crate/version/format not found)
        if let Some(Status::NotFound) = conn.status() {
            return Ok(None);
        }

        // Check for success after following redirects
        let mut conn = conn
            .success()
            .map_err(|e| anyhow!("HTTP request failed: {}", e))?;

        // Read response body
        let bytes = conn
            .response_body()
            .read_bytes()
            .await
            .context("Failed to read response body")?;

        Ok(Some(bytes))
    }

    /// Decompress zstd-compressed data
    fn decompress_zstd(&self, compressed: &[u8]) -> Result<Vec<u8>> {
        zstd::decode_all(compressed).context("Failed to decompress zstd data")
    }

    /// Save decompressed JSON to cache
    ///
    /// Stores the raw JSON indexed by its source format version.
    async fn save_to_cache(
        &self,
        crate_name: &str,
        version: &Version,
        format_version: u32,
        json: &[u8],
    ) -> Result<PathBuf> {
        let path = self.cache_path(crate_name, version, format_version);

        // Create parent directories
        if let Some(parent) = path.parent() {
            async_fs::create_dir_all(parent)
                .await
                .context("Failed to create cache directory")?;
        }

        async_fs::write(&path, json)
            .await
            .context("Failed to write cache file")?;

        log::debug!(
            "Cached to {} (format version {})",
            path.display(),
            format_version
        );
        Ok(path)
    }
}
