use ratatui::{buffer::Buffer, layout::Rect, style::Modifier};

use super::{state::InteractiveState, utils::find_paragraph_truncation_point};
use crate::document::{DocumentNode, HeadingLevel, ShowWhen, TruncationLevel, TuiAction};

// Truncated block borders are outdented (to the left of content) so that content
// doesn't shift when expanding/collapsing the block. The border is purely decorative.
const TRUNCATION_BORDER_WIDTH: u16 = 2; // "│ " takes 2 columns
const TRUNCATION_BORDER_OUTDENT: i16 = -2; // Draw border 2 columns left of content

impl<'a> InteractiveState<'a> {
    /// Draw all active blockquote markers at the current row
    pub(super) fn draw_blockquote_markers(&mut self, buf: &mut Buffer) {
        let quote_style = self.theme.muted_style;
        for &marker_x in &self.layout.blockquote_markers {
            self.write_text(
                buf,
                self.layout.pos.y,
                marker_x,
                "  ┃ ",
                self.layout.area,
                quote_style,
            );
        }
    }

    /// Render a single node
    pub(super) fn render_node(&mut self, node: &DocumentNode<'a>, buf: &mut Buffer) {
        match node {
            DocumentNode::Paragraph { spans } => {
                // Block element: unconditionally position at indent
                self.layout.pos.x = self.layout.indent;
                // Draw blockquote markers if we're inside a blockquote
                self.draw_blockquote_markers(buf);
                for span in spans {
                    self.render_span(span, buf);
                }

                // Block element: increment y when done
                self.layout.pos.y += 1;
            }

            DocumentNode::Heading { level, spans } => {
                // Block element: unconditionally position at indent
                self.layout.pos.x = self.layout.indent;
                // Draw blockquote markers if we're inside a blockquote
                self.draw_blockquote_markers(buf);

                // Render heading spans (bold)
                for span in spans {
                    self.render_span_with_modifier(span, Modifier::BOLD, buf);
                }

                // New line after heading
                self.layout.pos.y += 1;

                // Add decorative underline (respecting left margin)
                let underline_char = match level {
                    HeadingLevel::Title => '═',
                    HeadingLevel::Section => '┄',
                };

                // Draw blockquote markers on underline row
                self.draw_blockquote_markers(buf);

                if self.layout.pos.y >= self.viewport.scroll_offset
                    && self.layout.pos.y < self.viewport.scroll_offset + self.layout.area.height
                {
                    for c in self.layout.indent..self.layout.area.width {
                        buf.cell_mut((c, self.layout.pos.y - self.viewport.scroll_offset))
                            .unwrap()
                            .set_char(underline_char);
                    }
                }

                // Block element: increment y when done
                self.layout.pos.y += 1;
            }

            DocumentNode::List { items } => {
                for (item_idx, item) in items.iter().enumerate() {
                    // Add blank line between list items
                    if item_idx > 0 {
                        // Draw blockquote markers on blank line
                        self.draw_blockquote_markers(buf);
                        self.layout.pos.y += 1;
                    }

                    // Block element: unconditionally position at indent
                    self.layout.pos.x = self.layout.indent;
                    // Draw blockquote markers before bullet
                    self.draw_blockquote_markers(buf);

                    // Bullet with nice unicode character based on nesting level
                    let bullet = crate::renderer::bullet_for_indent(self.layout.indent);
                    let bullet_text = format!("  {} ", bullet);
                    let bullet_style = self.theme.muted_style;
                    self.write_text(
                        buf,
                        self.layout.pos.y,
                        self.layout.pos.x,
                        &bullet_text,
                        self.layout.area,
                        bullet_style,
                    );
                    self.layout.pos.x += 4;

                    // Save indent and update for content
                    let saved_indent = self.layout.indent;
                    self.layout.indent = self.layout.pos.x;

                    // Render content nodes
                    // Note: no blank line spacing between blocks in list items - keep them compact
                    for (content_idx, content_node) in item.content.iter().enumerate() {
                        // Save path, update it, render, restore
                        let saved_path = self.layout.node_path;
                        self.layout.node_path.push(item_idx);
                        self.layout.node_path.push(content_idx);

                        self.render_node(content_node, buf);

                        self.layout.node_path = saved_path;
                    }

                    // Restore indent
                    self.layout.indent = saved_indent;
                }
                // Container: children handle their own spacing
            }

            DocumentNode::Section { title, nodes } => {
                if let Some(title_spans) = title {
                    // Block element: unconditionally position at indent
                    self.layout.pos.x = self.layout.indent;

                    for span in title_spans {
                        self.render_span_with_modifier(span, Modifier::BOLD, buf);
                    }

                    // Add blank line after section title
                    self.layout.pos.y += 1;
                    self.layout.pos.y += 1;
                }

                for (idx, child_node) in nodes.iter().enumerate() {
                    // Add blank line between consecutive blocks
                    if idx > 0 {
                        self.layout.pos.y += 1;
                    }

                    // Save and update path
                    let saved_path = self.layout.node_path;
                    self.layout.node_path.push(idx);

                    self.render_node(child_node, buf);

                    self.layout.node_path = saved_path;
                }
                // Container: children handle their own spacing
            }

            DocumentNode::CodeBlock { lang, code } => {
                // Block element: unconditionally position at indent
                self.layout.pos.x = self.layout.indent;

                self.render_code_block(lang.as_deref(), code, buf);

                // Block element: increment y when done
                self.layout.pos.y += 1;
            }

            DocumentNode::GeneratedCode { spans } => {
                // Block element: unconditionally position at indent
                self.layout.pos.x = self.layout.indent;
                // Draw blockquote markers if we're inside a blockquote
                self.draw_blockquote_markers(buf);

                // Render spans inline (flows with current position)
                for span in spans {
                    self.render_span(span, buf);
                }

                // Block element: increment y when done
                self.layout.pos.y += 1;
            }

            DocumentNode::HorizontalRule => {
                // Block element: unconditionally position at indent
                self.layout.pos.x = self.layout.indent;
                // Draw blockquote markers if we're inside a blockquote
                self.draw_blockquote_markers(buf);

                if self.layout.pos.y >= self.viewport.scroll_offset
                    && self.layout.pos.y < self.viewport.scroll_offset + self.layout.area.height
                {
                    let rule_style = self.theme.muted_style;
                    // Use a decorative pattern: ─── • ───
                    let pattern = ['─', '─', '─', ' ', '•', ' '];
                    for c in 0..self.layout.area.width {
                        let ch = pattern[(c as usize) % pattern.len()];
                        if let Some(cell) =
                            buf.cell_mut((c, self.layout.pos.y - self.viewport.scroll_offset))
                        {
                            cell.set_char(ch);
                            cell.set_style(rule_style);
                        }
                    }
                }

                // Block element: increment y when done
                self.layout.pos.y += 1;
            }

            DocumentNode::BlockQuote { nodes } => {
                // Add this blockquote's marker position to the stack
                let marker_x = self.layout.indent;
                self.layout.blockquote_markers.push(marker_x);

                // Update indent to account for marker
                let saved_indent = self.layout.indent;
                self.layout.indent += 4; // "  ┃ " takes 4 columns

                for (idx, child_node) in nodes.iter().enumerate() {
                    // Add blank line between consecutive blocks
                    if idx > 0 {
                        // Draw all blockquote markers on the blank line
                        self.draw_blockquote_markers(buf);
                        self.layout.pos.y += 1;
                    }

                    let saved_path = self.layout.node_path;
                    self.layout.node_path.push(idx);
                    self.render_node(child_node, buf);
                    self.layout.node_path = saved_path;
                }

                // Restore indent and pop marker
                self.layout.indent = saved_indent;
                self.layout.blockquote_markers.pop();

                // Container: children handle their own spacing
            }

            DocumentNode::Table { header, rows } => {
                // Block element: unconditionally position at indent
                self.layout.pos.x = self.layout.indent;

                self.render_table(header.as_deref(), rows, buf);

                // Block element: increment y when done
                self.layout.pos.y += 1;
            }

            DocumentNode::TruncatedBlock { nodes, level } => {
                // Transparent container: doesn't add its own newlines
                // Just controls which children to render and adds decorative borders if truncated

                // Determine line limit based on truncation level
                let line_limit = match level {
                    TruncationLevel::SingleLine => 3,  // Show ~3 lines for single-line
                    TruncationLevel::Brief => 8, // Show ~8 lines for brief (actual wrapped lines)
                    TruncationLevel::Full => u16::MAX, // Show everything
                };

                let start_row = self.layout.pos.y;
                let mut rendered_all = true;
                let border_style = self.theme.muted_style;

                // Calculate border and content columns
                // Border is outdented (to the left) so content doesn't shift when expanding
                // Full mode never truncates, so it will never use the border
                let border_col = self
                    .layout
                    .indent
                    .saturating_add_signed(TRUNCATION_BORDER_OUTDENT);
                let content_col = self.layout.indent; // Content stays at current indent

                // For SingleLine with heading as first node, just show the heading text (no decoration)
                let render_nodes = if matches!(level, TruncationLevel::SingleLine) {
                    match nodes.first() {
                        Some(DocumentNode::Heading { spans, .. }) => {
                            // Block element: unconditionally position at indent
                            self.layout.pos.x = self.layout.indent;
                            // Just render the spans without the heading decoration
                            for span in spans {
                                self.render_span(span, buf);
                            }
                            // Heading normally increments y, so do it here too
                            self.layout.pos.y += 1;
                            rendered_all = nodes.len() <= 1;
                            false // Skip normal rendering
                        }
                        _ => true, // Normal rendering for other node types
                    }
                } else {
                    true
                };

                if render_nodes {
                    // For Brief mode, try to find a good truncation point at second paragraph break
                    let truncate_at = if matches!(level, TruncationLevel::Brief) {
                        find_paragraph_truncation_point(nodes, line_limit, self.layout.area.width)
                    } else {
                        None
                    };

                    // Track last row with actual content (to trim trailing blank lines)
                    let mut last_content_row = start_row;

                    // Render nodes
                    for (idx, child_node) in nodes.iter().enumerate() {
                        // Check if we've hit our truncation point
                        if let Some(cutoff) = truncate_at
                            && idx >= cutoff
                        {
                            rendered_all = false;
                            break;
                        }

                        // Check if we've exceeded the line limit (fallback)
                        if self.layout.pos.y - start_row >= line_limit
                            && !matches!(level, TruncationLevel::Full)
                        {
                            rendered_all = false;
                            break;
                        }

                        // Skip headings in the middle of truncated content (not first node)
                        // Only do this for Brief/SingleLine, not Full
                        if idx > 0
                            && !matches!(level, TruncationLevel::Full)
                            && matches!(child_node, DocumentNode::Heading { .. })
                        {
                            rendered_all = false;
                            break;
                        }

                        // Skip code blocks and lists in Brief/SingleLine mode
                        // These are multi-line structures that don't make sense in snippets
                        if !matches!(level, TruncationLevel::Full)
                            && matches!(
                                child_node,
                                DocumentNode::CodeBlock { .. }
                                    | DocumentNode::GeneratedCode { .. }
                                    | DocumentNode::List { .. }
                            )
                        {
                            rendered_all = false;
                            break;
                        }

                        // Add blank line between consecutive blocks
                        if idx > 0 {
                            self.layout.pos.y += 1;
                        }

                        // Save and update path and indent
                        let saved_path = self.layout.node_path;
                        self.layout.node_path.push(idx);
                        let saved_indent = self.layout.indent;
                        self.layout.indent = content_col;

                        let row_before = self.layout.pos.y;
                        self.render_node(child_node, buf);

                        // Restore path and indent
                        self.layout.node_path = saved_path;
                        self.layout.indent = saved_indent;

                        // Track last row with content
                        if self.layout.pos.y > row_before {
                            last_content_row = self.layout.pos.y;
                        }

                        // If this is the last node, we rendered everything
                        if idx == nodes.len() - 1 {
                            rendered_all = true;
                        }
                    }

                    // Draw left border only if content was truncated
                    if !rendered_all {
                        // Draw borders from start to last content row (exclusive)
                        for r in start_row..last_content_row {
                            if r >= self.viewport.scroll_offset
                                && r < self.viewport.scroll_offset + self.layout.area.height
                            {
                                self.write_text(
                                    buf,
                                    r,
                                    border_col,
                                    "│ ",
                                    self.layout.area,
                                    border_style,
                                );
                            }
                        }
                    }
                }

                // Show bottom border with [...] if we didn't render all nodes
                if !rendered_all {
                    let ellipsis_text = "╰─[...]";
                    let ellipsis_row = self.layout.pos.y;

                    // Check if hovered
                    let is_hovered = self.viewport.cursor_pos.map_or_else(
                        || false,
                        |cursor_pos| {
                            cursor_pos.y == ellipsis_row
                                && cursor_pos.x >= border_col
                                && cursor_pos.x < border_col + ellipsis_text.len() as u16
                        },
                    );

                    let final_style = if is_hovered {
                        border_style.add_modifier(Modifier::REVERSED)
                    } else {
                        border_style
                    };

                    // Write the border with ellipsis
                    self.write_text(
                        buf,
                        ellipsis_row,
                        border_col,
                        ellipsis_text,
                        self.layout.area,
                        final_style,
                    );

                    // Track the action with the current path
                    let rect = Rect::new(border_col, ellipsis_row, ellipsis_text.len() as u16, 1);
                    self.render_cache
                        .actions
                        .push((rect, TuiAction::ExpandBlock(self.layout.node_path)));

                    // Increment y to account for ellipsis line
                    self.layout.pos.y += 1;
                }
                // Transparent container: no additional spacing
            }

            DocumentNode::Conditional { show_when, nodes } => {
                // Transparent container: doesn't add its own newlines
                // Interactive renderer is always in interactive mode
                let should_show = match show_when {
                    ShowWhen::Always => true,
                    ShowWhen::Interactive => true,
                    ShowWhen::NonInteractive => false,
                };

                if should_show {
                    for (idx, node) in nodes.iter().enumerate() {
                        // Add blank line between consecutive blocks
                        if idx > 0 {
                            self.layout.pos.y += 1;
                        }
                        self.render_node(node, buf);
                    }
                }
                // Transparent container: no additional spacing
            }
        }
    }
}
