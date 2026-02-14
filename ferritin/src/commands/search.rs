use crate::document::{Document, DocumentNode, HeadingLevel, ListItem, Span, TruncationLevel};
use crate::renderer::HistoryEntry;
use crate::request::Request;

pub(crate) fn execute<'a>(
    request: &'a Request,
    query: &str,
    limit: usize,
    crate_: Option<&str>,
) -> Document<'a> {
    log::info!("Searching for {query}");

    let crate_names: Vec<_> = match crate_ {
        Some(crate_) => vec![crate_],
        None => request
            .list_available_crates()
            .map(|ci| ci.name())
            .collect(),
    };

    // Search using Navigator's built-in search
    let scored_results = match request.search(query, &crate_names) {
        Ok(results) => results,
        Err(suggestions) => {
            // No crates could be loaded - show suggestions
            let mut nodes = vec![DocumentNode::paragraph(vec![Span::plain(format!(
                "No crates could be loaded for search."
            ))])];

            if !suggestions.is_empty() {
                nodes.push(DocumentNode::paragraph(vec![Span::plain(
                    "Did you mean one of these?",
                )]));

                let items: Vec<_> = suggestions
                    .into_iter()
                    .take(5)
                    .filter(|s| s.score() > 0.8)
                    .map(|s| {
                        let mut content = vec![DocumentNode::paragraph(vec![Span::plain(
                            s.path().to_string(),
                        )])];
                        if let Some(item) = s.item() {
                            content.push(DocumentNode::paragraph(vec![Span::plain(format!(
                                "({:?})",
                                item.kind()
                            ))]));
                        }
                        ListItem::new(content)
                    })
                    .collect();

                if !items.is_empty() {
                    nodes.push(DocumentNode::List { items });
                }
            }

            return Document::from(nodes).with_error();
        }
    };

    log::info!("Found {} matching items", scored_results.len());

    // Handle empty results
    if scored_results.is_empty() {
        if query.is_empty() {
            // Empty query - show search instructions
            return Document::from(vec![
                DocumentNode::Heading {
                    level: HeadingLevel::Title,
                    spans: vec![Span::plain("Search")],
                },
                DocumentNode::paragraph(vec![Span::plain(
                    "Type to search. Press Tab to toggle between current crate and all crates.",
                )]),
            ]);
        } else {
            // No matches for query
            return Document::from(vec![
                DocumentNode::Heading {
                    level: HeadingLevel::Title,
                    spans: vec![Span::plain("No results")],
                },
                DocumentNode::paragraph(vec![
                    Span::plain("No results found for '"),
                    Span::plain(query.to_string()),
                    Span::plain("'"),
                ]),
            ])
            .with_error();
        }
    }

    // Get top values for normalization (so best result = 100 in each metric)
    let top_score = scored_results
        .first()
        .map(|r| r.score)
        .unwrap_or(1.0)
        .max(1.0);

    let top_relevance = scored_results
        .iter()
        .map(|r| r.relevance)
        .fold(0.0f32, |a, b| a.max(b))
        .max(1.0);

    let top_authority = scored_results
        .iter()
        .map(|r| r.authority)
        .fold(0.0f32, |a, b| a.max(b))
        .max(0.01); // Avoid division by zero

    let mut nodes = vec![DocumentNode::Heading {
        level: HeadingLevel::Title,
        spans: vec![
            Span::plain("Search results for '"),
            Span::emphasis(query.to_string()),
            Span::plain("'"),
        ],
    }];

    // Display up to `limit` results
    let mut list_items = vec![];

    for (i, result) in scored_results.into_iter().enumerate() {
        if i >= limit {
            break;
        }

        if let Some((item, path_segments)) =
            request.get_item_from_id_path(result.crate_name, &result.id_path)
        {
            let path = path_segments.join("::");
            let normalized_score = 100.0 * result.score / top_score;
            let normalized_relevance = 100.0 * result.relevance / top_relevance;
            let normalized_authority = 100.0 * result.authority / top_authority;

            let mut content = vec![DocumentNode::paragraph(vec![
                Span::plain(path).with_target(Some(item)),
                Span::plain(" "),
                Span::plain(format!(
                    " ({:?}) - score: {:.0} (relevance: {:.0}, authority: {:.0})",
                    item.kind(),
                    normalized_score,
                    normalized_relevance,
                    normalized_authority
                )),
            ])];

            if let Some(docs) = request.docs_to_show(item, TruncationLevel::SingleLine) {
                content.extend(docs);
            }

            list_items.push(ListItem::new(content));
        }
    }

    nodes.push(DocumentNode::List { items: list_items });

    Document::from(nodes).with_history_entry(HistoryEntry::Search {
        query: query.into(),
        crate_name: crate_.map(String::from),
    })
}
