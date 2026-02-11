use crate::request::Request;
use crate::styled_string::{Document, DocumentNode, HeadingLevel, ListItem, Span, TruncationLevel};

pub(crate) fn execute<'a>(
    request: &'a Request,
    query: &str,
    limit: usize,
    crate_: Option<&str>,
) -> (Document<'a>, bool) {
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

            return (Document::from(nodes), true);
        }
    };

    log::info!("Found {} matching items", scored_results.len());

    // Handle empty results
    if scored_results.is_empty() {
        if query.is_empty() {
            // Empty query - show search instructions
            let doc = Document::from(vec![
                DocumentNode::Heading {
                    level: HeadingLevel::Title,
                    spans: vec![Span::plain("Search")],
                },
                DocumentNode::paragraph(vec![Span::plain(
                    "Type to search. Press Tab to toggle between current crate and all crates.",
                )]),
            ]);
            return (doc, false);
        } else {
            // No matches for query
            let error_doc = Document::from(vec![
                DocumentNode::Heading {
                    level: HeadingLevel::Title,
                    spans: vec![Span::plain("No results")],
                },
                DocumentNode::paragraph(vec![
                    Span::plain("No results found for '"),
                    Span::plain(query.to_string()),
                    Span::plain("'"),
                ]),
            ]);
            return (error_doc, false);
        }
    }

    // Get top score for normalization (so best result = 100)
    let top_score = scored_results
        .first()
        .map(|r| r.score)
        .unwrap_or(1.0)
        .max(1.0);

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

            let mut content = vec![DocumentNode::paragraph(vec![
                Span::plain(path).with_target(Some(item)),
                Span::plain(" "),
                Span::plain(format!(
                    " ({:?}) - score: {:.1}",
                    item.kind(),
                    normalized_score
                )),
            ])];

            if let Some(docs) = request.docs_to_show(item, TruncationLevel::SingleLine) {
                content.extend(docs);
            }

            list_items.push(ListItem::new(content));
        }
    }

    nodes.push(DocumentNode::List { items: list_items });

    (Document::from(nodes), false)
}
