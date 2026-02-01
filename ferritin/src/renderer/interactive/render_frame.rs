use ratatui::{Frame, layout::Rect};

use super::{InteractiveState, UiMode, render_document, render_help_screen, render_status_bar};

impl<'a> InteractiveState<'a> {
    pub(super) fn render_frame(&mut self, frame: &mut Frame) {
        self.loading.frame_count = self.loading.frame_count.wrapping_add(1);

        // Reserve last 2 lines for status bars
        let main_area = Rect {
            x: frame.area().x,
            y: frame.area().y,
            width: frame.area().width,
            height: frame.area().height.saturating_sub(2),
        };

        let breadcrumb_area = Rect {
            x: frame.area().x,
            y: frame.area().height.saturating_sub(2),
            width: frame.area().width,
            height: 1,
        };

        let status_area = Rect {
            x: frame.area().x,
            y: frame.area().height.saturating_sub(1),
            width: frame.area().width,
            height: 1,
        };

        if matches!(self.ui_mode, UiMode::Help) {
            // Render help screen (covers entire area including status bars)
            let help_area = frame.area();
            render_help_screen(frame.buffer_mut(), help_area, &self.theme);
        } else {
            // Clear main area with theme background
            for y in 0..main_area.height {
                for x in 0..main_area.width {
                    frame
                        .buffer_mut()
                        .cell_mut((x, y))
                        .unwrap()
                        .set_style(self.theme.document_bg_style);
                }
            }

            // Render main document
            self.render_cache.actions = render_document(
                &self.document.document.nodes,
                &self.ui_config,
                main_area,
                frame.buffer_mut(),
                self.viewport.scroll_offset,
                self.viewport.cursor_pos,
                &self.theme,
            );

            // Render breadcrumb bar with full history
            self.document
                .history
                .render(frame.buffer_mut(), breadcrumb_area, &self.theme);

            // Get current crate name for search scope display
            let current_crate = self
                .document
                .history
                .current()
                .and_then(|entry| entry.crate_name());

            // Render status bar
            render_status_bar(
                frame.buffer_mut(),
                status_area,
                &self.ui.debug_message,
                &self.ui_mode,
                current_crate,
                &self.theme,
                self.loading.pending_request,
                self.loading.frame_count,
            );
        }
    }
}
