use crate::{
    format::ItemDoc,
    request::Request,
    styled_string::{Document, DocumentNode, ListItem, Span},
};
use ferritin_common::{CratePath, DocRef, Suggestion, block_on};
use rustdoc_types::Item;
use std::collections::HashSet;

/// Outcome of resolving a path for `--format json`: either the structural item
/// model (with its canonical URL) or a structural not-found result carrying the
/// "did you mean" suggestions.
pub(crate) enum JsonOutcome<'a> {
    Found {
        /// Boxed to keep the enum small: an `ItemDoc` is several hundred bytes,
        /// an order of magnitude more than the not-found payload it would
        /// otherwise pad out.
        model: Box<ItemDoc<'a>>,
        canonical_url: String,
    },
    NotFound(NotFoundDoc<'a>),
}

/// Structural "could not find / did you mean" result. Built when a path doesn't
/// resolve; lowered to the suggestions document for the terminal
/// ([`lower_not_found`]) and serialized as `{ error, query, suggestions }` for
/// JSON.
pub(crate) struct NotFoundDoc<'a> {
    /// The path that failed to resolve.
    pub(crate) query: String,
    pub(crate) suggestions: Vec<SuggestionDoc<'a>>,
    /// Set when the leading crate segment names a crate that exists on
    /// crates.io but whose documentation we could not load (no rustdoc JSON,
    /// a failed build, an unsupported target, …). This is a distinct outcome
    /// from a typo — the crate is real, we just can't serve it — so it is
    /// shown with its own message and no "did you mean" list.
    pub(crate) unavailable_crate: Option<String>,
}

/// A single "did you mean" candidate: its path and the resolved item (for kind
/// and nav URL).
pub(crate) struct SuggestionDoc<'a> {
    pub(crate) path: String,
    pub(crate) item: Option<DocRef<'a, Item>>,
}

/// Number of "did you mean" candidates surfaced — the top score-ranked matches.
const MAX_SUGGESTIONS: usize = 5;

/// Minimum similarity for a *crate-name* suggestion to be offered, so an
/// unrelated request (`xyzzy`) yields nothing rather than the nearest few random
/// crates — in particular no longer "did you mean std?". Item-path suggestions
/// come from the resolved crate tree and are not filtered this way.
const CRATE_SUGGESTION_THRESHOLD: f64 = 0.7;

impl<'a> NotFoundDoc<'a> {
    fn new(query: &str, suggestions: &[Suggestion<'a>], unavailable_crate: Option<String>) -> Self {
        // An "exists but unavailable" crate is not a typo, so it carries no
        // "did you mean" list — the exact-match candidate would be misleading.
        let suggestions = if unavailable_crate.is_some() {
            Vec::new()
        } else {
            // Rank by score before truncating: the most likely candidate must
            // survive the `MAX_SUGGESTIONS` cut and lead the list.
            let mut ranked: Vec<&Suggestion<'a>> = suggestions.iter().collect();
            ranked.sort_by(|a, b| b.score().total_cmp(&a.score()));
            let mut seen = HashSet::new();
            ranked
                .into_iter()
                // Crate-name candidates (no resolved item) are drawn from the
                // whole namespace, so drop the barely-similar ones. Item-path
                // candidates carry a resolved item and are kept as-is.
                .filter(|s| s.item().is_some() || s.score() >= CRATE_SUGGESTION_THRESHOLD)
                .filter(|s| seen.insert(s.path().to_string()))
                .take(MAX_SUGGESTIONS)
                .map(|s| SuggestionDoc {
                    path: s.path().to_string(),
                    item: s.item().copied(),
                })
                .collect()
        };
        Self {
            query: query.to_string(),
            suggestions,
            unavailable_crate,
        }
    }
}

/// Assemble the not-found document for a failed `get`, consulting the crates.io
/// namespace index for the leading crate segment.
///
/// This function owns the sync/async boundary. The index API is async (a query
/// may revalidate the artifact), so it is driven with [`block_on`] *here*, at
/// the command layer, rather than behind a synchronous method in
/// `ferritin-common` — every caller of this command runs on a blocking context
/// (the serve worker pool, the CLI's main thread, the TUI's request thread), so
/// the block is honest and safe where an executor thread would not be.
fn build_not_found<'a>(
    request: &Request<'a>,
    path: &str,
    mut suggestions: Vec<Suggestion<'a>>,
) -> NotFoundDoc<'a> {
    let CratePath {
        name, version_req, ..
    } = CratePath::parse(path);

    // Item-level miss: the crate loaded, so the failure is a path within it. The
    // resolver already produced item suggestions; the index has nothing to add.
    if request.navigator().load_crate(name, &version_req).is_some() {
        return NotFoundDoc::new(path, &suggestions, None);
    }

    // Crate-level miss: ask the namespace index, once, whether the crate is real
    // (docs unavailable) or a typo (worth suggestions).
    let missing = block_on(request.navigator().classify_missing_crate(name));
    if let Some(exists_as) = missing.exists_as {
        return NotFoundDoc::new(path, &suggestions, Some(exists_as));
    }
    suggestions.extend(
        missing
            .suggestions
            .into_iter()
            .map(|(name, score)| Suggestion::for_crate(name, score)),
    );
    NotFoundDoc::new(path, &suggestions, None)
}

/// Resolve a path to its semantic [`ItemDoc`] model for the `--format json`
/// output, or to the not-found suggestions document.
pub(crate) fn model<'a>(
    request: &mut Request<'a>,
    path: &str,
    source: bool,
    recursive: bool,
) -> JsonOutcome<'a> {
    request
        .format_context()
        .set_include_source(source)
        .set_recursive(recursive);

    let mut suggestions = vec![];
    match request.resolve_path(path, &mut suggestions) {
        Some(item) => JsonOutcome::Found {
            canonical_url: crate::docsrs_url::generate_docsrs_url(item),
            model: Box::new(request.model_item(item)),
        },
        None => JsonOutcome::NotFound(build_not_found(request, path, suggestions)),
    }
}

/// Lower a [`NotFoundDoc`] to the "Could not find … / Did you mean" document,
/// shared by the text (`execute`) and JSON (`model`) not-found paths.
pub(crate) fn lower_not_found<'a>(not_found: &NotFoundDoc<'a>) -> Document<'a> {
    let mut nodes = vec![DocumentNode::paragraph(vec![Span::plain(format!(
        "Could not find '{}'",
        not_found.query,
    ))])];

    // The crate is real; we just can't serve its docs. Say so plainly instead
    // of offering typo suggestions for a correctly-spelled name.
    if let Some(name) = &not_found.unavailable_crate {
        nodes.push(DocumentNode::paragraph(vec![Span::plain(format!(
            "The crate '{name}' exists on crates.io, but its documentation isn't available here \
             (no rustdoc JSON on docs.rs)."
        ))]));
        return Document::from(nodes);
    }

    if !not_found.suggestions.is_empty() {
        nodes.push(DocumentNode::paragraph(vec![Span::plain("Did you mean:")]));
        let items = not_found
            .suggestions
            .iter()
            .map(|s| {
                ListItem::new(vec![DocumentNode::paragraph(vec![
                    Span::plain(s.path.clone()).with_target(s.item),
                ])])
            })
            .collect();
        nodes.push(DocumentNode::List { items });
    }

    Document::from(nodes)
}

pub(crate) fn execute<'a>(
    request: &mut Request<'a>,
    path: &str,
    source: bool,
    recursive: bool,
) -> (Document<'a>, bool, Option<DocRef<'a, Item>>) {
    request
        .format_context()
        .set_include_source(source)
        .set_recursive(recursive);

    let mut suggestions = vec![];
    log::info!("Getting {path}...");

    match request.resolve_path(path, &mut suggestions) {
        Some(item) => {
            if let Some(name) = item.name() {
                log::info!("Resolved {name}");
            }
            let start = std::time::Instant::now();
            let doc_nodes = request.format_item(item);
            let format_elapsed = start.elapsed();
            if let Some(name) = item.name() {
                log::debug!("⏱️ Formatted {name} in {:?}", format_elapsed);
            }
            (Document::from(doc_nodes), false, Some(item))
        }
        None => (
            lower_not_found(&build_not_found(request, path, suggestions)),
            true,
            None,
        ),
    }
}
