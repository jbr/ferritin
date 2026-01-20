use rustdoc_core::{Navigator, RustdocProject};
use std::ops::Deref;
use std::rc::Rc;

/// MCP-specific wrapper around Navigator that adds formatting capabilities
pub(crate) struct Request {
    navigator: Navigator,
}

impl Deref for Request {
    type Target = Navigator;

    fn deref(&self) -> &Self::Target {
        &self.navigator
    }
}

impl Request {
    /// Create a new request for a project
    pub(crate) fn new(project: Rc<RustdocProject>) -> Self {
        Self {
            navigator: Navigator::new(project),
        }
    }
}
