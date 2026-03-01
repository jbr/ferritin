use crate::document::DocumentNode;
use crossterm::{queue, style::Print};
use ratatui::prelude::Backend;
use std::{env, io};

/// Detect if the terminal supports mouse cursor shape changes
pub(super) fn supports_cursor_shape() -> bool {
    // Kitty, WezTerm, and some other modern terminals support OSC 22
    env::var("TERM_PROGRAM")
        .map(|t| t == "kitty" || t == "WezTerm")
        .unwrap_or(false)
        || env::var("TERM")
            .map(|t| t.contains("kitty"))
            .unwrap_or(false)
}

/// Set the mouse cursor shape (for terminals that support it)
pub(super) fn set_cursor_shape<B: Backend + io::Write>(backend: &mut B, shape: &str) {
    // OSC 22 sequence: \x1b]22;<shape>\x07
    // Supported shapes: default, pointer, text, etc.
    let _ = queue!(backend, Print(format!("\x1b]22;{}\x07", shape)));
    let _ = Backend::flush(backend);
}

pub(super) fn find_node_at_path_mut<'a, 'b>(
    nodes: &'a mut [DocumentNode<'b>],
    path: &[u16],
) -> Option<&'a mut DocumentNode<'b>> {
    if path.is_empty() {
        return None;
    }

    let idx = path[0] as usize;
    if idx >= nodes.len() {
        return None;
    }

    if path.len() == 1 {
        // This is the target node
        return Some(&mut nodes[idx]);
    }

    // Recurse into children
    let remaining_path = &path[1..];
    match &mut nodes[idx] {
        DocumentNode::Section { nodes, .. }
        | DocumentNode::BlockQuote { nodes }
        | DocumentNode::TruncatedBlock { nodes, .. } => {
            find_node_at_path_mut(nodes, remaining_path)
        }
        DocumentNode::List { items } => {
            // Path into list items
            if remaining_path.is_empty() {
                return None;
            }
            let item_idx = remaining_path[0] as usize;
            if item_idx >= items.len() {
                return None;
            }
            find_node_at_path_mut(&mut items[item_idx].content, &remaining_path[1..])
        }
        _ => None,
    }
}

/// Find the best truncation point for Brief mode at second paragraph break
/// Returns the node index to stop at, or None to fall back to line-based truncation
pub(super) fn find_paragraph_truncation_point(
    _nodes: &[DocumentNode],
    _max_lines: u16,
    _screen_width: u16,
) -> Option<usize> {
    // Paragraph break detection was removed since Span nodes no longer exist
    // All inline content is now in Paragraph nodes
    None
}

/// Estimate how many lines a node will consume when rendered
pub(super) fn estimate_node_lines(node: &DocumentNode, _screen_width: u16) -> u16 {
    match node {
        DocumentNode::Heading { .. } => 3, // Title + underline + spacing
        DocumentNode::CodeBlock { code, .. } => {
            code.lines().count() as u16 + 2 // Lines + spacing
        }
        DocumentNode::GeneratedCode { spans } => {
            // Count newlines in the spans
            let newlines = spans.iter().filter(|s| s.text.contains('\n')).count() as u16;
            newlines.max(1) + 1 // At least 1 line + spacing
        }
        DocumentNode::HorizontalRule => 1,
        DocumentNode::List { items } => items.len() as u16, // Rough estimate
        _ => 2,                                             // Default estimate for other nodes
    }
}
