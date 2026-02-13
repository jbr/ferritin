use ratatui::{buffer::Buffer, layout::Rect};

use super::{
    render_document::BASELINE_LEFT_MARGIN,
    state::{InputMode, InteractiveState, UiMode},
};

impl<'a> InteractiveState<'a> {
    /// Render status bar at the bottom of the screen
    pub(super) fn render_status_bar(&mut self, buf: &mut Buffer, area: Rect) {
        let style = self.theme.status_style;
        let hint_style = self.theme.status_hint_style;

        // Clear the status line with static background
        for x in 0..area.width {
            buf.cell_mut((x, area.y)).unwrap().reset();
            buf.cell_mut((x, area.y)).unwrap().set_style(style);
        }

        // Determine what to display based on UI mode
        let (display_text, hint_text) = match &self.ui_mode {
            UiMode::Normal | UiMode::Help | UiMode::DevLog { .. } | UiMode::ThemePicker { .. } => {
                (self.ui.debug_message.clone(), None)
            }

            _ if self.loading.pending_request => (self.ui.debug_message.clone(), None),

            UiMode::Input(InputMode::GoTo { buffer }) => {
                (format!("Go to: {}", buffer).into(), None)
            }
            UiMode::Input(InputMode::Search {
                buffer, all_crates, ..
            }) => {
                // Get current crate name for search scope display
                let current_crate = self
                    .document
                    .history
                    .current()
                    .and_then(|entry| entry.crate_name());

                let scope = if *all_crates {
                    "all crates".to_string()
                } else {
                    current_crate
                        .map(|c| c.to_string())
                        .unwrap_or_else(|| "current crate".to_string())
                };

                // Only show toggle hint if there's a crate to toggle to
                let hint = if current_crate.is_some() {
                    Some("[tab] toggle scope")
                } else {
                    None
                };

                (format!("Search in {}: {}", scope, buffer).into(), hint)
            }
        };

        // Calculate space for hint text (accounting for left margin)
        let hint_len = hint_text.as_ref().map(|h| h.len()).unwrap_or(0);
        let available_width = (area.width as usize).saturating_sub(BASELINE_LEFT_MARGIN as usize);
        let text_max_width = if hint_len > 0 {
            available_width.saturating_sub(hint_len + 2) // +2 for spacing
        } else {
            available_width
        };

        // Render main text (truncate if needed)
        let truncated = if display_text.len() > text_max_width {
            &display_text[..text_max_width]
        } else {
            &display_text
        };

        let mut col = BASELINE_LEFT_MARGIN;
        for ch in truncated.chars() {
            if col >= area.width {
                break;
            }
            buf.cell_mut((col, area.y))
                .unwrap()
                .set_char(ch)
                .set_style(style);
            col += 1;
        }

        // Render right-justified hint text if present (within margin-adjusted area)
        if let Some(hint) = hint_text {
            let hint_start = area
                .width
                .saturating_sub(hint.len() as u16)
                .max(BASELINE_LEFT_MARGIN);
            let mut hint_col = hint_start;
            for ch in hint.chars() {
                if hint_col >= area.width {
                    break;
                }
                buf.cell_mut((hint_col, area.y))
                    .unwrap()
                    .set_char(ch)
                    .set_style(hint_style);
                hint_col += 1;
            }
        }
    }
}
