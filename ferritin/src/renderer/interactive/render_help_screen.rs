use ratatui::{buffer::Buffer, layout::Rect};

use super::state::InteractiveState;

impl<'a> InteractiveState<'a> {
    /// Render help screen showing all available keybindings
    pub(super) fn render_help_screen(&mut self, buf: &mut Buffer, area: Rect) {
        let bg_style = self.theme.help_bg_style;
        let title_style = self.theme.help_title_style;
        let key_style = self.theme.help_key_style;
        let desc_style = self.theme.help_desc_style;

        // Clear the entire screen
        for y in 0..area.height {
            for x in 0..area.width {
                buf.cell_mut((x, y)).unwrap().reset();
                buf.cell_mut((x, y)).unwrap().set_style(bg_style);
            }
        }

        let help_text = vec![
            ("", "FERRITIN INTERACTIVE MODE - KEYBINDINGS", title_style),
            ("", "", bg_style),
            ("Navigation:", "", title_style),
            ("  j, ↓", "Scroll down", key_style),
            ("  k, ↑", "Scroll up", key_style),
            ("  Ctrl+d, PgDn", "Page down", key_style),
            ("  Ctrl+u, PgUp", "Page up", key_style),
            ("  Home", "Jump to top", key_style),
            ("  Shift+G, End", "Jump to bottom", key_style),
            ("  ←", "Navigate back in history", key_style),
            ("  →", "Navigate forward in history", key_style),
            ("", "", bg_style),
            ("Commands:", "", title_style),
            ("  g", "Go to item by path", key_style),
            ("  s", "Search (scoped to current crate)", key_style),
            (
                "    Tab",
                "  Toggle search scope (current/all crates)",
                key_style,
            ),
            ("  l", "List available crates", key_style),
            ("  c", "Toggle source code display", key_style),
            ("  t", "Select theme", key_style),
            ("  Esc", "Cancel input mode / Exit help / Quit", key_style),
            ("", "", bg_style),
            ("Mouse:", "", title_style),
            ("  m", "Toggle mouse mode (for text selection)", key_style),
            ("  Click", "Navigate to item / Expand block", key_style),
            ("  Hover", "Show preview in status bar", key_style),
            ("  Scroll", "Scroll content", key_style),
            ("", "", bg_style),
            ("Help:", "", title_style),
            ("  ?, h", "Show this help screen", key_style),
            ("", "", bg_style),
            ("Other:", "", title_style),
            ("  q, Ctrl+c", "Quit", key_style),
            ("", "", bg_style),
            ("", "Press any key to close help", desc_style),
        ];

        // Calculate maximum width for consistent formatting
        let max_width = help_text
            .iter()
            .map(|(key, desc, _)| {
                if key.is_empty() {
                    desc.len()
                } else {
                    format!("{:20} {}", key, desc).len()
                }
            })
            .max()
            .unwrap_or(60);

        let start_row = (area.height.saturating_sub(help_text.len() as u16)) / 2;
        let start_col = (area.width.saturating_sub(max_width as u16)) / 2;

        for (i, (key, desc, style)) in help_text.iter().enumerate() {
            let row = start_row + i as u16;
            if row >= area.height {
                break;
            }

            let text = if key.is_empty() {
                format!("{:width$}", desc, width = max_width)
            } else {
                format!("{:20} {:width$}", key, desc, width = max_width - 21)
            };

            let mut col = start_col;
            for ch in text.chars() {
                if col >= area.width {
                    break;
                }
                buf.cell_mut((col, row))
                    .unwrap()
                    .set_char(ch)
                    .set_style(*style);
                col += 1;
            }
        }
    }
}
