//! Channel types for UI ↔ Request thread communication

use ferritin_common::DocRef;
use rustdoc_types::Item;

use super::history::HistoryEntry;
use crate::styled_string::Document;
use std::borrow::Cow;

/// Commands sent from UI thread to Request thread
#[derive(Debug)]
pub enum UiCommand<'a> {
    /// Navigate to an already-resolved item (e.g., from clicking a link)
    Navigate(DocRef<'a, Item>),

    /// Navigate to a path by string (e.g., "std::vec::Vec" from GoTo mode)
    NavigateToPath(Cow<'a, str>),

    /// Search for items
    Search {
        query: Cow<'a, str>,
        crate_name: Option<Cow<'a, str>>,
        limit: usize,
    },

    /// Show list of available crates
    List,

    /// Toggle source code display
    ToggleSource {
        include_source: bool,
        current_item: Option<DocRef<'a, Item>>,
    },

    /// Shutdown the request thread
    Shutdown,
}

/// Responses sent from Request thread to UI thread
pub enum RequestResponse<'a> {
    /// Successfully loaded a document with optional history entry
    Document {
        doc: Document<'a>,
        entry: Option<HistoryEntry<'a>>,
    },

    /// An error occurred (path not found, etc.)
    Error(String),

    /// Acknowledgment that shutdown is complete
    ShuttingDown,
}
