mod events;
mod render;
mod state;
mod theme;
mod ui;
mod utils;

use events::handle_action;
use render::render_document;
use theme::InteractiveTheme;

pub use state::HistoryEntry;
use state::InputMode;
use ui::{render_breadcrumb_bar, render_help_screen, render_status_bar};
use utils::{set_cursor_shape, supports_cursor_shape};

use crate::{
    request::Request,
    styled_string::{Document, TuiAction},
};
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers, MouseEvent,
        MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend, layout::Position};
use std::{
    io::{self, stdout},
    time::Duration,
};

/// Render a document in interactive mode with scrolling and hover tracking
pub fn render_interactive<'a>(
    initial_document: &mut Document<'a>,
    request: &'a Request,
    initial_entry: Option<HistoryEntry<'a>>,
) -> io::Result<()> {
    let document = initial_document;

    // Navigation history
    let mut history: Vec<HistoryEntry<'a>> = Vec::new();
    let mut history_index: usize = 0;

    // Initialize history with current entry if provided
    if let Some(entry) = initial_entry {
        history.push(entry);
    }

    // Set up terminal
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    // Build theme once at startup
    let interactive_theme = InteractiveTheme::from_format_context(&request.format_context());

    // Track scroll position and cursor
    let mut scroll_offset = 0u16;
    let mut cursor_pos: Option<(u16, u16)> = None;
    let mut actions = Vec::new();
    let mut clicked_position: Option<Position> = None;
    let mut breadcrumb_clickable_areas: Vec<(usize, std::ops::Range<u16>)> = Vec::new();
    let mut breadcrumb_hover_pos: Option<(u16, u16)> = None;
    let supports_cursor = supports_cursor_shape();
    let mut is_hovering = false;
    let mut mouse_enabled = true;
    let mut debug_message =
        String::from("ferritin - q:quit ?:help ←/→:history g:go s:search l:list c:code");

    // Input mode state
    let mut input_mode = InputMode::Normal;
    let mut input_buffer = String::new();
    let mut search_all_crates = false;
    let mut show_help = false;
    let mut include_source = false;

    // Main event loop
    let result = loop {
        // Ensure format context is synchronized with current toggle state
        request.mutate_format_context(|fc| {
            fc.set_include_source(include_source);
        });

        let _ = terminal.draw(|frame| {
            let format_context = request.format_context();
            // Reserve last 2 lines for status bars
            let main_area = ratatui::layout::Rect {
                x: frame.area().x,
                y: frame.area().y,
                width: frame.area().width,
                height: frame.area().height.saturating_sub(2),
            };
            let breadcrumb_area = ratatui::layout::Rect {
                x: frame.area().x,
                y: frame.area().height.saturating_sub(2),
                width: frame.area().width,
                height: 1,
            };
            let status_area = ratatui::layout::Rect {
                x: frame.area().x,
                y: frame.area().height.saturating_sub(1),
                width: frame.area().width,
                height: 1,
            };

            if show_help {
                // Render help screen (covers entire area including status bars)
                let help_area = frame.area();
                render_help_screen(frame.buffer_mut(), help_area, &interactive_theme);
            } else {
                // Clear main area with theme background
                for y in 0..main_area.height {
                    for x in 0..main_area.width {
                        frame
                            .buffer_mut()
                            .cell_mut((x, y))
                            .unwrap()
                            .set_style(interactive_theme.document_bg_style);
                    }
                }

                // Render main document
                actions = render_document(
                    &document.nodes,
                    &format_context,
                    main_area,
                    frame.buffer_mut(),
                    scroll_offset,
                    cursor_pos,
                    &interactive_theme,
                );

                // Render breadcrumb bar with full history
                breadcrumb_clickable_areas.clear();
                render_breadcrumb_bar(
                    frame.buffer_mut(),
                    breadcrumb_area,
                    &history,
                    history_index,
                    &mut breadcrumb_clickable_areas,
                    breadcrumb_hover_pos,
                    &interactive_theme,
                );

                // Get current crate name for search scope display
                let current_crate = history
                    .get(history_index)
                    .and_then(|entry| entry.crate_name());

                // Render status bar
                render_status_bar(
                    frame.buffer_mut(),
                    status_area,
                    &debug_message,
                    input_mode,
                    &input_buffer,
                    search_all_crates,
                    current_crate,
                    &interactive_theme,
                );
            }
        })?;

        // Update cursor shape based on hover state (both content and breadcrumb)
        if supports_cursor {
            let content_hover = cursor_pos
                .map(|pos| {
                    actions
                        .iter()
                        .any(|(rect, _)| rect.contains(Position::new(pos.0, pos.1)))
                })
                .unwrap_or(false);

            let breadcrumb_hover = breadcrumb_clickable_areas.iter().any(|(_, range)| {
                breadcrumb_hover_pos
                    .map(|(col, _)| range.contains(&col))
                    .unwrap_or(false)
            });

            let now_hovering = content_hover || breadcrumb_hover;

            if now_hovering != is_hovering {
                is_hovering = now_hovering;
                let shape = if is_hovering { "pointer" } else { "default" };
                let _ = set_cursor_shape(terminal.backend_mut(), shape);
            }
        }

        // Update debug message with hover info
        if mouse_enabled {
            if let Some(pos) = cursor_pos {
                if let Some((_, action)) = actions
                    .iter()
                    .find(|(rect, _)| rect.contains(Position::new(pos.0, pos.1)))
                {
                    debug_message = match action {
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
                    debug_message = format!(
                        "Pos: ({}, {}) | Scroll: {} | Mouse: ON | Source: {}",
                        pos.0,
                        pos.1,
                        scroll_offset,
                        if include_source { "ON" } else { "OFF" }
                    );
                }
            }
        } else {
            debug_message = format!(
                "Mouse: OFF (text selection enabled - m to re-enable) | Source: {}",
                if include_source { "ON" } else { "OFF" }
            );
        }

        // Handle any clicked action from previous iteration
        if let Some(click_pos) = clicked_position.take() {
            let action_opt = actions
                .iter()
                .find(|(rect, _)| rect.contains(click_pos))
                .map(|(_, action)| action.clone());

            if let Some(action) = action_opt {
                debug_message = match &action {
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

                if let Some((new_doc, doc_ref)) = handle_action(document, &action, request) {
                    *document = new_doc;
                    scroll_offset = 0; // Reset scroll to top of new document

                    let new_entry = HistoryEntry::Item(doc_ref);
                    // Add to history if not a duplicate of current position
                    if history.is_empty() || history.get(history_index) != Some(&new_entry) {
                        // Truncate history after current position (discard forward history)
                        history.truncate(history_index + 1);
                        // Add new item
                        history.push(new_entry);
                        history_index = history.len() - 1;
                    }

                    debug_message = format!(
                        "Navigated to: {}",
                        doc_ref
                            .path()
                            .map(|p| p.to_string())
                            .unwrap_or_else(|| "?".to_string())
                    );
                }
            }
        }

        // Handle events with timeout for hover updates
        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key) => {
                    // Always allow Escape to exit help, cancel input mode, or quit
                    if key.code == KeyCode::Esc {
                        if show_help {
                            show_help = false;
                        } else if input_mode != InputMode::Normal {
                            input_mode = InputMode::Normal;
                            input_buffer.clear();
                            debug_message =
                                "ferritin - q:quit ?:help ←/→:history g:go s:search l:list c:code"
                                    .to_string();
                        } else {
                            break Ok(());
                        }
                    }
                    // Handle help screen
                    else if show_help {
                        // Any key (except Escape, handled above) exits help
                        show_help = false;
                    }
                    // Handle input mode
                    else if input_mode != InputMode::Normal {
                        match key.code {
                            KeyCode::Char(c) => {
                                input_buffer.push(c);
                            }
                            KeyCode::Backspace => {
                                input_buffer.pop();
                            }
                            KeyCode::Tab => {
                                // Toggle search scope (only in Search mode)
                                if input_mode == InputMode::Search {
                                    search_all_crates = !search_all_crates;
                                }
                            }
                            KeyCode::Enter => {
                                // Execute the command
                                match input_mode {
                                    InputMode::GoTo => {
                                        let mut suggestions = vec![];
                                        if let Some(item) =
                                            request.resolve_path(&input_buffer, &mut suggestions)
                                        {
                                            let doc_nodes = request.format_item(item);
                                            *document = Document::from(doc_nodes);
                                            scroll_offset = 0;

                                            let new_entry = HistoryEntry::Item(item);
                                            // Add to history
                                            if history.is_empty()
                                                || history.get(history_index) != Some(&new_entry)
                                            {
                                                history.truncate(history_index + 1);
                                                history.push(new_entry);
                                                history_index = history.len() - 1;
                                            }

                                            debug_message = format!(
                                                "Navigated to: {}",
                                                item.path()
                                                    .map(|p| p.to_string())
                                                    .unwrap_or_else(|| "?".to_string())
                                            );
                                        } else {
                                            debug_message = format!("Not found: {}", input_buffer);
                                        }
                                    }
                                    InputMode::Search => {
                                        // Determine search scope (clone to avoid borrow issues)
                                        let search_crate = if search_all_crates {
                                            None
                                        } else {
                                            history
                                                .get(history_index)
                                                .and_then(|entry| entry.crate_name())
                                                .map(|s| s.to_string())
                                        };

                                        // Execute search
                                        let (search_doc, is_error) =
                                            crate::commands::search::execute(
                                                request,
                                                &input_buffer,
                                                20, // limit
                                                search_crate.as_deref(),
                                            );
                                        *document = search_doc;
                                        scroll_offset = 0;

                                        if is_error {
                                            debug_message =
                                                format!("No results for: {}", input_buffer);
                                        } else {
                                            // Add search to history
                                            let new_entry = HistoryEntry::Search {
                                                query: input_buffer.clone(),
                                                crate_name: search_crate.clone(),
                                            };

                                            if history.is_empty()
                                                || history.get(history_index) != Some(&new_entry)
                                            {
                                                history.truncate(history_index + 1);
                                                history.push(new_entry);
                                                history_index = history.len() - 1;
                                            }

                                            let scope = if search_all_crates {
                                                "all crates"
                                            } else {
                                                search_crate.as_deref().unwrap_or("current crate")
                                            };
                                            debug_message = format!(
                                                "Search results in {}: {}",
                                                scope, input_buffer
                                            );
                                        }
                                    }
                                    InputMode::Normal => unreachable!(),
                                }
                                input_mode = InputMode::Normal;
                                input_buffer.clear();
                            }
                            _ => {}
                        }
                    }
                    // Normal mode keybindings
                    else {
                        match (key.code, key.modifiers) {
                            // Quit
                            (KeyCode::Char('q'), _)
                            | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                                break Ok(());
                            }

                            // Scroll down
                            (KeyCode::Char('j'), _) | (KeyCode::Down, _) => {
                                scroll_offset = scroll_offset.saturating_add(1);
                            }

                            // Scroll up
                            (KeyCode::Char('k'), _) | (KeyCode::Up, _) => {
                                scroll_offset = scroll_offset.saturating_sub(1);
                            }

                            // Page down
                            (KeyCode::Char('d'), KeyModifiers::CONTROL)
                            | (KeyCode::PageDown, _) => {
                                let page_size = terminal.size()?.height / 2;
                                scroll_offset = scroll_offset.saturating_add(page_size);
                            }

                            // Page up
                            (KeyCode::Char('u'), KeyModifiers::CONTROL) | (KeyCode::PageUp, _) => {
                                let page_size = terminal.size()?.height / 2;
                                scroll_offset = scroll_offset.saturating_sub(page_size);
                            }

                            // Jump to top
                            (KeyCode::Home, _) => {
                                scroll_offset = 0;
                            }

                            // Jump to bottom (will clamp in render)
                            (KeyCode::Char('G'), KeyModifiers::SHIFT) | (KeyCode::End, _) => {
                                scroll_offset = 10000; // Large number, will clamp
                            }

                            // Enter GoTo mode
                            (KeyCode::Char('g'), _) => {
                                input_mode = InputMode::GoTo;
                                input_buffer.clear();
                            }

                            // Enter Search mode
                            (KeyCode::Char('s'), _) => {
                                input_mode = InputMode::Search;
                                input_buffer.clear();
                                search_all_crates = false; // Default to current crate
                            }

                            // Show list of crates
                            (KeyCode::Char('l'), _) => {
                                let (list_doc, _is_error) = crate::commands::list::execute(request);
                                *document = list_doc;
                                scroll_offset = 0;

                                let new_entry = HistoryEntry::List;
                                if history.is_empty()
                                    || history.get(history_index) != Some(&new_entry)
                                {
                                    history.truncate(history_index + 1);
                                    history.push(new_entry);
                                    history_index = history.len() - 1;
                                }

                                debug_message = "List of crates".to_string();
                            }

                            // Toggle mouse mode for text selection
                            (KeyCode::Char('m'), _) => {
                                mouse_enabled = !mouse_enabled;
                                if mouse_enabled {
                                    let _ = execute!(terminal.backend_mut(), EnableMouseCapture);
                                    debug_message = "Mouse enabled (hover/click)".to_string();
                                } else {
                                    let _ = execute!(terminal.backend_mut(), DisableMouseCapture);
                                    cursor_pos = None; // Clear cursor position
                                    debug_message =
                                        "Mouse disabled (text selection enabled)".to_string();
                                }
                            }

                            // Toggle source code display
                            (KeyCode::Char('c'), _) => {
                                include_source = !include_source;
                                request.mutate_format_context(|fc| {
                                    fc.set_include_source(include_source);
                                });
                                debug_message = if include_source {
                                    "Source code display enabled".to_string()
                                } else {
                                    "Source code display disabled".to_string()
                                };
                            }

                            // Show help
                            (KeyCode::Char('?'), _) | (KeyCode::Char('h'), _) => {
                                show_help = true;
                            }

                            // Navigate back
                            (KeyCode::Left, _) => {
                                if history_index > 0 {
                                    history_index -= 1;
                                    let entry = &history[history_index];
                                    *document = entry.render(request);
                                    scroll_offset = 0;
                                    debug_message = format!("Back to: {}", entry.display_name());
                                } else {
                                    debug_message = "Already at beginning of history".to_string();
                                }
                            }

                            // Navigate forward
                            (KeyCode::Right, _) => {
                                if history_index + 1 < history.len() {
                                    history_index += 1;
                                    let entry = &history[history_index];
                                    *document = entry.render(request);
                                    scroll_offset = 0;
                                    debug_message = format!("Forward to: {}", entry.display_name());
                                } else {
                                    debug_message = "Already at end of history".to_string();
                                }
                            }

                            _ => {}
                        }
                    }
                }

                Event::Mouse(MouseEvent {
                    kind: MouseEventKind::Moved,
                    column,
                    row,
                    ..
                }) if mouse_enabled => {
                    // Track cursor for hover effects
                    let terminal_height = terminal.size()?.height;
                    let content_height = terminal_height.saturating_sub(2); // Exclude 2 status lines
                    let breadcrumb_row = terminal_height.saturating_sub(2);

                    if row < content_height {
                        // Mouse in main content area
                        cursor_pos = Some((column, row + scroll_offset));
                        breadcrumb_hover_pos = None;
                    } else if row == breadcrumb_row {
                        // Mouse over breadcrumb bar - check if hovering over a clickable item
                        cursor_pos = None;
                        let hovering_breadcrumb = breadcrumb_clickable_areas
                            .iter()
                            .any(|(_, range)| range.contains(&column));

                        // Track hover position for visual feedback
                        breadcrumb_hover_pos = if hovering_breadcrumb {
                            Some((column, row))
                        } else {
                            None
                        };
                    } else {
                        // Mouse over status bar
                        cursor_pos = None;
                        breadcrumb_hover_pos = None;
                    }
                }

                Event::Mouse(MouseEvent {
                    kind: MouseEventKind::ScrollDown,
                    ..
                }) if mouse_enabled => {
                    scroll_offset = scroll_offset.saturating_add(1);
                }

                Event::Mouse(MouseEvent {
                    kind: MouseEventKind::ScrollUp,
                    ..
                }) if mouse_enabled => {
                    scroll_offset = scroll_offset.saturating_sub(1);
                }

                Event::Mouse(MouseEvent {
                    kind: MouseEventKind::Down(_),
                    column,
                    row,
                    ..
                }) if mouse_enabled => {
                    let terminal_height = terminal.size()?.height;
                    let content_height = terminal_height.saturating_sub(2); // Exclude 2 status lines
                    let breadcrumb_row = terminal_height.saturating_sub(2);

                    if row < content_height {
                        // Click in main content area
                        clicked_position = Some(Position::new(column, row + scroll_offset));
                    } else if row == breadcrumb_row {
                        // Click on breadcrumb bar - check if clicking a history item
                        if let Some((idx, _)) = breadcrumb_clickable_areas
                            .iter()
                            .find(|(_, range)| range.contains(&column))
                        {
                            // Jump to this history position
                            history_index = *idx;
                            let entry = &history[history_index];
                            *document = entry.render(request);
                            scroll_offset = 0;
                            debug_message = format!("Jumped to: {}", entry.display_name());
                        }
                    }
                }

                _ => {}
            }
        }
    };

    // Clean up terminal
    disable_raw_mode()?;

    // Restore default cursor shape before exiting
    if supports_cursor {
        let _ = set_cursor_shape(terminal.backend_mut(), "default");
    }

    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}
