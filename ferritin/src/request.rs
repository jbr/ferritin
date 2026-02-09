use ferritin_common::{
    Navigator,
    sources::{DocsRsSource, LocalSource, StdSource},
};
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::OnceLock;

use crate::format_context::FormatContext;

/// Wrapper around Navigator that adds formatting capabilities
pub(crate) struct Request {
    inner: OnceLock<Navigator>,
    manifest_path: PathBuf,
    format_context: FormatContext,
}

impl Deref for Request {
    type Target = Navigator;

    fn deref(&self) -> &Self::Target {
        self.inner
            .get()
            .expect("Request::populate() must be called before use")
    }
}

impl Request {
    /// Create a new request with Navigator and formatting configuration
    pub(crate) fn new(navigator: Navigator, format_context: FormatContext) -> Self {
        Self {
            inner: OnceLock::from(navigator),
            manifest_path: PathBuf::new(), // Not used in eager mode
            format_context,
        }
    }

    /// Create a lazy request that defers Navigator construction until populate() is called
    pub(crate) fn lazy(manifest_path: PathBuf, format_context: FormatContext) -> Self {
        Self {
            inner: OnceLock::new(),
            manifest_path,
            format_context,
        }
    }

    /// Populate the Navigator with sources (if not already populated)
    /// This is the slow operation that loads all documentation sources
    pub(crate) fn populate(&self) {
        let manifest_path = &self.manifest_path;
        self.inner.get_or_init(|| {
            log::info!("Checking for std documentation from rustup");
            let std_source = StdSource::from_rustup();
            if let Some(std_source) = &std_source {
                log::info!(
                    "Found std docs for {} at {}",
                    std_source.rustc_version(),
                    std_source.docs_path().display()
                );
            }

            log::info!(
                "Looking for a cargo workspace from {}",
                manifest_path.display()
            );
            let local_source = LocalSource::load(manifest_path).ok();
            if let Some(local_source) = &local_source {
                log::info!(
                    "Found cargo workspace at {}",
                    local_source.manifest_path().display()
                );
            }
            log::info!("Building a docs.rs client");
            let docsrs_source = DocsRsSource::from_default_cache();
            if let Some(docsrs_source) = &docsrs_source {
                log::info!(
                    "Built new docs.rs client with cache at {}",
                    docsrs_source.client().cache_dir().display()
                );
            }

            Navigator::default()
                .with_std_source(std_source)
                .with_local_source(local_source)
                .with_docsrs_source(docsrs_source)
        });
    }

    /// Get the formatting context
    pub(crate) fn format_context(&self) -> &FormatContext {
        &self.format_context
    }
}
