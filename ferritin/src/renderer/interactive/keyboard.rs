use std::{borrow::Cow, io::Write};

use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture, KeyCode, KeyEvent, KeyModifiers},
    execute,
};
use ratatui::{Terminal, prelude::Backend};

use super::{InputMode, InteractiveState, UiMode, channels::UiCommand};
use crate::render_context::RenderContext;

impl<'a> InteractiveState<'a> {
    pub(crate) fn handle_key_event(
        &mut self,
        key: KeyEvent,
        terminal: &mut Terminal<impl Backend + Write>,
    ) -> bool {
        // Always allow Escape (or C-g) to exit help, cancel input mode, or quit
        if key.code == KeyCode::Esc
            || (key.code == KeyCode::Char('g') && key.modifiers == KeyModifiers::CONTROL)
        {
            match std::mem::replace(&mut self.ui_mode, UiMode::Normal) {
                UiMode::Help => {
                    // Already set to Normal by replace
                }
                UiMode::DevLog {
                    previous_document,
                    previous_scroll,
                } => {
                    // Restore previous state
                    self.document.document = previous_document;
                    self.set_scroll_offset(previous_scroll);
                }
                UiMode::Input(_) => {
                    // Already set to Normal by replace
                    self.ui.debug_message =
                        "ferritin - q:quit ?:help ←/→:history g:go s:search l:list c:code".into();
                }
                UiMode::ThemePicker {
                    saved_theme_name, ..
                } => {
                    // Already set to Normal by replace, just revert theme
                    let _ = self.apply_theme(&saved_theme_name);
                    self.ui.debug_message =
                        "ferritin - q:quit ?:help ←/→:history g:go s:search l:list c:code".into();
                }
                UiMode::Normal => {
                    return true;
                }
            }
        } else if matches!(self.ui_mode, UiMode::Help) {
            // Any key (except Escape, handled above) exits help
            self.ui_mode = UiMode::Normal;
        } else if let UiMode::Input(ref mut input_mode) = self.ui_mode {
            match key.code {
                KeyCode::Char(c) => match input_mode {
                    InputMode::GoTo { buffer } => buffer.push(c),
                    InputMode::Search { buffer, .. } => buffer.push(c),
                },
                KeyCode::Backspace => match input_mode {
                    InputMode::GoTo { buffer } => {
                        buffer.pop();
                    }
                    InputMode::Search { buffer, .. } => {
                        buffer.pop();
                    }
                },
                KeyCode::Tab => {
                    // Toggle search scope (only in Search mode and only if there's a crate to scope to)
                    if let InputMode::Search { all_crates, .. } = input_mode {
                        // Only allow toggling if there's actually a current crate
                        let has_crate = self
                            .document
                            .history
                            .current()
                            .and_then(|entry| entry.crate_name())
                            .is_some();
                        if has_crate {
                            *all_crates = !*all_crates;
                        }
                    }
                }
                KeyCode::Enter => {
                    // Execute the command based on current input mode
                    let command = match input_mode {
                        InputMode::GoTo { buffer } => {
                            self.ui.debug_message = format!("Loading: {buffer}...").into();
                            Some(UiCommand::NavigateToPath(Cow::Owned(buffer.clone())))
                        }
                        InputMode::Search { buffer, all_crates } => {
                            // Determine search scope
                            let search_crate = if *all_crates {
                                None
                            } else {
                                self.document
                                    .history
                                    .current()
                                    .and_then(|entry| entry.crate_name())
                                    .map(|s| Cow::Owned(s.into()))
                            };

                            self.ui.debug_message = format!("Searching: {buffer}...").into();
                            Some(UiCommand::Search {
                                query: Cow::Owned(buffer.clone()),
                                crate_name: search_crate,
                                limit: 20,
                            })
                        }
                    };

                    if let Some(cmd) = command {
                        let _ = self.cmd_tx.send(cmd);
                        self.loading.start();
                    }
                    self.ui_mode = UiMode::Normal;
                }
                _ => {}
            }
        } else if let UiMode::ThemePicker {
            ref mut selected_index,
            ..
        } = self.ui_mode
        {
            // Theme picker mode keybindings
            let themes = RenderContext::available_themes();
            let theme_count = themes.len();

            match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    // Move selection up
                    if *selected_index > 0 {
                        *selected_index -= 1;
                        // Apply theme immediately for preview
                        if let Some(theme_name) = themes.get(*selected_index) {
                            let _ = self.apply_theme(theme_name);
                        }
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    // Move selection down
                    if *selected_index + 1 < theme_count {
                        *selected_index += 1;
                        // Apply theme immediately for preview
                        if let Some(theme_name) = themes.get(*selected_index) {
                            let _ = self.apply_theme(theme_name);
                        }
                    }
                }
                KeyCode::Enter => {
                    // Save current theme and exit
                    let theme_name = self
                        .current_theme_name
                        .clone()
                        .unwrap_or_else(|| "default".into());
                    self.ui_mode = UiMode::Normal;
                    self.ui.debug_message = format!("Theme saved: {theme_name}").into();
                }
                _ => {}
            }
        } else {
            // Normal mode keybindings
            match (key.code, key.modifiers) {
                // Quit
                (KeyCode::Char('q'), _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                    return true;
                }

                // Navigate down / scroll down
                (KeyCode::Char('j'), _)
                | (KeyCode::Down, _)
                | (KeyCode::Char('n'), KeyModifiers::CONTROL) => {
                    self.handle_navigate_down();
                }

                // Navigate up / scroll up
                (KeyCode::Char('k'), _)
                | (KeyCode::Up, _)
                | (KeyCode::Char('p'), KeyModifiers::CONTROL) => {
                    self.handle_navigate_up();
                }

                // Activate focused link
                (KeyCode::Enter, _) | (KeyCode::Char(' '), _) => {
                    self.handle_activate_focused_link();
                }

                // Page down
                (KeyCode::Char('d'), KeyModifiers::CONTROL)
                | (KeyCode::Char('v'), KeyModifiers::CONTROL)
                | (KeyCode::PageDown, _) => {
                    let Ok(size) = terminal.size() else {
                        return false;
                    };
                    let page_size = size.height / 2;
                    self.set_scroll_offset(self.viewport.scroll_offset.saturating_add(page_size));
                }

                // Page up
                (KeyCode::Char('u'), KeyModifiers::CONTROL)
                | (KeyCode::Char('v'), KeyModifiers::ALT)
                | (KeyCode::PageUp, _) => {
                    let Ok(size) = terminal.size() else {
                        return false;
                    };
                    let page_size = size.height / 2;
                    self.set_scroll_offset(self.viewport.scroll_offset.saturating_sub(page_size));
                }

                // Dump logs to disk (undocumented debug feature)
                (KeyCode::Char('l'), KeyModifiers::ALT) => match self.dump_logs_to_disk() {
                    Ok(filename) => {
                        self.ui.debug_message = format!("Logs saved to {}", filename).into();
                    }
                    Err(e) => {
                        self.ui.debug_message = format!("Failed to save logs: {}", e).into();
                    }
                },

                // Toggle dev log (undocumented debug feature)
                (KeyCode::Char('l'), KeyModifiers::CONTROL) => {
                    match std::mem::replace(&mut self.ui_mode, UiMode::Normal) {
                        UiMode::DevLog {
                            previous_document,
                            previous_scroll,
                        } => {
                            // Exiting dev log - restore previous state
                            self.document.document = previous_document;
                            self.set_scroll_offset(previous_scroll);
                        }
                        UiMode::Normal => {
                            // Entering dev log - swap in dev log document
                            let dev_log_doc = self.create_dev_log_document();
                            let previous_document =
                                std::mem::replace(&mut self.document.document, dev_log_doc);
                            let previous_scroll = self.viewport.scroll_offset;
                            self.set_scroll_offset(0);
                            self.ui_mode = UiMode::DevLog {
                                previous_document,
                                previous_scroll,
                            };
                        }
                        other => {
                            // Was in a different mode, restore it
                            self.ui_mode = other;
                        }
                    }
                }

                // Jump to top
                (KeyCode::Home, _) | (KeyCode::Char('<'), KeyModifiers::ALT) => {
                    self.set_scroll_offset(0);
                }

                // Jump to bottom (will clamp to actual max)
                (KeyCode::Char('G'), KeyModifiers::SHIFT)
                | (KeyCode::End, _)
                | (KeyCode::Char('>'), KeyModifiers::ALT) => {
                    self.set_scroll_offset(u16::MAX); // Large number, will clamp to actual max
                }

                // Enter GoTo mode
                (KeyCode::Char('g'), _) => {
                    self.ui_mode = UiMode::Input(InputMode::GoTo {
                        buffer: String::new(),
                    });
                }

                // Enter Search mode
                (KeyCode::Char('s'), _) | (KeyCode::Char('/'), _) => {
                    // Default to current crate only if there is one
                    let has_crate = self
                        .document
                        .history
                        .current()
                        .and_then(|entry| entry.crate_name())
                        .is_some();

                    self.ui_mode = UiMode::Input(InputMode::Search {
                        buffer: String::new(),
                        all_crates: !has_crate, // Search all crates if no current crate
                    });
                }

                // Show list of crates
                (KeyCode::Char('l'), _) => {
                    // Send List command to request thread (non-blocking)
                    let _ = self.cmd_tx.send(UiCommand::List);
                    self.loading.start();
                    self.ui.debug_message = "Loading crate list...".into();
                }

                // Toggle mouse mode for text selection
                (KeyCode::Char('m'), _) => {
                    self.ui.mouse_enabled = !self.ui.mouse_enabled;
                    if self.ui.mouse_enabled {
                        let _ = execute!(terminal.backend_mut(), EnableMouseCapture);
                        self.ui.debug_message = "Mouse enabled (hover/click)".into();
                    } else {
                        let _ = execute!(terminal.backend_mut(), DisableMouseCapture);
                        self.viewport.cursor_pos = None; // Clear cursor position
                        self.ui.debug_message = "Mouse disabled (text selection enabled)".into();
                    }
                }

                // Toggle source code display
                (KeyCode::Char('c'), _) => {
                    self.ui.include_source = !self.ui.include_source;
                    // Send command to request thread to update FormatContext
                    let _ = self.cmd_tx.send(UiCommand::ToggleSource {
                        include_source: self.ui.include_source,
                        current_item: self.document.history.current().and_then(|e| e.item()),
                    });
                    self.ui.debug_message = if self.ui.include_source {
                        "Source code display enabled".into()
                    } else {
                        "Source code display disabled".into()
                    };
                }

                // Enter theme picker mode
                (KeyCode::Char('t'), _) => {
                    let themes = RenderContext::available_themes();
                    let current_theme = self
                        .current_theme_name
                        .clone()
                        .or_else(|| themes.first().cloned())
                        .unwrap_or_else(|| "default".to_string());

                    let selected_index =
                        themes.iter().position(|t| t == &current_theme).unwrap_or(0);

                    self.ui_mode = UiMode::ThemePicker {
                        selected_index,
                        saved_theme_name: current_theme,
                    };
                    self.ui.debug_message =
                        "Select theme (↑/↓ to navigate, Enter to save, Esc to cancel)".into();
                }

                // Show help
                (KeyCode::Char('?'), _) | (KeyCode::Char('h'), _) => {
                    self.ui_mode = UiMode::Help;
                }

                // Navigate back
                (KeyCode::Left, _) | (KeyCode::Backspace, _) => {
                    if let Some(entry) = self.document.history.go_back() {
                        // Send command from history entry (non-blocking)
                        let _ = self.cmd_tx.send(entry.to_command());
                        self.loading.start();
                        self.ui.debug_message =
                            format!("Loading: {}...", entry.display_name()).into();
                    } else {
                        self.ui.debug_message = "Already at beginning of history".into();
                    }
                }

                // Navigate forward
                (KeyCode::Right, _) => {
                    if let Some(entry) = self.document.history.go_forward() {
                        // Send command from history entry (non-blocking)
                        let _ = self.cmd_tx.send(entry.to_command());
                        self.loading.start();
                        self.ui.debug_message =
                            format!("Loading: {}...", entry.display_name()).into();
                    } else {
                        self.ui.debug_message = "Already at end of history".into();
                    }
                }

                _ => { /*unhandled key event*/ }
            }
        }
        false
    }

    /// Handle j/↓ key: navigate to next link or scroll down
    ///
    /// Implements seamless transition between link navigation and scrolling:
    /// - When no link is focused (VirtualTop): Focus first visible link, or scroll if none
    /// - When a link is focused and visible: Move to next visible link, or scroll to reveal more
    /// - When scrolling reveals new links below: Automatically focus them (auto-focus behavior)
    /// - When focused link is off-screen above (scrolled past): Re-enter by focusing first visible
    /// - When focused link is off-screen below: Scroll towards it, focusing links as they appear
    /// - When at bottom with no more links: Enter VirtualBottom state
    ///
    /// The auto-focus behavior ensures continuous j/k presses smoothly navigate through
    /// all links in the document without the user needing to manually focus each new link.
    fn handle_navigate_down(&mut self) {
        use super::state::KeyboardCursor;

        match self.viewport.keyboard_cursor {
            KeyboardCursor::VirtualTop => {
                // Try to focus first visible link
                if let Some(first_idx) = self.first_visible_link() {
                    self.viewport.keyboard_cursor = KeyboardCursor::Focused {
                        action_index: first_idx,
                    };
                } else {
                    // No visible links, just scroll down
                    let new_offset = self.viewport.scroll_offset.saturating_add(1);
                    self.set_scroll_offset(new_offset);
                }
            }
            KeyboardCursor::Focused { action_index } => {
                // Check if focused link is off-screen
                if let Some(is_above) = self.is_link_off_screen(action_index) {
                    if is_above {
                        // Focused link is above viewport - re-enter from top
                        // Moving away from focus, so immediately select first visible
                        if let Some(first_idx) = self.first_visible_link() {
                            self.viewport.keyboard_cursor = KeyboardCursor::Focused {
                                action_index: first_idx,
                            };
                        }
                    } else {
                        // Focused link is below viewport - scroll down towards it
                        // Moving towards focus, so scroll and focus new links if they appear
                        self.set_scroll_offset(self.viewport.scroll_offset.saturating_add(1));
                        // Focus first visible link (moving towards the off-screen focus)
                        if let Some(first_idx) = self.first_visible_link() {
                            self.viewport.keyboard_cursor = KeyboardCursor::Focused {
                                action_index: first_idx,
                            };
                        }
                    }
                } else {
                    // Focused link is visible - try to move to next visible link
                    if let Some(next_idx) = self.next_visible_link(action_index) {
                        self.viewport.keyboard_cursor = KeyboardCursor::Focused {
                            action_index: next_idx,
                        };
                    } else {
                        // No more visible links below - scroll down and watch for new links
                        let old_offset = self.viewport.scroll_offset;
                        self.set_scroll_offset(old_offset.saturating_add(1));

                        // If we actually scrolled, check if a new link appeared
                        if self.viewport.scroll_offset > old_offset {
                            // Check if there's now a next visible link
                            if let Some(next_idx) = self.next_visible_link(action_index) {
                                // Auto-focus the newly visible link
                                self.viewport.keyboard_cursor = KeyboardCursor::Focused {
                                    action_index: next_idx,
                                };
                            }
                            // Otherwise keep current focus (will scroll off-screen eventually)
                        } else {
                            // Couldn't scroll (at bottom) - enter virtual bottom state
                            self.viewport.keyboard_cursor = KeyboardCursor::VirtualBottom;
                        }
                    }
                }
            }
            KeyboardCursor::VirtualBottom => {
                // Can't go further down
            }
        }
    }

    /// Handle k/↑ key: navigate to previous link or scroll up
    ///
    /// Mirror of handle_navigate_down, moving upward through the document:
    /// - When no link is focused (VirtualTop): Do nothing (can't go above virtual top)
    /// - When a link is focused and visible: Move to previous visible link, or scroll to reveal more
    /// - When scrolling reveals new links above: Automatically focus them (auto-focus behavior)
    /// - When focused link is off-screen below (scrolled past): Re-enter by focusing last visible
    /// - When focused link is off-screen above: Scroll towards it, focusing links as they appear
    /// - When at top with no more links: Enter VirtualTop state
    /// - From VirtualBottom: Focus last visible link, or scroll if none
    fn handle_navigate_up(&mut self) {
        use super::state::KeyboardCursor;

        match self.viewport.keyboard_cursor {
            KeyboardCursor::VirtualTop => {
                // Can't go higher than virtual top
            }
            KeyboardCursor::Focused { action_index } => {
                // Check if focused link is off-screen
                if let Some(is_above) = self.is_link_off_screen(action_index) {
                    if is_above {
                        // Focused link is above viewport - scroll up towards it
                        // Moving towards focus, so scroll and focus new links if they appear
                        self.set_scroll_offset(self.viewport.scroll_offset.saturating_sub(1));
                        // Focus last visible link (moving towards the off-screen focus)
                        if let Some(last_idx) = self.last_visible_link() {
                            self.viewport.keyboard_cursor = KeyboardCursor::Focused {
                                action_index: last_idx,
                            };
                        }
                    } else {
                        // Focused link is below viewport - re-enter from bottom
                        // Moving away from focus, so immediately select last visible
                        if let Some(last_idx) = self.last_visible_link() {
                            self.viewport.keyboard_cursor = KeyboardCursor::Focused {
                                action_index: last_idx,
                            };
                        }
                    }
                } else {
                    // Focused link is visible - try to move to previous visible link
                    if let Some(prev_idx) = self.prev_visible_link(action_index) {
                        self.viewport.keyboard_cursor = KeyboardCursor::Focused {
                            action_index: prev_idx,
                        };
                    } else {
                        // No more visible links above - scroll up and watch for new links
                        let old_offset = self.viewport.scroll_offset;
                        self.set_scroll_offset(old_offset.saturating_sub(1));

                        // If we actually scrolled, check if a new link appeared
                        if self.viewport.scroll_offset < old_offset {
                            // Check if there's now a prev visible link
                            if let Some(prev_idx) = self.prev_visible_link(action_index) {
                                // Auto-focus the newly visible link
                                self.viewport.keyboard_cursor = KeyboardCursor::Focused {
                                    action_index: prev_idx,
                                };
                            }
                            // Otherwise keep current focus (will scroll off-screen eventually)
                        } else {
                            // Couldn't scroll (at top) - enter virtual top state
                            self.viewport.keyboard_cursor = KeyboardCursor::VirtualTop;
                        }
                    }
                }
            }
            KeyboardCursor::VirtualBottom => {
                // Re-enter from bottom - focus last visible link
                if let Some(last_idx) = self.last_visible_link() {
                    self.viewport.keyboard_cursor = KeyboardCursor::Focused {
                        action_index: last_idx,
                    };
                } else {
                    // No visible links, just scroll up
                    let new_offset = self.viewport.scroll_offset.saturating_sub(1);
                    self.set_scroll_offset(new_offset);
                }
            }
        }
    }

    /// Handle Enter/Space: activate the focused link
    ///
    /// Activates the currently focused link (if any), triggering the same action
    /// as clicking it with the mouse. Navigation actions send a command to the
    /// request thread and reset keyboard focus to VirtualTop for the new document.
    /// ExpandBlock actions mutate the document in place and preserve focus.
    /// Does nothing when in VirtualTop or VirtualBottom states.
    fn handle_activate_focused_link(&mut self) {
        use super::state::KeyboardCursor;

        if let KeyboardCursor::Focused { action_index } = self.viewport.keyboard_cursor {
            if let Some((_, action)) = self.render_cache.actions.get(action_index) {
                let action = action.clone();

                // Handle SelectTheme specially (same as mouse click)
                if let crate::styled_string::TuiAction::SelectTheme(theme_name) = &action {
                    let _ = self.apply_theme(theme_name);
                    if let super::UiMode::ThemePicker {
                        ref mut selected_index,
                        ..
                    } = self.ui_mode
                    {
                        let themes = crate::render_context::RenderContext::available_themes();
                        if let Some(idx) = themes.iter().position(|t| t == theme_name.as_ref()) {
                            *selected_index = idx;
                        }
                    }
                    self.ui.debug_message = format!("Selected theme: {theme_name}").into();
                } else {
                    match super::events::handle_action(&mut self.document.document, action) {
                        Some(command) => {
                            let _ = self.cmd_tx.send(command);
                            self.loading.start();
                            // Reset keyboard cursor on navigation
                            self.viewport.keyboard_cursor = KeyboardCursor::VirtualTop;
                        }
                        None => {
                            // Action mutated document in place (e.g., ExpandBlock)
                            self.viewport.cached_layout = None;
                        }
                    }
                }
            }
        }
    }
}
