use super::channels::UiCommand;
use super::utils::find_node_at_path_mut;
use crate::document::{Document, DocumentNode, TruncationLevel, TuiAction};

/// Handle a TuiAction, returning a command to send if navigation is needed
///
/// Some actions (ExpandBlock) mutate the document in place and return None.
/// Navigation actions return a UiCommand that the caller should send via channel.
pub(super) fn handle_action<'a>(
    document: &mut Document<'a>,
    action: TuiAction<'a>,
) -> Option<UiCommand<'a>> {
    match action {
        TuiAction::ExpandBlock(path) => {
            // Find the node at this path and expand it
            if let Some(node) = find_node_at_path_mut(document.nodes_mut(), path.indices())
                && let DocumentNode::TruncatedBlock { level, .. } = node
            {
                // Cycle through truncation levels: SingleLine -> Full
                *level = match level {
                    TruncationLevel::SingleLine | TruncationLevel::Brief => TruncationLevel::Full,
                    TruncationLevel::Full => TruncationLevel::Full, // Already expanded
                };
            }
            None // No command needed, just mutated in place
        }
        TuiAction::Navigate { doc_ref, url: _ } => {
            // Return Navigate command - caller will send it and wait for response
            Some(UiCommand::Navigate(doc_ref))
        }
        TuiAction::NavigateToPath { path, url: _ } => {
            // Return NavigateToPath command - caller will send it and wait for response
            Some(UiCommand::NavigateToPath(path))
        }
        TuiAction::OpenUrl(url) => {
            // Open external URL in browser
            if let Err(e) = webbrowser::open(&url) {
                eprintln!("[ERROR] Failed to open URL {}: {}", url, e);
            }
            None // No command needed
        }
        TuiAction::SelectTheme(_) => {
            // SelectTheme is handled specially in mouse.rs handle_click()
            // It should never reach this function, but we need the match to be exhaustive
            None
        }
    }
}
