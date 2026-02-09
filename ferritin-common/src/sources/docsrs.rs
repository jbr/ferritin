use super::{CrateProvenance, Source};
use crate::{CrateInfo, RustdocData};
use anyhow::Result;
use fieldwork::Fieldwork;
use semver::{Version, VersionReq};
use std::{borrow::Cow, path::PathBuf};
use trillium_smol::async_io::block_on;

mod client;
use client::{DocsRsClient, ResolvedMetadata};

/// Source for docs.rs documentation
#[derive(Debug, Fieldwork)]
pub struct DocsRsSource {
    #[field(get)]
    client: DocsRsClient,
}

impl DocsRsSource {
    /// Create a new DocsRsSource with a cache directory
    pub fn new(cache_dir: PathBuf) -> Result<Self> {
        let client = DocsRsClient::new(cache_dir)?;
        Ok(Self { client })
    }

    /// Try to create from default cache location
    pub fn from_default_cache() -> Option<Self> {
        let cache_dir = home::cargo_home().ok()?.join("rustdoc-json");
        DocsRsClient::new(cache_dir)
            .ok()
            .map(|client| Self { client })
    }

    /// Load a crate from docs.rs
    async fn load_async(&self, crate_name: &str, version: &Version) -> Result<Option<RustdocData>> {
        self.client.get_crate(crate_name, version).await
    }

    /// Docs.rs has unbounded crates, so we don't provide a list
    /// This method exists for API consistency but always returns None
    pub fn list_available_crates(&self) -> Option<std::iter::Empty<String>> {
        None
    }
}

impl Source for DocsRsSource {
    fn lookup<'a>(&'a self, name: &str, version_req: &VersionReq) -> Option<Cow<'a, CrateInfo>> {
        let ResolvedMetadata {
            name,
            version,
            description,
        } = block_on(self.client.resolve(name, version_req))
            .ok()
            .flatten()?;

        Some(Cow::Owned(CrateInfo {
            provenance: CrateProvenance::DocsRs,
            version: Some(version),
            description: Some(description),
            name,
            default_crate: false,
            used_by: vec![],
            json_path: None,
        }))
    }

    fn load(&self, crate_name: &str, version: Option<&Version>) -> Option<RustdocData> {
        block_on(self.load_async(crate_name, version?))
            .ok()
            .flatten()
    }
}
