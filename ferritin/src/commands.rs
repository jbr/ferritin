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

    /// Search for items by name or documentation.
    ///
    /// Use `ferritin search <CRATE> <QUERY>...` to search a single crate
    /// (e.g. `ferritin search serde Vec`, `ferritin search serde@1.0 Vec`),
    /// or `ferritin search all <QUERY>...` to search every available crate.
    Search {
        /// Maximum number of results
        #[arg(long, default_value = "10")]
        limit: usize,

        #[command(subcommand)]
        target: SearchTarget,
    },

    /// List available crates
    List,
}

/// What to search: a single crate, or every available crate.
///
/// `All` is a literal subcommand (`ferritin search all foo bar`).
/// `Crate` uses clap's external-subcommand fallback: any other first word
/// becomes the crate name and the remaining words are the query
/// (`ferritin search serde Vec into iter`).
#[derive(clap::Subcommand, Debug)]
pub(crate) enum SearchTarget {
    /// Search across every available crate
    All {
        /// Search query (multiple words are joined with spaces; use `--`
        /// before queries that begin with a hyphen)
        #[arg(trailing_var_arg = true, required = true)]
        query: Vec<String>,
    },

    /// Search a single crate. The first word is the crate name (optionally
    /// `name@version`); remaining words are the query.
    #[command(external_subcommand)]
    Crate(Vec<String>),
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
            limit: 10,
            target: SearchTarget::All {
                query: vec![query.to_string()],
            },
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
            Self::Search { limit, target } => {
                let query = match target {
                    SearchTarget::All { query } => query,
                    SearchTarget::Crate(parts) => parts.into_iter().skip(1).collect(),
                };
                let mut parts = Vec::with_capacity(query.len() + 1);
                parts.push(crate_.to_string());
                parts.extend(query);
                Self::Search {
                    limit,
                    target: SearchTarget::Crate(parts),
                }
            }
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
            Self::Search { target, .. } => Self::Search { limit, target },
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
            Commands::Search { limit, target } => {
                let (crate_, query_parts) = match target {
                    SearchTarget::All { query } => (None, query),
                    SearchTarget::Crate(mut parts) => {
                        if parts.is_empty() {
                            (None, Vec::new())
                        } else {
                            let crate_ = parts.remove(0);
                            (Some(crate_), parts)
                        }
                    }
                };
                let query = query_parts.join(" ");
                if query.trim().is_empty() {
                    let doc = Document::from(vec![crate::styled_string::DocumentNode::paragraph(
                        vec![crate::styled_string::Span::plain(
                            "search requires a query (e.g. `ferritin search serde Vec` or `ferritin search all Vec`)",
                        )],
                    )]);
                    return (doc, true, None);
                }
                let (doc, is_error) = search::execute(request, &query, limit, crate_.as_deref());
                let history_entry = Some(HistoryEntry::Search {
                    query,
                    crate_name: crate_,
                });
                (doc, is_error, history_entry)
            }
            Commands::List => {
                let (doc, is_error, default_crate) = list::execute(request);
                let history_entry = Some(HistoryEntry::List { default_crate });
                (doc, is_error, history_entry)
            }
        }
    }
}
