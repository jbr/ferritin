use ratatui::{
    Frame,
    layout::{Position, Rect},
};

use super::{InteractiveState, UiMode};
use crate::document::NodePath;

impl<'a> InteractiveState<'a> {
    pub(super) fn render_frame(&mut self, frame: &mut Frame) {
        // Reserve last 2 lines for status bars and rightmost column for scrollbar
        let main_area = Rect {
            x: frame.area().x,
            y: frame.area().y,
            width: frame.area().width.saturating_sub(1), // Reserve rightmost column for scrollbar
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
            self.render_help_screen(frame.buffer_mut(), help_area);
        } else {
            // Normal mode or DevLog mode - both render self.document.document
            // (DevLog has already swapped in its document)
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

            // Store viewport height for scroll clamping
            self.viewport.last_viewport_height = main_area.height;

            // Reset layout state for this frame
            self.layout.pos = Position::default();
            self.layout.indent = 0;
            self.layout.node_path = NodePath::new();
            self.layout.area = main_area;

            // Render main document (will update cache if needed)
            self.render_document(main_area, frame.buffer_mut());

            // Render breadcrumb bar or loading animation
            if self.loading.pending_request {
                // Show loading animation in breadcrumb area
                self.render_loading_bar(frame.buffer_mut(), breadcrumb_area);
            } else {
                // Show normal breadcrumb/history bar
                self.document
                    .history
                    .render(frame.buffer_mut(), breadcrumb_area, &self.theme);
            }

            // Render status bar
            self.render_status_bar(frame.buffer_mut(), status_area);

            // Render scrollbar if we have cached layout information
            if let Some(layout_cache) = self.viewport.cached_layout {
                self.render_scrollbar(frame.buffer_mut(), main_area, layout_cache.document_height);
            }

            // Render theme picker overlay if in theme picker mode
            if let UiMode::ThemePicker { selected_index, .. } = self.ui_mode {
                let area = frame.area();
                self.render_theme_picker(frame.buffer_mut(), area, selected_index);
            }
        }
    }
}
