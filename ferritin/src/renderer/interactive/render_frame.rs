use ratatui::{
    Frame,
    layout::{Position, Rect},
};

use super::{InteractiveState, UiMode};
use crate::styled_string::NodePath;

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
            self.render_help_screen(frame.buffer_mut(), help_area);
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

            // Reset layout state for this frame
            self.layout.pos = Position::default();
            self.layout.indent = 0;
            self.layout.node_path = NodePath::new();
            self.layout.area = main_area;

            // Render main document
            self.render_document(main_area, frame.buffer_mut());

            // Render breadcrumb bar with full history
            self.document
                .history
                .render(frame.buffer_mut(), breadcrumb_area, &self.theme);

            // Render status bar
            self.render_status_bar(frame.buffer_mut(), status_area);

            // Render theme picker overlay if in theme picker mode
            if let UiMode::ThemePicker { selected_index, .. } = self.ui_mode {
                let area = frame.area();
                self.render_theme_picker(frame.buffer_mut(), area, selected_index);
            }
        }
    }
}
