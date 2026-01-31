use ferritin_common::Navigator;
use std::ops::Deref;

use crate::format_context::FormatContext;

/// Wrapper around Navigator that adds formatting capabilities
pub(crate) struct Request {
    navigator: Navigator,
    format_context: FormatContext,
}

impl Deref for Request {
    type Target = Navigator;

    fn deref(&self) -> &Self::Target {
        &self.navigator
    }
}

impl Request {
    /// Create a new request with Navigator and formatting configuration
    pub(crate) fn new(navigator: Navigator, format_context: FormatContext) -> Self {
        Self {
            navigator,
            format_context,
        }
    }

    /// Get the formatting context
    pub(crate) fn format_context(&self) -> &FormatContext {
        &self.format_context
    }
}
