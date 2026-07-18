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

    /// Create rooted at the default cache location ([`ferritin_home`]),
    /// migrating a legacy `$CARGO_HOME/rustdoc-json` cache into it if one
    /// exists. Explicit [`new`](Self::new) callers (tests, custom roots) never
    /// trigger the migration.
    ///
    /// [`ferritin_home`]: crate::ferritin_home
    pub fn from_default_cache() -> Option<Self> {
        let root = crate::ferritin_home::resolve()?;
        crate::ferritin_home::migrate_legacy_cache(&root);
        DocsRsClient::new(root).ok().map(|client| Self { client })
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
    fn lookup<'a>(
        &'a self,
        name: &str,
        version_req: &VersionReq,
    ) -> Result<Option<Cow<'a, CrateInfo>>> {
        let Some(ResolvedMetadata {
            name,
            version,
            description,
        }) = block_on(self.client.resolve(name, version_req))?
        else {
            return Ok(None);
        };

        Ok(Some(Cow::Owned(CrateInfo {
            provenance: CrateProvenance::DocsRs,
            version,
            description,
            name,
            default_crate: false,
            used_by: vec![],
            json_path: None,
        })))
    }

    fn load(&self, crate_name: &str, version: &Version) -> Result<Option<RustdocData>> {
        block_on(self.load_async(crate_name, version))
    }
}
