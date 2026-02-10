use crate::request::Request;
use crate::styled_string::{Document, DocumentNode, HeadingLevel, ListItem, Span, TruncationLevel};
use ferritin_common::search::indexer::{BM25Scorer, SearchIndex};
use rayon::prelude::*;

pub(crate) fn execute<'a>(
    request: &'a Request,
    query: &str,
    limit: usize,
    crate_: Option<&str>,
) -> (Document<'a>, bool) {
    log::info!("Searching for {query}");

    let crate_names = match crate_ {
        Some(crate_) => vec![crate_.to_string()],
        None => request
            .list_available_crates()
            .map(|ci| ci.name().to_string())
            .collect(),
    };

    // Search each crate in parallel and collect results
    let crate_results: Vec<_> = crate_names
        .par_iter()
        .filter_map(|crate_name| {
            log::debug!("Starting index load for {crate_name}");

            // Try to load/build the search index for this crate
            match SearchIndex::load_or_build(request, crate_name) {
                Ok(index) => {
                    let len = index.len();
                    log::debug!("Loaded index for {crate_name} ({len} items)");

                    // Search and collect results paired with crate name
                    let results = index.search(query);
                    log::debug!("Searched {crate_name}: {} results", results.results.len());
                    Some((crate_name.as_str(), results, len))
                }
                Err(_) => {
                    log::debug!("Failed to load index for {crate_name}");
                    None
                }
            }
        })
        .collect();

    let total_items: usize = crate_results.iter().map(|(_, _, len)| len).sum();

    log::debug!(
        "Starting BM25 scoring across {} crates",
        crate_results.len()
    );

    // Use BM25 scorer to aggregate results across crates
    let mut scorer = BM25Scorer::new();
    for (crate_name, results, _) in crate_results {
        scorer.add(crate_name, results);
    }

    // Get scored results
    let scored_results = scorer.score();
    log::debug!(
        "BM25 scoring complete: {} total results",
        scored_results.len()
    );

    log::info!(
        "Found {} matching items out of {total_items}",
        scored_results.len()
    );

    if scored_results.is_empty() {
        let error_doc = Document::from(vec![DocumentNode::paragraph(vec![Span::plain(format!(
            "No results found for '{}'",
            query
        ))])]);
        return (error_doc, true);
    }

    // Calculate total score for normalization
    let total_score: f32 = scored_results.iter().map(|r| r.score).sum();
    let top_score = scored_results.first().map(|r| r.score).unwrap_or(0.0);

    let mut nodes = vec![DocumentNode::Heading {
        level: HeadingLevel::Title,
        spans: vec![
            Span::plain("Search results for '"),
            Span::plain(query.to_string()),
            Span::plain("'"),
        ],
    }];

    // Display results with early stopping based on score thresholds
    let min_results = 1;
    let mut cumulative_score = 0.0;
    let mut prev_score = top_score;
    let mut list_items = vec![];

    for (i, result) in scored_results.into_iter().enumerate() {
        // Early stopping: stop if we've shown enough results and scores are dropping significantly
        if i >= min_results && i >= limit {
            break;
        }

        if i >= min_results
            && (result.score / top_score < 0.05
                || result.score / prev_score < 0.5
                || cumulative_score / total_score > 0.3)
        {
            break;
        }

        if let Some((item, path_segments)) =
            request.get_item_from_id_path(result.crate_name, &result.id_path)
        {
            cumulative_score += result.score;
            prev_score = result.score;

            let path = path_segments.join("::");
            let normalized_score = 100.0 * result.score / total_score;

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
