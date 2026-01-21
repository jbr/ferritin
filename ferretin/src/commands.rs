use crate::renderer::HistoryEntry;
use crate::request::Request;
use crate::styled_string::Document;
use std::fmt::Display;

mod get;
pub(crate) mod list;
pub(crate) mod search;

#[derive(clap::Subcommand, Debug)]
pub(crate) enum Commands {
    /// Show documentation for an item
    Get {
        /// Path to the item (e.g., "std::vec::Vec" or "serde::Serialize")
        path: String,

        /// Show source code
        #[arg(short, long)]
        source: bool,

        /// Recursively show nested items
        #[arg(short, long)]
        recursive: bool,
    },

    /// Search for items by name or documentation
    Search {
        /// Search query
        query: String,

        /// Crate to search
        #[arg(short, long = "crate")]
        crate_: Option<String>,

        /// Maximum number of results
        #[arg(short, long, default_value = "10")]
        limit: usize,
    },

    /// List available crates
    List,
}

impl Commands {
    pub fn get(path: impl Display) -> Self {
        Self::Get {
            path: path.to_string(),
            source: false,
            recursive: false,
        }
    }

    pub fn search(query: impl Display) -> Self {
        Self::Search {
            query: query.to_string(),
            limit: 10,
            crate_: None,
        }
    }

    pub fn list() -> Self {
        Self::List
    }

    pub fn with_source(self) -> Self {
        match self {
            Self::Get {
                path, recursive, ..
            } => Self::Get {
                path,
                source: true,
                recursive,
            },
            other => other,
        }
    }

    pub fn in_crate(self, crate_: impl Display) -> Self {
        match self {
            Self::Search { query, limit, .. } => Self::Search {
                query,
                limit,
                crate_: Some(crate_.to_string()),
            },
            other => other,
        }
    }

    pub fn recursive(self) -> Self {
        match self {
            Self::Get { path, source, .. } => Self::Get {
                path,
                source,
                recursive: true,
            },
            other => other,
        }
    }

    pub fn with_limit(self, limit: usize) -> Self {
        match self {
            Self::Search { query, crate_, .. } => Self::Search {
                query,
                limit,
                crate_,
            },
            other => other,
        }
    }

    pub fn execute<'a>(
        self,
        request: &'a Request,
    ) -> (Document<'a>, bool, Option<HistoryEntry<'a>>) {
        match self {
            Commands::Get {
                path,
                source,
                recursive,
            } => {
                let (doc, is_error, item_ref) = get::execute(request, &path, source, recursive);
                let history_entry = item_ref.map(HistoryEntry::Item);
                (doc, is_error, history_entry)
            }
            Commands::Search {
                query,
                limit,
                crate_,
            } => {
                let (doc, is_error) = search::execute(request, &query, limit, crate_.as_deref());
                let history_entry = Some(HistoryEntry::Search {
                    query,
                    crate_name: crate_,
                });
                (doc, is_error, history_entry)
            }
            Commands::List => {
                let (doc, is_error) = list::execute(request);
                (doc, is_error, Some(HistoryEntry::List))
            }
        }
    }
}
