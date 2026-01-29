use crate::sources::CrateProvenance;
use crate::{RustdocData, sources::RustdocVersion};
use anyhow::{Context, Result, anyhow};
use rustdoc_types::FORMAT_VERSION;
use semver::{Version, VersionReq};
use serde::Deserialize;
use trillium_client::{Client, Status};
use trillium_rustls::RustlsConfig;
use trillium_smol::ClientConfig;

use std::path::PathBuf;

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

/// Client for fetching rustdoc JSON from docs.rs
#[derive(Debug)]
pub struct DocsRsClient {
    http_client: Client,
    cache_dir: PathBuf,
    format_version: u32,
}

#[derive(Debug)]
pub(super) struct ResolvedMetadata {
    pub(super) name: String,
    pub(super) version: Version,
    pub(super) description: String,
}

impl DocsRsClient {
    /// Create a new docs.rs client with the specified cache directory
    pub fn new(cache_dir: PathBuf) -> Result<Self> {
        let http_client = Client::new(RustlsConfig::<ClientConfig>::default()).with_default_pool();

        Ok(Self {
            http_client,
            cache_dir,
            format_version: FORMAT_VERSION,
        })
    }

    pub(super) async fn resolve(
        &self,
        crate_name: &str,
        version_req: &VersionReq,
    ) -> Result<Option<ResolvedMetadata>> {
        let Some((
            CrateMetadata {
                name,
                default_version,
                description,
            },
            versions,
        )) = self
            .metadata(crate_name, version_req != &VersionReq::STAR)
            .await?
        else {
            return Ok(None);
        };

        // Resolve "latest" to a specific version using crates.io API
        let version = if version_req.matches(&default_version) {
            Some(default_version)
        } else {
            versions
                .into_iter()
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

        // Fetch from docs.rs
        // Try format versions in descending order (newest we support first)
        let mut bytes = None;
        for format_ver in (MIN_FORMAT_VERSION..=self.format_version).rev() {
            log::debug!(
                "Trying to fetch {} version {} with format {}",
                crate_name,
                version,
                format_ver
            );

            if let Some(fetched) = self
                .fetch_from_docsrs(crate_name, version, format_ver)
                .await?
            {
                bytes = Some(fetched);
                break;
            }
        }

        let Some(bytes) = bytes else {
            return Ok(None);
        };

        // Decompress
        let json = self.decompress_zstd(&bytes)?;

        // Extract metadata from JSON before normalizing
        let RustdocVersion {
            format_version,
            crate_version,
        }: super::super::RustdocVersion =
            serde_json::from_slice(&json).context("Failed to parse JSON metadata")?;

        log::debug!(
            "Fetched crate {} version {:?} with format version {}",
            crate_name,
            crate_version,
            format_version
        );

        let Some(crate_version) = crate_version else {
            return Ok(None);
        };

        // Save raw JSON to cache (indexed by source format version)
        let fs_path = self
            .save_to_cache(crate_name, &crate_version, format_version, &json)
            .await?;

        // Normalize to current format version
        let crate_data = crate::conversions::load_and_normalize(&json)
            .context("Failed to normalize rustdoc JSON")?;

        // Build RustdocData
        let data = RustdocData {
            crate_data,
            name: crate_name.to_string(),
            provenance: CrateProvenance::DocsRs,
            fs_path,
            version: Some(crate_version),
        };

        Ok(Some(data))
    }

    /// Resolve "latest" to a specific version using the crates.io API
    /// Returns Ok(None) if the crate is not found
    async fn metadata(
        &self,
        crate_name: &str,
        include_versions: bool,
    ) -> Result<Option<(CrateMetadata, Vec<Version>)>> {
        let include = if include_versions {
            "versions"
        } else {
            "default_version"
        };

        let url = format!("https://crates.io/api/v1/crates/{crate_name}?include={include}");

        log::debug!("Resolving latest version from crates.io: {}", &url);

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
            serde_json::from_slice(&bytes).context("Failed to parse crates.io response")?;

        Ok(Some((krate, versions.into_iter().map(|v| v.num).collect())))
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

    /// Load from cache if available and valid
    ///
    /// Tries to find the crate in cache across different format versions.
    /// The cached JSON is normalized to the current format version on read.
    async fn load_from_cache(
        &self,
        crate_name: &str,
        version: &Version,
    ) -> Result<Option<RustdocData>> {
        // Try format versions in descending order (prefer newer versions)
        for source_format in (MIN_FORMAT_VERSION..=self.format_version).rev() {
            let path = self.cache_path(crate_name, version, source_format);

            if !path.exists() {
                continue;
            }

            log::debug!(
                "Found cached file with format version {}: {}",
                source_format,
                path.display()
            );

            let start = std::time::Instant::now();
            let json = async_fs::read(&path)
                .await
                .context("Failed to read cached file")?;
            let read_elapsed = start.elapsed();
            log::info!(
                "⏱️ Read {} ({:.2} MB) in {:?}",
                crate_name,
                json.len() as f64 / 1_000_000.0,
                read_elapsed
            );

            // Normalize to current format version
            let start = std::time::Instant::now();
            let crate_data = crate::conversions::load_and_normalize(&json)
                .context("Failed to normalize cached JSON")?;
            let parse_elapsed = start.elapsed();
            log::info!("⏱️ Parsed {} in {:?}", crate_name, parse_elapsed);

            let version = crate_data
                .crate_version
                .as_ref()
                .and_then(|v| Version::parse(v).ok());

            let data = RustdocData {
                crate_data,
                name: crate_name.to_string(),
                provenance: CrateProvenance::LocalDependency,
                fs_path: path,
                version,
            };

            return Ok(Some(data));
        }

        Ok(None)
    }

    /// Fetch from docs.rs
    /// Returns Ok(None) if the crate/version is not found (404)
    /// Returns Err for other errors
    async fn fetch_from_docsrs(
        &self,
        crate_name: &str,
        version: &Version,
        format_version: u32,
    ) -> Result<Option<Vec<u8>>> {
        // Construct URL with format version to ensure compatibility
        // https://docs.rs/crate/{crate_name}/{version}/json/{format_version}
        // (zstd compression is default)
        let url = format!("https://docs.rs/crate/{crate_name}/{version}/json/{format_version}");

        log::debug!("Fetching from docs.rs: {}", url);

        let mut conn = self.http_client.get(url).await?;

        // Check if we got a 404 (crate/version not found)
        if let Some(Status::NotFound) = conn.status() {
            return Ok(None);
        }

        // Handle redirects (docs.rs redirects to resolved version)
        if let Some(status) = conn.status()
            && status.is_redirection()
            && let Some(location) = conn.response_headers().get("location")
        {
            let location_str = location.to_string();
            // Location might be relative, construct full URL
            let redirect_url = if location_str.starts_with("http") {
                location_str
            } else {
                format!("https://docs.rs{}", location_str)
            };
            log::debug!("Following redirect to: {}", redirect_url);
            conn = self.http_client.get(redirect_url).await?;
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
