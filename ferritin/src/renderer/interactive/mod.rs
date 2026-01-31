mod channels;
mod events;
mod render;
mod request_thread;
mod state;
mod theme;
mod ui;
mod ui_config;
mod utils;

use events::handle_action;
use render::render_document;
use theme::InteractiveTheme;

pub use state::HistoryEntry;
use state::InputMode;
use ui::{render_breadcrumb_bar, render_help_screen, render_status_bar};
use utils::{set_cursor_shape, supports_cursor_shape};

use crate::{commands::Commands, request::Request, styled_string::TuiAction};
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers, MouseEvent,
        MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Position, Rect},
};
use std::{
    borrow::Cow,
    io::{self, stdout},
    ops::Range,
    sync::mpsc::channel,
    time::Duration,
};

use channels::{RequestResponse, UiCommand};
use request_thread::request_thread_loop;
use ui_config::UiRenderConfig;

/// Render a document in interactive mode with scrolling and hover tracking
pub fn render_interactive(
    request: &Request,
    render_context: crate::render_context::RenderContext,
    initial_command: Option<Commands>,
) -> io::Result<()> {
    // Use scoped threads so request can be borrowed by both threads
    std::thread::scope(|s| render_interactive_impl(s, request, render_context, initial_command))
}

fn render_interactive_impl<'scope, 'env>(
    scope: &'scope std::thread::Scope<'scope, 'env>,
    request: &'env Request,
    render_context: crate::render_context::RenderContext,
    initial_command: Option<Commands>,
) -> io::Result<()>
where
    'env: 'scope,
{
    // Extract rendering config for UI thread (not the full RenderContext)
    let ui_config = UiRenderConfig::from_render_context(&render_context);
    let interactive_theme = InteractiveTheme::from_render_context(&render_context);

    // Create channels for communication between UI and request threads
    let (cmd_tx, cmd_rx) = channel::<UiCommand<'env>>();
    let (resp_tx, resp_rx) = channel::<RequestResponse<'env>>();

    // Spawn UI thread - it only renders and handles input
    // UI thread starts without a document - will receive initial document via channel
    let ui_handle = scope.spawn(|| -> io::Result<()> {
        ui_thread_loop(ui_config, interactive_theme, cmd_tx, resp_rx)
    });

    // Main thread becomes request thread - owns Request and does all formatting
    // Send initial document via channel
    let (document, _is_error, initial_entry) = initial_command
        .unwrap_or_else(Commands::list)
        .execute(request);

    let _ = resp_tx.send(RequestResponse::Document {
        doc: document,
        entry: initial_entry,
    });

    // Run request thread loop
    request_thread_loop(request, cmd_rx, resp_tx);

    // Wait for UI thread to complete and return its result
    ui_handle.join().unwrap()?;

    Ok(())
}

/// UI thread loop - handles terminal rendering and input events only
fn ui_thread_loop<'a>(
    ui_config: UiRenderConfig,
    interactive_theme: InteractiveTheme,
    cmd_tx: std::sync::mpsc::Sender<UiCommand<'a>>,
    resp_rx: std::sync::mpsc::Receiver<RequestResponse<'a>>,
) -> io::Result<()> {
    // Set up terminal
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    // Wait for initial document from request thread
    let (mut document, mut history, mut history_index) = match resp_rx.recv() {
        Ok(RequestResponse::Document { doc, entry }) => {
            let mut history = Vec::new();
            if let Some(entry) = entry {
                history.push(entry);
            }
            (doc, history, 0)
        }
        _ => {
            return Err(io::Error::other("Failed to receive initial document"));
        }
    };

    // Track scroll position and cursor
    let mut scroll_offset = 0u16;
    let mut cursor_pos: Option<(u16, u16)> = None;
    let mut actions = Vec::new();
    let mut clicked_position: Option<Position> = None;
    let mut breadcrumb_clickable_areas: Vec<(usize, Range<u16>)> = Vec::new();
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

    // Request state - track if we're waiting for a response
    let mut pending_request = false;
    let mut was_loading = false;
    let mut frame_count = 0u32;

    // Main event loop
    let result = loop {
        frame_count = frame_count.wrapping_add(1);
        // Check for responses from request thread (non-blocking)
        if let Ok(response) = resp_rx.try_recv() {
            pending_request = false;
            match response {
                RequestResponse::Document { doc, entry } => {
                    document = doc;
                    scroll_offset = 0;

                    // Add to history if we got an entry
                    if let Some(new_entry) = entry {
                        if history.is_empty() || history.get(history_index) != Some(&new_entry) {
                            history.truncate(history_index + 1);
                            history.push(new_entry.clone());
                            history_index = history.len() - 1;
                        }
                        debug_message =
                            format!("Loaded: {}", history[history_index].display_name());
                    }
                }
                RequestResponse::Error(err) => {
                    debug_message = err;
                }
                RequestResponse::ShuttingDown => {
                    break Ok(());
                }
            }
        }

        let _ = terminal.draw(|frame| {
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
                    &ui_config,
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
                    pending_request,
                    frame_count,
                );
            }
        })?;

        // Update cursor shape based on loading and hover state
        if supports_cursor {
            if pending_request {
                // Loading takes precedence - show wait cursor
                if !was_loading {
                    let _ = set_cursor_shape(terminal.backend_mut(), "wait");
                    was_loading = true;
                }
            } else {
                // When not loading, check hover state
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

                // Update cursor only if state changed
                if was_loading || now_hovering != is_hovering {
                    let shape = if now_hovering { "pointer" } else { "default" };
                    let _ = set_cursor_shape(terminal.backend_mut(), shape);
                    is_hovering = now_hovering;
                    was_loading = false;
                }
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

                // Handle the action - may return a command to send
                if let Some(command) = handle_action(&mut document, action) {
                    // Send command to request thread (non-blocking)
                    let _ = cmd_tx.send(command);
                    pending_request = true;
                    debug_message = "Loading...".to_string();
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
                                        // Send NavigateToPath command to request thread (non-blocking)
                                        let _ = cmd_tx.send(UiCommand::NavigateToPath(Cow::Owned(
                                            input_buffer.clone(),
                                        )));
                                        pending_request = true;
                                        debug_message = format!("Loading: {}...", input_buffer);
                                    }
                                    InputMode::Search => {
                                        // Determine search scope
                                        let search_crate = if search_all_crates {
                                            None
                                        } else {
                                            history
                                                .get(history_index)
                                                .and_then(|entry| entry.crate_name())
                                                .map(|s| Cow::Owned(s.to_string()))
                                        };

                                        // Send Search command to request thread (non-blocking)
                                        let _ = cmd_tx.send(UiCommand::Search {
                                            query: Cow::Owned(input_buffer.clone()),
                                            crate_name: search_crate.clone(),
                                            limit: 20,
                                        });
                                        pending_request = true;
                                        debug_message = format!("Searching: {}...", input_buffer);
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
                                // Send List command to request thread (non-blocking)
                                let _ = cmd_tx.send(UiCommand::List);
                                pending_request = true;
                                debug_message = "Loading crate list...".to_string();
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
                                // Send command to request thread to update FormatContext
                                let _ = cmd_tx.send(UiCommand::ToggleSource {
                                    include_source,
                                    current_item: history[history_index].item(),
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

                                    // Send command from history entry (non-blocking)
                                    let _ = cmd_tx.send(entry.to_command());
                                    pending_request = true;
                                    debug_message = format!("Loading: {}...", entry.display_name());
                                } else {
                                    debug_message = "Already at beginning of history".to_string();
                                }
                            }

                            // Navigate forward
                            (KeyCode::Right, _) => {
                                if history_index + 1 < history.len() {
                                    history_index += 1;
                                    let entry = &history[history_index];

                                    // Send command from history entry (non-blocking)
                                    let _ = cmd_tx.send(entry.to_command());
                                    pending_request = true;
                                    debug_message = format!("Loading: {}...", entry.display_name());
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

                            // Send command from history entry (non-blocking)
                            let _ = cmd_tx.send(entry.to_command());
                            pending_request = true;
                            debug_message = format!("Loading: {}...", entry.display_name());
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
