use ratatui::{buffer::Buffer, layout::Rect, style::Modifier};

use super::state::InteractiveState;
use crate::styled_string::Span;

impl<'a> InteractiveState<'a> {
    /// Render a span with optional action tracking
    pub(super) fn render_span(&mut self, span: &Span<'a>, buf: &mut Buffer) {
        self.render_span_with_modifier(span, Modifier::empty(), buf);
    }

    /// Render a span with additional style modifier
    pub(super) fn render_span_with_modifier(
        &mut self,
        span: &Span<'a>,
        modifier: Modifier,
        buf: &mut Buffer,
    ) {
        let mut style = self.style(span.style);
        style = style.add_modifier(modifier);

        let start_col = self.layout.pos.x;
        let start_row = self.layout.pos.y;

        // Check if this span is hovered
        let is_hovered = if span.action.is_some() {
            self.viewport.cursor_pos.map_or_else(
                || false,
                |cursor| {
                    cursor.y == self.layout.pos.y
                        && cursor.x >= self.layout.pos.x
                        && cursor.x < self.layout.pos.x + span.text.len() as u16
                },
            )
        } else {
            false
        };

        // If hovered, invert colors
        if is_hovered {
            style = style.add_modifier(Modifier::REVERSED);
        }

        // Handle newlines in span text
        for (line_idx, line) in span.text.split('\n').enumerate() {
            if line_idx > 0 {
                self.layout.pos.y += 1;
                self.layout.pos.x = self.layout.indent;
                // Draw blockquote markers on new line
                self.draw_blockquote_markers(buf);
            }

            // Word wrap if line is too long
            let mut remaining = line;
            while !remaining.is_empty() {
                // Calculate available width: columns from current to edge (exclusive)
                // area.width is the total width, so valid columns are 0 to area.width-1
                let available_width = self.layout.area.width.saturating_sub(self.layout.pos.x);

                if available_width == 0 {
                    // No space left on this line, wrap to next
                    self.layout.pos.y += 1;
                    self.layout.pos.x = self.layout.indent;
                    // Draw blockquote markers on new line
                    self.draw_blockquote_markers(buf);
                    continue;
                }

                if remaining.len() <= available_width as usize {
                    // Fits on current line
                    self.write_text(
                        buf,
                        self.layout.pos.y,
                        self.layout.pos.x,
                        remaining,
                        self.layout.area,
                        style,
                    );
                    self.layout.pos.x += remaining.len() as u16;
                    break;
                } else {
                    // Need to wrap - find best break point
                    let truncate_at = available_width as usize;

                    // First try to find a good break point (whitespace or after punctuation)
                    let wrap_pos = find_wrap_position(remaining, truncate_at);

                    if let Some(wrap_at) = wrap_pos {
                        let (chunk, rest) = remaining.split_at(wrap_at);
                        self.write_text(
                            buf,
                            self.layout.pos.y,
                            self.layout.pos.x,
                            chunk,
                            self.layout.area,
                            style,
                        );
                        self.layout.pos.y += 1;
                        self.layout.pos.x = self.layout.indent;
                        // Draw blockquote markers on new line
                        self.draw_blockquote_markers(buf);
                        remaining = rest.trim_start(); // Skip leading whitespace on next line
                    } else {
                        // No good break point within available width
                        // Look for the next break point beyond the available width
                        // This creates ragged right margins but avoids splitting words
                        if let Some(next_space) = remaining.find(char::is_whitespace) {
                            // Check if the word will fit on the current line
                            if next_space <= available_width as usize {
                                // Word fits on current line, write it
                                let (chunk, rest) = remaining.split_at(next_space);
                                self.write_text(
                                    buf,
                                    self.layout.pos.y,
                                    self.layout.pos.x,
                                    chunk,
                                    self.layout.area,
                                    style,
                                );
                                self.layout.pos.y += 1;
                                self.layout.pos.x = self.layout.indent;
                                // Draw blockquote markers on new line
                                self.draw_blockquote_markers(buf);
                                remaining = rest.trim_start();
                            } else {
                                // Word doesn't fit, wrap to next line first
                                self.layout.pos.y += 1;
                                self.layout.pos.x = self.layout.indent;
                                // Draw blockquote markers on new line
                                self.draw_blockquote_markers(buf);
                                // Don't modify remaining, continue on next line and try again
                            }
                        } else {
                            // No whitespace at all in remaining text
                            // If it fits, write it; otherwise wrap first
                            if remaining.len() <= available_width as usize {
                                self.write_text(
                                    buf,
                                    self.layout.pos.y,
                                    self.layout.pos.x,
                                    remaining,
                                    self.layout.area,
                                    style,
                                );
                                self.layout.pos.x += remaining.len() as u16;
                                break;
                            } else {
                                // Doesn't fit, wrap to next line
                                self.layout.pos.y += 1;
                                self.layout.pos.x = self.layout.indent;
                                // Draw blockquote markers on new line
                                self.draw_blockquote_markers(buf);
                                // Continue to try writing on next line
                            }
                        }
                    }
                }
            }
        }

        // Track action if present
        if let Some(action) = &span.action {
            // Calculate width handling wrapping (pos.x might be less than start_col if we wrapped)
            let width = if self.layout.pos.y > start_row {
                // Multi-line span - use full width of first line as clickable area
                self.layout.area.width.saturating_sub(start_col).max(1)
            } else {
                // Single line - use actual span width
                self.layout.pos.x.saturating_sub(start_col).max(1)
            };

            let rect = Rect::new(
                start_col,
                start_row,
                width,
                (self.layout.pos.y - start_row + 1).max(1),
            );
            self.render_cache.actions.push((rect, action.clone()));
        }
    }
}

/// Find the best position to wrap text within a given width
/// Returns the position after which to break, or None if no good break point exists
fn find_wrap_position(text: &str, max_width: usize) -> Option<usize> {
    if max_width == 0 || text.is_empty() {
        return None;
    }

    // Find the byte position that corresponds to max_width characters (char-boundary-safe)
    let search_end = text
        .char_indices()
        .take(max_width)
        .last()
        .map(|(idx, ch)| idx + ch.len_utf8())
        .unwrap_or(0);

    let search_range = &text[..search_end];

    // First priority: break at whitespace
    if let Some(pos) = search_range.rfind(char::is_whitespace) {
        // Avoid breaking if it would leave a very short word (< 3 chars) on next line
        // This prevents orphans like "a" or "is" at the start of a line
        if pos > 0 && text.len() - pos > 3 {
            return Some(pos);
        }
        // If the remaining part is short enough, it's ok to break here
        if text.len() - pos <= max_width / 2 {
            return Some(pos);
        }
    }

    // Second priority: break after certain punctuation (., ,, ;, :, ), ])
    // This helps with long sentences without spaces
    for (i, ch) in search_range.char_indices().rev() {
        if matches!(ch, '.' | ',' | ';' | ':' | ')' | ']' | '}') {
            // Break after the punctuation
            if i + 1 < search_range.len() {
                return Some(i + 1);
            }
        }
    }

    // Third priority: break at word boundaries (after lowercase before uppercase)
    // This helps with camelCase or PascalCase identifiers
    for i in (1..search_range.len()).rev() {
        let chars: Vec<char> = search_range.chars().collect();
        if i < chars.len() - 1 {
            let prev = chars[i - 1];
            let curr = chars[i];
            if prev.is_lowercase() && curr.is_uppercase() {
                return Some(i);
            }
        }
    }

    None
}
