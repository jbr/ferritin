use ferritin_common::DocRef;
use rustdoc_types::Item;

use super::channels::UiCommand;
use std::borrow::Cow;

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
pub enum HistoryEntry<'a> {
    /// Regular item navigation
    Item(DocRef<'a, Item>),
    /// Search result page
    Search {
        query: String,
        crate_name: Option<String>,
    },
    /// List crates page
    List,
}

impl<'a> HistoryEntry<'a> {
    pub(super) fn item(&self) -> Option<DocRef<'a, Item>> {
        if let Self::Item(item) = self {
            Some(*item)
        } else {
            None
        }
    }

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
            HistoryEntry::List => "List".to_string(),
        }
    }

    /// Get the crate name if this is an item entry
    pub(super) fn crate_name(&self) -> Option<&str> {
        match self {
            HistoryEntry::Item(item) => Some(item.crate_docs().name()),
            HistoryEntry::Search { crate_name, .. } => crate_name.as_deref(),
            HistoryEntry::List => None,
        }
    }

    /// Convert this history entry to a command that can be sent to the request thread
    pub(super) fn to_command(&self) -> UiCommand<'a> {
        match self {
            HistoryEntry::Item(item) => UiCommand::Navigate(*item),
            HistoryEntry::Search { query, crate_name } => UiCommand::Search {
                query: Cow::Owned(query.clone()),
                crate_name: crate_name.as_ref().map(|c| Cow::Owned(c.clone())),
                limit: 20,
            },
            HistoryEntry::List => UiCommand::List,
        }
    }
}
