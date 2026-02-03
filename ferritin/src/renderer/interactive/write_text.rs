use ratatui::{buffer::Buffer, layout::Rect, style::Style};

use super::state::InteractiveState;

impl<'a> InteractiveState<'a> {
    /// Write text to buffer at position
    pub(super) fn write_text(
        &self,
        buf: &mut Buffer,
        row: u16,
        col: u16,
        text: &str,
        area: Rect,
        style: Style,
    ) {
        if row < self.viewport.scroll_offset || row >= self.viewport.scroll_offset + area.height {
            return; // Outside visible area
        }

        let screen_row = row - self.viewport.scroll_offset;
        let mut current_col = col;

        for ch in text.chars() {
            if current_col >= area.width {
                break; // Past right edge
            }

            // Handle tabs: replace with spaces to avoid column counting mismatches
            // Tabs display as multiple spaces in terminals but count as 1 character
            if ch == '\t' {
                // Write 4 spaces for each tab (Rust convention)
                for _ in 0..4 {
                    if current_col >= area.width {
                        break;
                    }
                    if let Some(cell) = buf.cell_mut((current_col, screen_row)) {
                        cell.set_char(' ');
                        cell.set_style(style);
                    }
                    current_col += 1;
                }
            } else {
                if let Some(cell) = buf.cell_mut((current_col, screen_row)) {
                    cell.set_char(ch);
                    cell.set_style(style);
                }
                current_col += 1;
            }
        }
    }
}
