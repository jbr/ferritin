use std::io::Write;

use crossterm::event::{MouseEvent, MouseEventKind};
use ratatui::{Terminal, layout::Position, prelude::Backend};

use crate::{
    document::TuiAction,
    render_context::RenderContext,
    renderer::interactive::{handle_action, set_cursor_shape},
};

use super::UiMode;

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

                // In ThemePicker mode, use absolute screen coordinates (no scroll offset)
                if matches!(self.ui_mode, super::UiMode::ThemePicker { .. }) {
                    self.viewport.cursor_pos = Some(Position::new(column, row));
                    return;
                }

                let terminal_height = size.height;
                let terminal_width = size.width;
                let content_height = terminal_height.saturating_sub(2); // Exclude 2 status lines
                let content_width = terminal_width.saturating_sub(1); // Exclude scrollbar column
                let breadcrumb_row = terminal_height.saturating_sub(2);

                // Check if hovering over scrollbar
                self.viewport.scrollbar_hovered =
                    row < content_height && column == content_width && self.scrollbar_visible();

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
                self.set_scroll_offset(self.viewport.scroll_offset.saturating_add(1));
            }

            MouseEvent {
                kind: MouseEventKind::ScrollUp,
                ..
            } => {
                self.set_scroll_offset(self.viewport.scroll_offset.saturating_sub(1));
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
                let content_width = size.width.saturating_sub(1); // Exclude scrollbar column
                let breadcrumb_row = terminal_height.saturating_sub(2);

                // Check if click is in scrollbar column
                if row < content_height && column == content_width && self.scrollbar_visible() {
                    // Start scrollbar drag
                    self.viewport.scrollbar_dragging = true;
                    // Calculate scroll position from click Y
                    self.handle_scrollbar_drag(row, content_height);
                } else if row < content_height {
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
                        self.loading.start();
                    }
                }
            }

            MouseEvent {
                kind: MouseEventKind::Drag(_),
                row,
                ..
            } => {
                if self.viewport.scrollbar_dragging {
                    let Ok(size) = terminal.size() else {
                        return;
                    };
                    let content_height = size.height.saturating_sub(2);
                    self.handle_scrollbar_drag(row, content_height);
                }
            }

            MouseEvent {
                kind: MouseEventKind::Up(_),
                ..
            } => {
                if self.viewport.scrollbar_dragging {
                    self.viewport.scrollbar_dragging = false;
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
                // Handle SelectTheme specially (doesn't go through request thread)
                if let TuiAction::SelectTheme(theme_name) = &action {
                    // Apply theme immediately
                    let _ = self.apply_theme(theme_name);

                    // Update selected index in ThemePicker mode
                    if let UiMode::ThemePicker {
                        ref mut selected_index,
                        ..
                    } = self.ui_mode
                    {
                        let themes = RenderContext::available_themes();
                        if let Some(idx) = themes.iter().position(|t| t == theme_name.as_ref()) {
                            *selected_index = idx;
                        }
                    }

                    self.ui.debug_message = format!("Selected theme: {theme_name}").into();
                } else {
                    match handle_action(&mut self.document.document, action) {
                        Some(command) => {
                            // Send command to request thread (non-blocking)
                            let _ = self.cmd_tx.send(command);
                            self.loading.start();
                        }
                        None => {
                            // Action mutated document in place (e.g., ExpandBlock)
                            // Invalidate layout cache
                            self.viewport.cached_layout = None;
                        }
                    }
                }
            }
        }
    }

    pub(super) fn handle_hover(&mut self) {
        if self.loading.pending_request {
            return;
        }

        use super::state::KeyboardCursor;

        // Check keyboard focus first (takes priority per spec)
        match self.viewport.keyboard_cursor {
            KeyboardCursor::Focused { action_index } => {
                if let Some((_, action)) = self.render_cache.actions.get(action_index) {
                    self.ui.debug_message = match action {
                        TuiAction::Navigate { doc_ref, url: _ } => {
                            if let Some(path) = doc_ref.path() {
                                format!("Navigate: {path} (⏎ to activate)").into()
                            } else if let Some(name) = doc_ref.name() {
                                format!("Navigate: {name} (⏎ to activate)").into()
                            } else {
                                "Navigate: <unknown> (⏎ to activate)".into()
                            }
                        }
                        TuiAction::NavigateToPath { path, url: _ } => {
                            format!("Go to: {} (⏎ to activate)", path).into()
                        }
                        TuiAction::ExpandBlock(path) => {
                            format!("Expand: {:?} (⏎ to activate)", path.indices()).into()
                        }
                        TuiAction::OpenUrl(url) => format!("Open: {} (⏎ to activate)", url).into(),
                        TuiAction::SelectTheme(theme_name) => {
                            format!("Preview theme: {} (⏎ to activate)", theme_name).into()
                        }
                    };
                    return; // Keyboard focus takes priority
                }
                // Focused on invalid action_index - fall through to mouse hover
            }
            KeyboardCursor::VirtualTop | KeyboardCursor::VirtualBottom => {
                // Not focused on any link - fall through to mouse hover
            }
        }

        // No keyboard focus (or invalid focus) - show mouse hover or default message
        if self.ui.mouse_enabled {
            if let Some(pos) = self.viewport.cursor_pos {
                if let Some((_, action)) = self
                    .render_cache
                    .actions
                    .iter()
                    .find(|(rect, _)| rect.contains(pos))
                {
                    self.ui.debug_message = match action {
                        TuiAction::Navigate { doc_ref, url: _ } => {
                            if let Some(path) = doc_ref.path() {
                                format!("Navigate: {path}").into()
                            } else if let Some(name) = doc_ref.name() {
                                format!("Navigate: {name}").into()
                            } else {
                                "Navigate: <unknown>".into()
                            }
                        }
                        TuiAction::NavigateToPath { path, url: _ } => {
                            format!("Go to: {}", path).into()
                        }
                        TuiAction::ExpandBlock(path) => {
                            format!("Expand: {:?}", path.indices()).into()
                        }
                        TuiAction::OpenUrl(url) => format!("Open: {}", url).into(),
                        TuiAction::SelectTheme(theme_name) => {
                            format!("Preview theme: {}", theme_name).into()
                        }
                    };
                } else {
                    self.ui.debug_message = format!(
                        "Pos: ({}, {}) | Scroll: {} | Mouse: ON | Source: {}",
                        pos.x,
                        pos.y,
                        self.viewport.scroll_offset,
                        if self.ui.include_source { "ON" } else { "OFF" }
                    )
                    .into();
                }
            }
        } else {
            self.ui.debug_message = format!(
                "Mouse: OFF (text selection enabled - m to re-enable) | Source: {}",
                if self.ui.include_source { "ON" } else { "OFF" }
            )
            .into();
        }
    }

    /// Handle scrollbar drag by calculating scroll position from mouse Y
    fn handle_scrollbar_drag(&mut self, mouse_y: u16, viewport_height: u16) {
        if let Some(cache) = self.viewport.cached_layout {
            let document_height = cache.document_height;

            // Calculate what percentage of the scrollbar was clicked
            let percentage = mouse_y as f32 / viewport_height as f32;

            // Map to scroll range
            let max_scroll = document_height.saturating_sub(viewport_height);
            let target_scroll = (percentage * max_scroll as f32).round() as u16;

            self.set_scroll_offset(target_scroll);
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
                // Check for scrollbar hover/drag
                let scrollbar_hover = self.viewport.scrollbar_hovered;
                let scrollbar_drag = self.viewport.scrollbar_dragging;

                // When not loading, check hover state
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

                let now_hovering = content_hover || breadcrumb_hover || scrollbar_hover;

                // Update cursor only if state changed
                if self.loading.was_loading || now_hovering != self.ui.is_hovering || scrollbar_drag
                {
                    let shape = if scrollbar_drag {
                        "grabbing"
                    } else if scrollbar_hover {
                        "grab"
                    } else if now_hovering {
                        "pointer"
                    } else {
                        "default"
                    };
                    set_cursor_shape(terminal.backend_mut(), shape);
                    self.ui.is_hovering = now_hovering;
                    self.loading.was_loading = false;
                }
            }
        }
    }
}
