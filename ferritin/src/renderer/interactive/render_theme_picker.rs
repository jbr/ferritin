use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Widget},
};
use std::borrow::Cow;

use super::state::InteractiveState;
use crate::render_context::RenderContext;
use crate::styled_string::TuiAction;

impl<'a> InteractiveState<'a> {
    /// Render theme picker modal overlay
    pub(super) fn render_theme_picker(
        &mut self,
        buf: &mut Buffer,
        area: Rect,
        selected_index: usize,
    ) {
        // Clear document actions - modal should block all background interactions
        self.render_cache.actions.clear();

        // Calculate centered modal area (60% width, 70% height)
        let modal_area = centered_rect(60, 70, area);

        // Clear the area for the modal
        Clear.render(modal_area, buf);

        // Get available themes
        let themes = RenderContext::available_themes();

        // Calculate where List widget will render items
        // Block with borders: inner area starts at y + 1 (after top border)
        // List items are rendered sequentially starting from inner area
        let list_inner_y = modal_area.y + 1;

        // Register clickable actions for each theme item
        for (i, theme_name) in themes.iter().enumerate() {
            let item_y = list_inner_y + i as u16;
            if item_y < modal_area.y + modal_area.height.saturating_sub(1) {
                let item_rect = Rect {
                    x: modal_area.x + 1, // Inner area x (after left border)
                    y: item_y,
                    width: modal_area.width.saturating_sub(2), // Inner area width
                    height: 1,
                };
                self.render_cache.actions.push((
                    item_rect,
                    TuiAction::SelectTheme(Cow::Owned(theme_name.clone())),
                ));
            }
        }

        // Create list items from theme names
        let items: Vec<ListItem> = themes
            .iter()
            .map(|theme_name| ListItem::new(Line::from(format!("  {}", theme_name))))
            .collect();

        // Create list state for selection
        let mut list_state = ListState::default();
        list_state.select(Some(selected_index));

        // Create block with title and borders
        let block = Block::default()
            .title(" Select Theme ")
            .borders(Borders::ALL)
            .style(self.theme.help_bg_style);

        // Create list widget with highlighting
        let list = List::new(items)
            .block(block)
            .highlight_style(
                Style::default()
                    .bg(self
                        .theme
                        .breadcrumb_style
                        .bg
                        .unwrap_or(ratatui::style::Color::Blue))
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");

        // Render the list
        ratatui::widgets::StatefulWidget::render(list, modal_area, buf, &mut list_state);

        // Apply hover styling if mouse is over a different item than selected
        if let Some(cursor_pos) = self.viewport.cursor_pos {
            // Check if cursor is within the modal
            if modal_area.contains(cursor_pos) {
                // Calculate which item is being hovered
                let relative_y = cursor_pos.y.saturating_sub(list_inner_y);
                let hovered_index = relative_y as usize;

                // Only highlight if it's a valid item and different from selected
                if hovered_index < themes.len() && hovered_index != selected_index {
                    let hover_y = list_inner_y + hovered_index as u16;

                    // Draw hover marker (^) in left margin
                    let marker_x = modal_area.x + 1;
                    if let Some(cell) = buf.cell_mut((marker_x, hover_y)) {
                        cell.set_char('>');
                    }

                    // Make the hovered row text bold
                    for x in (modal_area.x + 1)..(modal_area.x + modal_area.width - 1) {
                        if let Some(cell) = buf.cell_mut((x, hover_y)) {
                            cell.modifier.insert(ratatui::style::Modifier::BOLD);
                        }
                    }
                }
            }
        }

        // Render instructions at the bottom of the modal
        let instruction_y = modal_area.y + modal_area.height.saturating_sub(2);
        if instruction_y < area.height {
            let instructions = " ↑/↓:Navigate  Enter:Save  Esc:Cancel ";
            let instruction_x =
                modal_area.x + (modal_area.width.saturating_sub(instructions.len() as u16)) / 2;

            for (i, ch) in instructions.chars().enumerate() {
                let x = instruction_x + i as u16;
                if x < modal_area.x + modal_area.width {
                    if let Some(cell) = buf.cell_mut((x, instruction_y)) {
                        cell.set_char(ch);
                        cell.set_style(self.theme.status_hint_style);
                    }
                }
            }
        }
    }
}

/// Helper function to create a centered rect using up certain percentage of the available rect
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(r);

    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(popup_layout[1])[1]
}
