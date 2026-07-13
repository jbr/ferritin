use ferritin_common::{DocRef, Suggestion};
use rustdoc_types::Item;

use crate::format::ItemDoc;
use crate::request::Request;
use crate::styled_string::{Document, DocumentNode, ListItem, Span};

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
}

/// A single "did you mean" candidate: its path and the resolved item (for kind
/// and nav URL).
pub(crate) struct SuggestionDoc<'a> {
    pub(crate) path: String,
    pub(crate) item: Option<DocRef<'a, Item>>,
}

/// Number of "did you mean" candidates surfaced — the top score-ranked matches.
const MAX_SUGGESTIONS: usize = 5;

impl<'a> NotFoundDoc<'a> {
    fn new(query: &str, suggestions: &[Suggestion<'a>]) -> Self {
        Self {
            query: query.to_string(),
            suggestions: suggestions
                .iter()
                .take(MAX_SUGGESTIONS)
                .map(|s| SuggestionDoc {
                    path: s.path().to_string(),
                    item: s.item().copied(),
                })
                .collect(),
        }
    }
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
        None => JsonOutcome::NotFound(NotFoundDoc::new(path, &suggestions)),
    }
}

/// Lower a [`NotFoundDoc`] to the "Could not find … / Did you mean" document,
/// shared by the text (`execute`) and JSON (`model`) not-found paths.
pub(crate) fn lower_not_found<'a>(not_found: &NotFoundDoc<'a>) -> Document<'a> {
    let mut nodes = vec![DocumentNode::paragraph(vec![Span::plain(format!(
        "Could not find '{}'",
        not_found.query,
    ))])];

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
            lower_not_found(&NotFoundDoc::new(path, &suggestions)),
            true,
            None,
        ),
    }
}
