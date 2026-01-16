use crate::request::Request;
use crate::styled_string::{Document, DocumentNode, HeadingLevel, ListItem, Span};
use rustdoc_core::search::indexer::SearchIndex;

pub(crate) fn execute<'a>(request: &'a Request, query: &str, limit: usize) -> (Document<'a>, bool) {
    // Collect search results from all crates
    let mut all_results = vec![];

    for crate_info in request.project.crate_info(None) {
        let crate_name = crate_info.name();

        // Try to load/build the search index for this crate
        match SearchIndex::load_or_build(request, crate_name) {
            Ok(index) => {
                // Search and collect results with crate name
                let results = index.search(query);
                for (id_path, score) in results {
                    all_results.push((crate_name.to_string(), id_path.to_vec(), score));
                }
            }
            Err(_) => {
                // Silently skip crates that can't be indexed (e.g., not found)
                continue;
            }
        }
    }

    // Sort all results by score (descending)
    all_results.sort_by(|(_, _, score_a), (_, _, score_b)| {
        score_b
            .partial_cmp(score_a)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    if all_results.is_empty() {
        let error_doc = Document::from(vec![DocumentNode::Span(Span::plain(format!(
            "No results found for '{}'",
            query
        )))]);
        return (error_doc, true);
    }

    // Calculate total score for normalization
    let total_score: f32 = all_results.iter().map(|(_, _, score)| score).sum();
    let top_score = all_results
        .first()
        .map(|(_, _, score)| *score)
        .unwrap_or(0.0);

    let mut nodes = vec![
        DocumentNode::Heading {
            level: HeadingLevel::Title,
            spans: vec![
                Span::plain("Search results for '"),
                Span::plain(query.to_string()),
                Span::plain("'"),
            ],
        },
        DocumentNode::Span(Span::plain("\n")),
    ];

    // Display results with early stopping based on score thresholds
    let min_results = 1;
    let mut cumulative_score = 0.0;
    let mut prev_score = top_score;
    let mut list_items = vec![];

    for (i, (crate_name, id_path, score)) in all_results.into_iter().enumerate() {
        // Early stopping: stop if we've shown enough results and scores are dropping significantly
        if i >= min_results && i >= limit {
            break;
        }

        if i >= min_results
            && (score / top_score < 0.05
                || score / prev_score < 0.5
                || cumulative_score / total_score > 0.3)
        {
            break;
        }

        if let Some((item, path_segments)) = request.get_item_from_id_path(&crate_name, &id_path) {
            cumulative_score += score;
            prev_score = score;

            let path = path_segments.join("::");
            let normalized_score = 100.0 * score / total_score;

            let mut item_nodes = vec![DocumentNode::Span(Span::plain(format!(
                " ({:?}) - score: {:.0}",
                item.kind(),
                normalized_score
            )))];

            // Show first few lines of docs if available
            if let Some(docs) = &item.docs {
                let doc_preview: Vec<_> = docs.lines().take(2).collect();
                if !doc_preview.is_empty() {
                    for line in doc_preview {
                        if !line.trim().is_empty() {
                            item_nodes.push(DocumentNode::Span(Span::plain("\n    ".to_string())));
                            item_nodes.push(DocumentNode::Span(Span::plain(line.to_string())));
                        }
                    }
                }
            }

            list_items.push(ListItem::labeled(vec![Span::plain(path)], item_nodes));
        }
    }

    nodes.push(DocumentNode::List { items: list_items });

    (Document::from(nodes), false)
}
