use crate::request::Request;
use crate::styled_string::Document;
use std::fmt::Display;

mod get;
mod list;
mod search;

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
            Self::Search { query, .. } => Self::Search { query, limit },
            other => other,
        }
    }

    pub fn execute<'a>(self, request: &'a Request) -> (Document<'a>, bool) {
        match self {
            Commands::Get {
                path,
                source,
                recursive,
            } => get::execute(request, &path, source, recursive),
            Commands::Search { query, limit } => search::execute(request, &query, limit),
            Commands::List => list::execute(request),
        }
    }
}
