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
            match self.ui_mode {
                UiMode::Help => {
                    self.ui_mode = UiMode::Normal;
                }
                UiMode::Input(_) => {
                    self.ui_mode = UiMode::Normal;
                    self.ui.debug_message =
                        "ferritin - q:quit ?:help ←/→:history g:go s:search l:list c:code"
                            .to_string();
                }
                UiMode::ThemePicker {
                    ref saved_theme_name,
                    ..
                } => {
                    // Revert to saved theme on cancel
                    let theme_name = saved_theme_name.clone();
                    self.ui_mode = UiMode::Normal;
                    let _ = self.apply_theme(&theme_name);
                    self.ui.debug_message =
                        "ferritin - q:quit ?:help ←/→:history g:go s:search l:list c:code"
                            .to_string();
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
                    // Toggle search scope (only in Search mode)
                    if let InputMode::Search { all_crates, .. } = input_mode {
                        *all_crates = !*all_crates;
                    }
                }
                KeyCode::Enter => {
                    // Execute the command based on current input mode
                    let command = match input_mode {
                        InputMode::GoTo { buffer } => {
                            self.ui.debug_message = format!("Loading: {}...", buffer);
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
                                    .map(|s| Cow::Owned(s.to_string()))
                            };

                            self.ui.debug_message = format!("Searching: {}...", buffer);
                            Some(UiCommand::Search {
                                query: Cow::Owned(buffer.clone()),
                                crate_name: search_crate,
                                limit: 20,
                            })
                        }
                    };

                    if let Some(cmd) = command {
                        let _ = self.cmd_tx.send(cmd);
                        self.loading.pending_request = true;
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
                        .unwrap_or_else(|| "default".to_string());
                    self.ui_mode = UiMode::Normal;
                    self.ui.debug_message = format!("Theme saved: {}", theme_name);
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
                    self.viewport.scroll_offset = self.viewport.scroll_offset.saturating_add(1);
                }

                // Scroll up
                (KeyCode::Char('k'), _) | (KeyCode::Up, _) => {
                    self.viewport.scroll_offset = self.viewport.scroll_offset.saturating_sub(1);
                }

                // Page down
                (KeyCode::Char('d'), KeyModifiers::CONTROL) | (KeyCode::PageDown, _) => {
                    let Ok(size) = terminal.size() else {
                        return false;
                    };
                    let page_size = size.height / 2;
                    self.viewport.scroll_offset =
                        self.viewport.scroll_offset.saturating_add(page_size);
                }

                // Page up
                (KeyCode::Char('u'), KeyModifiers::CONTROL) | (KeyCode::PageUp, _) => {
                    let Ok(size) = terminal.size() else {
                        return false;
                    };
                    let page_size = size.height / 2;
                    self.viewport.scroll_offset =
                        self.viewport.scroll_offset.saturating_sub(page_size);
                }

                // Jump to top
                (KeyCode::Home, _) => {
                    self.viewport.scroll_offset = 0;
                }

                // Jump to bottom (will clamp in render)
                (KeyCode::Char('G'), KeyModifiers::SHIFT) | (KeyCode::End, _) => {
                    self.viewport.scroll_offset = 10000; // Large number, will clamp
                }

                // Enter GoTo mode
                (KeyCode::Char('g'), _) => {
                    self.ui_mode = UiMode::Input(InputMode::GoTo {
                        buffer: String::new(),
                    });
                }

                // Enter Search mode
                (KeyCode::Char('s'), _) => {
                    self.ui_mode = UiMode::Input(InputMode::Search {
                        buffer: String::new(),
                        all_crates: false, // Default to current crate
                    });
                }

                // Show list of crates
                (KeyCode::Char('l'), _) => {
                    // Send List command to request thread (non-blocking)
                    let _ = self.cmd_tx.send(UiCommand::List);
                    self.loading.pending_request = true;
                    self.ui.debug_message = "Loading crate list...".to_string();
                }

                // Toggle mouse mode for text selection
                (KeyCode::Char('m'), _) => {
                    self.ui.mouse_enabled = !self.ui.mouse_enabled;
                    if self.ui.mouse_enabled {
                        let _ = execute!(terminal.backend_mut(), EnableMouseCapture);
                        self.ui.debug_message = "Mouse enabled (hover/click)".to_string();
                    } else {
                        let _ = execute!(terminal.backend_mut(), DisableMouseCapture);
                        self.viewport.cursor_pos = None; // Clear cursor position
                        self.ui.debug_message =
                            "Mouse disabled (text selection enabled)".to_string();
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
                        "Source code display enabled".to_string()
                    } else {
                        "Source code display disabled".to_string()
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
                        "Select theme (↑/↓ to navigate, Enter to save, Esc to cancel)".to_string();
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
                        self.loading.pending_request = true;
                        self.ui.debug_message = format!("Loading: {}...", entry.display_name());
                    } else {
                        self.ui.debug_message = "Already at beginning of history".to_string();
                    }
                }

                // Navigate forward
                (KeyCode::Right, _) => {
                    if let Some(entry) = self.document.history.go_forward() {
                        // Send command from history entry (non-blocking)
                        let _ = self.cmd_tx.send(entry.to_command());
                        self.loading.pending_request = true;
                        self.ui.debug_message = format!("Loading: {}...", entry.display_name());
                    } else {
                        self.ui.debug_message = "Already at end of history".to_string();
                    }
                }

                _ => { /*unhandled key event*/ }
            }
        }
        false
    }
}
