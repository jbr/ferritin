use super::utils::find_node_at_path_mut;
use crate::styled_string::{Document, DocumentNode, TruncationLevel, TuiAction};

/// Handle a TuiAction, returning (new Document, DocRef) if navigation occurred
pub(super) fn handle_action<'a>(
    document: &mut Document<'a>,
    action: &TuiAction<'a>,
    request: &'a crate::request::Request,
) -> Option<(
    Document<'a>,
    ferretin_common::DocRef<'a, rustdoc_types::Item>,
)> {
    match action {
        TuiAction::ExpandBlock(path) => {
            // Find the node at this path and expand it
            if let Some(node) = find_node_at_path_mut(&mut document.nodes, path.indices())
                && let DocumentNode::TruncatedBlock { level, .. } = node
            {
                // Cycle through truncation levels: SingleLine -> Full
                *level = match level {
                    TruncationLevel::SingleLine | TruncationLevel::Brief => TruncationLevel::Full,
                    TruncationLevel::Full => TruncationLevel::Full, // Already expanded
                };
            }
            None // No new document, just mutated in place
        }
        TuiAction::Navigate(doc_ref) => {
            // Format the item directly without path lookup
            let doc_nodes = request.format_item(*doc_ref);
            Some((Document::from(doc_nodes), *doc_ref))
        }
        TuiAction::NavigateToPath(path) => {
            // Resolve the path and navigate if found
            let mut suggestions = vec![];
            if let Some(doc_ref) = request.resolve_path(path, &mut suggestions) {
                let doc_nodes = request.format_item(doc_ref);
                Some((Document::from(doc_nodes), doc_ref))
            } else {
                None // Path not found
            }
        }
        TuiAction::OpenUrl(url) => {
            // Open external URL in browser
            if let Err(e) = webbrowser::open(url) {
                eprintln!("[ERROR] Failed to open URL {}: {}", url, e);
            }
            None // No new document
        }
    }
}
