use std::time::Instant;

use crate::document::{Document, DocumentNode, ListItem, Span};
use crate::renderer::HistoryEntry;
use crate::request::Request;

pub(crate) fn execute<'a>(
    request: &'a Request,
    path: &str,
    source: bool,
    recursive: bool,
) -> Document<'a> {
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
            let start = Instant::now();
            let doc_nodes = request.format_item(item);
            let format_elapsed = start.elapsed();
            if let Some(name) = item.name() {
                log::debug!("⏱️ Formatted {name} in {:?}", format_elapsed);
            }
            Document::from(doc_nodes)
                .with_item(item)
                .with_history_entry(HistoryEntry::Item(item))
        }
        None => {
            let mut nodes = vec![DocumentNode::paragraph(vec![
                Span::plain("Could not find '"),
                Span::emphasis(path.to_string()),
                Span::plain("'"),
            ])];

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

            Document::from(nodes).with_error()
        }
    }
}
