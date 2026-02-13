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
        // Always allow Escape to exit help, cancel input mode, or quit
        if key.code == KeyCode::Esc {
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

                // Scroll down
                (KeyCode::Char('j'), _) | (KeyCode::Down, _) => {
                    self.set_scroll_offset(self.viewport.scroll_offset.saturating_add(1));
                }

                // Scroll up
                (KeyCode::Char('k'), _) | (KeyCode::Up, _) => {
                    self.set_scroll_offset(self.viewport.scroll_offset.saturating_sub(1));
                }

                // Page down
                (KeyCode::Char('d'), KeyModifiers::CONTROL) | (KeyCode::PageDown, _) => {
                    let Ok(size) = terminal.size() else {
                        return false;
                    };
                    let page_size = size.height / 2;
                    self.set_scroll_offset(self.viewport.scroll_offset.saturating_add(page_size));
                }

                // Page up
                (KeyCode::Char('u'), KeyModifiers::CONTROL) | (KeyCode::PageUp, _) => {
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
                (KeyCode::Home, _) => {
                    self.set_scroll_offset(0);
                }

                // Jump to bottom (will clamp to actual max)
                (KeyCode::Char('G'), KeyModifiers::SHIFT) | (KeyCode::End, _) => {
                    self.set_scroll_offset(u16::MAX); // Large number, will clamp to actual max
                }

                // Enter GoTo mode
                (KeyCode::Char('g'), _) => {
                    self.ui_mode = UiMode::Input(InputMode::GoTo {
                        buffer: String::new(),
                    });
                }

                // Enter Search mode
                (KeyCode::Char('s'), _) => {
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
                (KeyCode::Left, _) => {
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
}
