use crate::styled_string::DocumentNode;
use crossterm::queue;
use ratatui::backend::CrosstermBackend;
use std::io::{self, Write};

/// Detect if the terminal supports mouse cursor shape changes
pub(super) fn supports_cursor_shape() -> bool {
    // Kitty, WezTerm, and some other modern terminals support OSC 22
    std::env::var("TERM_PROGRAM")
        .map(|t| t == "kitty" || t == "WezTerm")
        .unwrap_or(false)
        || std::env::var("TERM")
            .map(|t| t.contains("kitty"))
            .unwrap_or(false)
}

/// Set the mouse cursor shape (for terminals that support it)
pub(super) fn set_cursor_shape(
    backend: &mut CrosstermBackend<std::io::Stdout>,
    shape: &str,
) -> io::Result<()> {
    // OSC 22 sequence: \x1b]22;<shape>\x07
    // Supported shapes: default, pointer, text, etc.
    queue!(
        backend,
        crossterm::style::Print(format!("\x1b]22;{}\x07", shape))
    )?;
    backend.flush()
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
    nodes: &[DocumentNode],
    max_lines: u16,
    screen_width: u16,
) -> Option<usize> {
    let mut paragraph_breaks = 0;
    let mut estimated_lines = 0u16;
    let mut consecutive_newlines = 0;

    for (idx, node) in nodes.iter().enumerate() {
        // Estimate lines this node will take
        estimated_lines += estimate_node_lines(node, screen_width);

        // Track newlines across span boundaries
        if let DocumentNode::Span(span) = node {
            // Count newlines at the start of this span
            for ch in span.text.chars() {
                if ch == '\n' {
                    consecutive_newlines += 1;
                    // Two or more consecutive newlines = paragraph break
                    if consecutive_newlines >= 2 {
                        paragraph_breaks += 1;

                        // Found second paragraph break - truncate here if within line limit
                        if paragraph_breaks >= 2 {
                            if estimated_lines <= max_lines {
                                return Some(idx);
                            } else {
                                // Second paragraph is too long, fall back to line limit
                                return None;
                            }
                        }

                        // Reset counter after detecting a break
                        consecutive_newlines = 0;
                    }
                } else if !ch.is_whitespace() {
                    // Non-whitespace resets the counter
                    consecutive_newlines = 0;
                }
            }
        } else {
            // Non-span nodes reset newline tracking
            consecutive_newlines = 0;
        }
    }

    // Didn't find second paragraph break
    None
}

/// Estimate how many lines a node will consume when rendered
pub(super) fn estimate_node_lines(node: &DocumentNode, screen_width: u16) -> u16 {
    match node {
        DocumentNode::Span(span) => {
            // Count explicit newlines + word wrapping
            let text_len = span.text.len() as u16;
            let newline_count = span.text.matches('\n').count() as u16;
            let wrapped_lines = if screen_width > 0 {
                (text_len + screen_width - 1) / screen_width // Ceiling division
            } else {
                1
            };
            newline_count.max(1) + wrapped_lines.saturating_sub(1)
        }
        DocumentNode::Heading { .. } => 3, // Title + underline + spacing
        DocumentNode::CodeBlock { code, .. } => {
            code.lines().count() as u16 + 2 // Lines + spacing
        }
        DocumentNode::HorizontalRule => 1,
        DocumentNode::List { items } => items.len() as u16, // Rough estimate
        _ => 2,                                             // Default estimate for other nodes
    }
}
