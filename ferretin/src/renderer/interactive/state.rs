use crate::styled_string::Document;

/// Input mode for the interactive renderer
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InputMode {
    /// Normal browsing mode
    Normal,
    /// Go-to mode (g pressed) - navigate to an item by path
    GoTo,
    /// Search mode (s pressed) - search for items
    Search,
}

/// Entry in navigation history
#[derive(Debug, Clone, PartialEq)]
pub(super) enum HistoryEntry<'a> {
    /// Regular item navigation
    Item(ferretin_common::DocRef<'a, rustdoc_types::Item>),
    /// Search result page
    Search {
        query: String,
        crate_name: Option<String>,
    },
}

impl<'a> HistoryEntry<'a> {
    /// Get a display name for this history entry
    pub(super) fn display_name(&self) -> String {
        match self {
            HistoryEntry::Item(item) => item.name().unwrap_or("<unnamed>").to_string(),
            HistoryEntry::Search { query, crate_name } => {
                if let Some(crate_name) = crate_name {
                    format!("\"{}\" in {}", query, crate_name)
                } else {
                    format!("\"{}\"", query)
                }
            }
        }
    }

    /// Get the crate name if this is an item entry
    pub(super) fn crate_name(&self) -> Option<&str> {
        match self {
            HistoryEntry::Item(item) => Some(item.crate_docs().name()),
            HistoryEntry::Search { crate_name, .. } => crate_name.as_deref(),
        }
    }

    /// Render this history entry to a document
    pub(super) fn render(&self, request: &'a crate::request::Request) -> Document<'a> {
        match self {
            HistoryEntry::Item(item) => {
                let doc_nodes = request.format_item(*item);
                Document::from(doc_nodes)
            }
            HistoryEntry::Search { query, crate_name } => {
                let (search_doc, _is_error) = crate::commands::search::execute(
                    request,
                    query,
                    20, // limit
                    crate_name.as_deref(),
                );
                search_doc
            }
        }
    }
}
