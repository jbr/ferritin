use super::SearchTarget;
use crate::{
    request::Request,
    styled_string::{Document, DocumentNode, HeadingLevel, ListItem, Span, TruncationLevel},
};
use ferritin_common::{DocRef, search::QueryCompletion};
use rustdoc_types::Item;

/// Structural model of a search outcome. The terminal path lowers it back to a
/// `Document` ([`lower_search`]); the JSON path serializes it directly.
pub(crate) enum SearchDoc<'a> {
    /// Matches found for a non-empty query.
    Results {
        query: String,
        results: Vec<SearchResult<'a>>,
    },
    /// A non-empty query matched nothing.
    NoResults { query: String },
    /// The query was empty (the TUI shows search instructions).
    EmptyQuery,
    /// No crates could be loaded to search; carries fuzzy crate suggestions.
    NoCrates {
        suggestions: Vec<SearchSuggestion<'a>>,
    },
}

/// A single search hit: its qualified path, the resolved item (kind + nav URL),
/// the normalized score (best result = 100), and brief docs.
pub(crate) struct SearchResult<'a> {
    pub(crate) path: String,
    pub(crate) item: DocRef<'a, Item>,
    pub(crate) score: f32,
    pub(crate) docs: Option<Vec<DocumentNode<'a>>>,
}

/// A "did you mean" crate suggestion for the no-crates-loaded case.
pub(crate) struct SearchSuggestion<'a> {
    pub(crate) path: String,
    pub(crate) item: Option<DocRef<'a, Item>>,
}

impl SearchDoc<'_> {
    /// Whether this outcome is an error (only the no-crates case).
    pub(crate) fn is_error(&self) -> bool {
        matches!(self, SearchDoc::NoCrates { .. })
    }
}

/// Split a parsed [`SearchTarget`] into an optional crate filter and the joined
/// query string. Shared by the terminal dispatch and the JSON path.
pub(crate) fn parse_target(target: SearchTarget) -> (Option<String>, String) {
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
    (crate_, query_parts.join(" "))
}

/// Resolve a search into its semantic [`SearchDoc`] model.
pub(crate) fn model<'a>(
    request: &mut Request<'a>,
    query: &str,
    limit: usize,
    crate_: Option<&str>,
    completion: QueryCompletion,
) -> SearchDoc<'a> {
    log::info!("Searching for {query}");

    let crate_names: Vec<_> = match crate_ {
        Some(crate_) => vec![crate_],
        None => request
            .navigator()
            .list_available_crates()
            .map(|ci| ci.name())
            .collect(),
    };

    let scored_results = match request.navigator().search(query, &crate_names, completion) {
        Ok(results) => results,
        Err(suggestions) => {
            let suggestions = suggestions
                .into_iter()
                .take(5)
                .filter(|s| s.score() > 0.8)
                .map(|s| SearchSuggestion {
                    path: s.path().to_string(),
                    item: s.item().copied(),
                })
                .collect();
            return SearchDoc::NoCrates { suggestions };
        }
    };

    log::info!("Found {} matching items", scored_results.len());

    if scored_results.is_empty() {
        return if query.is_empty() {
            SearchDoc::EmptyQuery
        } else {
            SearchDoc::NoResults {
                query: query.to_string(),
            }
        };
    }

    // Normalize so the best result = 100.
    let top_score = scored_results
        .first()
        .map(|r| r.score)
        .unwrap_or(1.0)
        .max(1.0);

    let mut results = vec![];
    for result in scored_results {
        if results.len() >= limit {
            break;
        }

        if let Some((item, path_segments)) =
            request.get_item_from_id_path(result.crate_name, &result.id_path)
        {
            // Skip results the active `--kind` filter excludes; only displayed
            // items count toward `limit`.
            if !request.format_context().should_display(item) {
                continue;
            }

            // Quantize to 0.1% buckets to match the sort's tiebreak precision
            // (see BM25Scorer::score) so ordering and display stay in sync.
            let score = (1000.0 * result.score / top_score).round() / 10.0;
            let docs = request.docs_to_show(item, TruncationLevel::SingleLine);

            results.push(SearchResult {
                path: path_segments.join("::"),
                item,
                score,
                docs,
            });
        }
    }

    SearchDoc::Results {
        query: query.to_string(),
        results,
    }
}

/// Run a search and lower it to a `Document` for the terminal renderers.
pub(crate) fn execute<'a>(
    request: &mut Request<'a>,
    query: &str,
    limit: usize,
    crate_: Option<&str>,
    completion: QueryCompletion,
) -> (Document<'a>, bool) {
    let doc = model(request, query, limit, crate_, completion);
    let is_error = doc.is_error();
    (lower_search(doc), is_error)
}

/// Lower a [`SearchDoc`] to its presentation `Document`, reproducing the old
/// `search::execute` output for each state.
pub(crate) fn lower_search(doc: SearchDoc<'_>) -> Document<'_> {
    match doc {
        SearchDoc::Results { query, results } => {
            let mut nodes = vec![DocumentNode::Heading {
                level: HeadingLevel::Title,
                spans: vec![
                    Span::plain("Search results for '"),
                    Span::emphasis(query),
                    Span::plain("'"),
                ],
            }];

            let list_items = results
                .into_iter()
                .map(|result| {
                    let mut content = vec![DocumentNode::paragraph(vec![
                        Span::plain(result.path).with_target(Some(result.item)),
                        Span::plain(" "),
                        Span::plain(format!(
                            " ({:?}) - score: {:.1}",
                            result.item.kind(),
                            result.score,
                        )),
                    ])];
                    if let Some(docs) = result.docs {
                        content.extend(docs);
                    }
                    ListItem::new(content)
                })
                .collect();

            nodes.push(DocumentNode::List { items: list_items });
            Document::from(nodes)
        }
        SearchDoc::NoResults { query } => Document::from(vec![
            DocumentNode::Heading {
                level: HeadingLevel::Title,
                spans: vec![Span::plain("No results")],
            },
            DocumentNode::paragraph(vec![
                Span::plain("No results found for '"),
                Span::plain(query),
                Span::plain("'"),
            ]),
        ]),
        SearchDoc::EmptyQuery => Document::from(vec![
            DocumentNode::Heading {
                level: HeadingLevel::Title,
                spans: vec![Span::plain("Search")],
            },
            DocumentNode::paragraph(vec![Span::plain(
                "Type to search. Press Tab to toggle between current crate and all crates.",
            )]),
        ]),
        SearchDoc::NoCrates { suggestions } => {
            let mut nodes = vec![DocumentNode::paragraph(vec![Span::plain(
                "No crates could be loaded for search.",
            )])];

            if !suggestions.is_empty() {
                nodes.push(DocumentNode::paragraph(vec![Span::plain(
                    "Did you mean one of these?",
                )]));

                let items: Vec<_> = suggestions
                    .into_iter()
                    .map(|s| {
                        let mut content = vec![DocumentNode::paragraph(vec![Span::plain(s.path)])];
                        if let Some(item) = s.item {
                            content.push(DocumentNode::paragraph(vec![Span::plain(format!(
                                "({:?})",
                                item.kind()
                            ))]));
                        }
                        ListItem::new(content)
                    })
                    .collect();

                nodes.push(DocumentNode::List { items });
            }

            Document::from(nodes)
        }
    }
}
