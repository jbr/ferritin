use ferritin_common::{DocRef, Suggestion};
use rustdoc_types::Item;

use crate::format::ItemDoc;
use crate::request::Request;
use crate::styled_string::{Document, DocumentNode, ListItem, Span};

/// Outcome of resolving a path for `--format json`: either the structural item
/// model (with its canonical URL) or the "could not find / did you mean"
/// document, which the JSON path serializes generically — keeping parity with
/// the text renderers, which show the same suggestions.
pub(crate) enum JsonOutcome<'a> {
    Found {
        model: ItemDoc<'a>,
        canonical_url: String,
    },
    NotFound(Document<'a>),
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
            canonical_url: crate::generate_docsrs_url::generate_docsrs_url(item),
            model: request.model_item(item),
        },
        None => JsonOutcome::NotFound(not_found_document(path, &suggestions)),
    }
}

/// Build the "Could not find … / Did you mean" document, shared by the text
/// (`execute`) and JSON (`model`) not-found paths.
fn not_found_document<'a>(path: &str, suggestions: &[Suggestion<'a>]) -> Document<'a> {
    let mut nodes = vec![DocumentNode::paragraph(vec![Span::plain(format!(
        "Could not find '{path}'",
    ))])];

    if !suggestions.is_empty() {
        nodes.push(DocumentNode::paragraph(vec![Span::plain("Did you mean:")]));
        let items = suggestions
            .iter()
            .take(5)
            .map(|s| {
                ListItem::new(vec![DocumentNode::paragraph(vec![
                    Span::plain(s.path().to_string()).with_target(s.item().copied()),
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
        None => (not_found_document(path, &suggestions), true, None),
    }
}
