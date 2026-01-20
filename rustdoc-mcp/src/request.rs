use ferretin_common::{Navigator, RustdocProject};
use std::ops::Deref;
use std::path::PathBuf;

/// MCP-specific wrapper around Navigator that adds formatting capabilities
pub(crate) struct Request {
    pub(crate) project: RustdocProject,
    navigator: Navigator,
}

impl Deref for Request {
    type Target = Navigator;

    fn deref(&self) -> &Self::Target {
        &self.navigator
    }
}

impl Request {
    /// Create a new request for a manifest path
    pub(crate) fn new(manifest_path: PathBuf) -> Self {
        // Load the project (still needed for some MCP-specific operations)
        let project =
            RustdocProject::load(manifest_path.clone()).expect("Failed to load RustdocProject");

        // Build Navigator with all sources (local will be loaded lazily)
        let navigator = Navigator::builder()
            .with_std_source_if_available()
            .with_local_context(manifest_path, true) // can rebuild with nightly
            .with_docsrs_source_if_available()
            .build()
            .expect("Failed to build Navigator");

        Self { project, navigator }
    }
}
