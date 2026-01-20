use crate::request::Request;
use crate::styled_string::{Document, DocumentNode, ListItem, Span};

pub(crate) fn execute<'a>(
    request: &'a Request,
    path: &str,
    source: bool,
    recursive: bool,
) -> (
    Document<'a>,
    bool,
    Option<ferretin_common::DocRef<'a, rustdoc_types::Item>>,
) {
    request.mutate_format_context(|fc| {
        fc.set_include_source(source).set_recursive(recursive);
    });

    let mut suggestions = vec![];

    match request.resolve_path(path, &mut suggestions) {
        Some(item) => {
            let doc_nodes = request.format_item(item);
            (Document::from(doc_nodes), false, Some(item))
        }
        None => {
            let mut nodes = vec![DocumentNode::Span(Span::plain(format!(
                "Could not find '{path}'",
            )))];

            if !suggestions.is_empty() {
                nodes.push(DocumentNode::Span(Span::plain("\n\nDid you mean:\n")));
                let items = suggestions
                    .iter()
                    .take(5)
                    .map(|s| {
                        ListItem::from_span(
                            Span::plain(s.path().to_string()).with_target(s.item().copied()),
                        )
                    })
                    .collect();

                nodes.push(DocumentNode::List { items });
            }

            (Document::from(nodes), true, None)
        }
    }
}
