use ferretin_common::{Navigator, RustdocProject};
use std::cell::{Ref, RefCell};
use std::ops::Deref;

use crate::format_context::FormatContext;

/// wrapper around Navigator that adds formatting capabilities
pub(crate) struct Request {
    pub(crate) project: RustdocProject,
    navigator: Navigator,
    format_context: RefCell<FormatContext>,
}

impl Deref for Request {
    type Target = Navigator;

    fn deref(&self) -> &Self::Target {
        &self.navigator
    }
}

impl Request {
    /// Create a new request for a project
    pub(crate) fn new(project: RustdocProject, format_context: FormatContext) -> Self {
        let manifest_path = project.manifest_path().to_path_buf();

        // Build Navigator with all sources (local will be loaded lazily)
        let navigator = Navigator::builder()
            .with_std_source_if_available()
            .with_local_context(manifest_path, true) // can rebuild with nightly
            .with_docsrs_source_if_available()
            .build()
            .expect("Failed to build Navigator");

        Self {
            project,
            navigator,
            format_context: RefCell::new(format_context),
        }
    }

    pub(crate) fn mutate_format_context(&self, f: impl FnOnce(&mut FormatContext)) {
        let mut b = self.format_context.borrow_mut();
        f(&mut b);
    }

    pub(crate) fn format_context(&self) -> Ref<'_, FormatContext> {
        self.format_context.borrow()
    }
}
