use std::io::Write;

use crossterm::event::{MouseEvent, MouseEventKind};
use ratatui::{Terminal, layout::Position, prelude::Backend};

use crate::{
    renderer::interactive::{handle_action, set_cursor_shape},
    styled_string::TuiAction,
};

impl<'a> super::InteractiveState<'a> {
    pub(super) fn handle_mouse_event(
        &mut self,
        mouse_event: MouseEvent,
        terminal: &Terminal<impl Backend>,
    ) {
        if !self.ui.mouse_enabled {
            return;
        }

        match mouse_event {
            MouseEvent {
                kind: MouseEventKind::Moved,
                column,
                row,
                ..
            } => {
                let Ok(size) = terminal.size() else {
                    return;
                };

                let terminal_height = size.height;
                let content_height = terminal_height.saturating_sub(2); // Exclude 2 status lines
                let breadcrumb_row = terminal_height.saturating_sub(2);

                if row < content_height {
                    // Mouse in main content area
                    self.viewport.cursor_pos =
                        Some(Position::new(column, row + self.viewport.scroll_offset));
                    self.document.history.clear_hover();
                } else if row == breadcrumb_row {
                    // Mouse over breadcrumb bar
                    self.viewport.cursor_pos = None;
                    self.document
                        .history
                        .handle_hover(Position::new(column, row));
                } else {
                    // Mouse over status bar
                    self.viewport.cursor_pos = None;
                    self.document.history.clear_hover();
                }
            }

            MouseEvent {
                kind: MouseEventKind::ScrollDown,
                ..
            } => {
                self.viewport.scroll_offset = self.viewport.scroll_offset.saturating_add(1);
            }

            MouseEvent {
                kind: MouseEventKind::ScrollUp,
                ..
            } => {
                self.viewport.scroll_offset = self.viewport.scroll_offset.saturating_sub(1);
            }

            MouseEvent {
                kind: MouseEventKind::Down(_),
                column,
                row,
                ..
            } => {
                let Ok(size) = terminal.size() else {
                    return;
                };

                let terminal_height = size.height;
                let content_height = terminal_height.saturating_sub(2); // Exclude 2 status lines
                let breadcrumb_row = terminal_height.saturating_sub(2);

                if row < content_height {
                    // Click in main content area
                    self.viewport.clicked_position =
                        Some(Position::new(column, row + self.viewport.scroll_offset));
                } else if row == breadcrumb_row {
                    // Click on breadcrumb bar
                    if let Some(entry) = self
                        .document
                        .history
                        .handle_click(Position::new(column, row))
                    {
                        // Send command from history entry (non-blocking)
                        let _ = self.cmd_tx.send(entry.to_command());
                        self.loading.pending_request = true;
                        self.ui.debug_message = format!("Loading: {}...", entry.display_name());
                    }
                }
            }
            _ => { /*unhandled*/ }
        }
    }

    pub(super) fn handle_click(&mut self) {
        // Handle any clicked action from previous iteration
        if let Some(click_pos) = self.viewport.clicked_position.take() {
            let action_opt = self
                .render_cache
                .actions
                .iter()
                .find(|(rect, _)| rect.contains(click_pos))
                .map(|(_, action)| action.clone());

            if let Some(action) = action_opt {
                self.ui.debug_message = match &action {
                    TuiAction::Navigate(doc_ref) => format!(
                        "Clicked: {}",
                        doc_ref
                            .path()
                            .map(|p| p.to_string())
                            .unwrap_or_else(|| "unknown".to_string())
                    ),
                    TuiAction::NavigateToPath(path) => format!("Clicked: {}", path),
                    TuiAction::ExpandBlock(path) => format!("Clicked: {:?}", path.indices()),
                    TuiAction::OpenUrl(url) => format!("Clicked: {}", url),
                };

                // Handle the action - may return a command to send
                if let Some(command) = handle_action(&mut self.document.document, action) {
                    // Send command to request thread (non-blocking)
                    let _ = self.cmd_tx.send(command);
                    self.loading.pending_request = true;
                    self.ui.debug_message = "Loading...".to_string();
                }
            }
        }
    }

    pub(super) fn handle_hover(&mut self) {
        // Update debug message with hover info
        if self.ui.mouse_enabled {
            if let Some(pos) = self.viewport.cursor_pos {
                if let Some((_, action)) = self
                    .render_cache
                    .actions
                    .iter()
                    .find(|(rect, _)| rect.contains(pos))
                {
                    self.ui.debug_message = match action {
                        TuiAction::Navigate(doc_ref) => {
                            if let Some(path) = doc_ref.path() {
                                format!("Navigate: {}", path)
                            } else if let Some(name) = doc_ref.name() {
                                format!("Navigate: {}", name)
                            } else {
                                "Navigate: <unknown>".to_string()
                            }
                        }
                        TuiAction::NavigateToPath(path) => {
                            format!("Go to: {}", path)
                        }
                        TuiAction::ExpandBlock(path) => {
                            format!("Expand: {:?}", path.indices())
                        }
                        TuiAction::OpenUrl(url) => {
                            format!("Open: {}", url)
                        }
                    };
                } else {
                    self.ui.debug_message = format!(
                        "Pos: ({}, {}) | Scroll: {} | Mouse: ON | Source: {}",
                        pos.x,
                        pos.y,
                        self.viewport.scroll_offset,
                        if self.ui.include_source { "ON" } else { "OFF" }
                    );
                }
            }
        } else {
            self.ui.debug_message = format!(
                "Mouse: OFF (text selection enabled - m to re-enable) | Source: {}",
                if self.ui.include_source { "ON" } else { "OFF" }
            );
        }
    }

    pub(super) fn update_cursor(&mut self, terminal: &mut Terminal<impl Backend + Write>) {
        // Update cursor shape based on loading and hover state
        if self.ui.supports_cursor {
            if self.loading.pending_request {
                // Loading takes precedence - show wait cursor
                if !self.loading.was_loading {
                    set_cursor_shape(terminal.backend_mut(), "wait");
                    self.loading.was_loading = true;
                }
            } else {
                // When not loading, check hover self
                let content_hover = self
                    .viewport
                    .cursor_pos
                    .map(|pos| {
                        self.render_cache
                            .actions
                            .iter()
                            .any(|(rect, _)| rect.contains(pos))
                    })
                    .unwrap_or(false);

                let breadcrumb_hover = self.document.history.is_hovering();

                let now_hovering = content_hover || breadcrumb_hover;

                // Update cursor only if self changed
                if self.loading.was_loading || now_hovering != self.ui.is_hovering {
                    let shape = if now_hovering { "pointer" } else { "default" };
                    set_cursor_shape(terminal.backend_mut(), shape);
                    self.ui.is_hovering = now_hovering;
                    self.loading.was_loading = false;
                }
            }
        }
    }
}
