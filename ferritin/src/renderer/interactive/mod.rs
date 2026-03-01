//! Interactive terminal UI renderer for Rust documentation.
//!
//! This module implements a TUI (Terminal User Interface) for browsing Rust documentation
//! with scrolling, hover tracking, and interactive navigation.
//!
//! # Architecture
//!
//! The renderer uses a two-thread architecture:
//! - **UI thread**: Handles terminal rendering and user input
//! - **Request thread**: Executes commands and formats documentation
//!
//! Communication between threads uses channels to pass documents and commands.
//!
//! # Layout Model
//!
//! The layout system follows a simple, principled model for positioning block elements:
//!
//! ## Block Element Conventions
//!
//! Each block element (Paragraph, Heading, CodeBlock, etc.) follows these rules:
//!
//! 1. **Start**: Unconditionally set `pos.x = indent` at the beginning
//!    - Assumes it's starting on a fresh line
//!    - No need to check previous state
//!
//! 2. **End**: Increment `pos.y` when done rendering
//!    - Moves to the next line
//!    - Leaves `pos.x` wherever content ended (next element will reset it)
//!
//! ## Container Spacing
//!
//! Containers that render multiple child blocks add blank lines between them:
//!
//! ```rust,ignore
//! for (idx, child) in children.iter().enumerate() {
//!     if idx > 0 {
//!         self.layout.pos.y += 1;  // Blank line between consecutive blocks
//!     }
//!     self.render_node(child, buf);
//! }
//! ```
//!
//! This applies to:
//! - Top-level document nodes (`render_document`)
//! - Section content
//! - BlockQuote content
//! - TruncatedBlock content (when not truncated)
//! - Conditional content
//!
//! ## Special Cases
//!
//! - **List items**: Add blank lines *between* items, but not within an item's content
//!   - Keeps labels visually connected to their descriptions
//!
//! - **Transparent containers** (TruncatedBlock, Conditional): Don't add their own spacing
//!   - Just control which children to render
//!   - Children handle their own positioning
//!
//! - **TruncatedBlock borders**: Outdented by 2 columns so content doesn't shift when expanding
//!   - Border at `indent - 2`, content at `indent`
//!
//! ## Layout State
//!
//! Layout state is centralized in `LayoutState`:
//! - `pos`: Current cursor position (x, y)
//! - `indent`: Current indentation level for block elements
//! - `node_path`: Path to current node in document tree (for expand/collapse tracking)
//! - `area`: Visible rendering area
//!
//! The layout state is saved and restored when rendering children at different indentation levels.

mod channels;
mod dev_log;
mod events;
mod history;
mod keyboard;
mod mouse;
mod render_code_block;
mod render_document;
mod render_frame;
mod render_help_screen;
mod render_loading_bar;
mod render_node;
mod render_scrollbar;
mod render_span;
mod render_status_bar;
mod render_table;
mod render_theme_picker;
mod request_thread;
mod response;
mod span_style;
mod state;
mod theme;
mod utils;
mod write_text;

#[cfg(test)]
mod tests;

use events::handle_action;
use theme::InteractiveTheme;

pub use history::HistoryEntry;

use utils::set_cursor_shape;

use crate::{
    commands::Commands,
    document::{Document, DocumentNode, HeadingLevel, Span},
    logging::LogReader,
    render_context::RenderContext,
    renderer::interactive::state::{InputMode, InteractiveState, UiMode},
    request::Request,
};
use crossbeam_channel::select;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::{
    io::{self, stdout},
    thread,
};

use channels::{RequestResponse, UiCommand};
use request_thread::request_thread_loop;

/// Create a static loading document to show while sources are being loaded
fn initial_document() -> Document<'static> {
    Document::from(vec![
        DocumentNode::Heading {
            level: HeadingLevel::Title,
            spans: vec![Span::plain("Loading...")],
        },
        DocumentNode::paragraph(vec![Span::plain(
            "Loading documentation sources, please wait...",
        )]),
    ])
}

/// Render a document in interactive mode with scrolling and hover tracking
pub fn render_interactive(
    manifest_path: std::path::PathBuf,
    render_context: RenderContext,
    initial_command: Option<Commands>,
    log_reader: LogReader,
) -> io::Result<()> {
    use crate::format_context::FormatContext;

    // Create lazy Request - exists immediately but Navigator not built yet
    let format_context = FormatContext::new();
    let request = Request::lazy(manifest_path, format_context);

    // Use scoped threads so request can be borrowed by both threads
    thread::scope(|scope| {
        render_interactive_impl(scope, &request, render_context, initial_command, log_reader)
    })
}

fn render_interactive_impl<'scope, 'env: 'scope>(
    scope: &'scope thread::Scope<'scope, 'env>,
    request: &'env Request,
    render_context: RenderContext,
    initial_command: Option<Commands>,
    log_reader: LogReader,
) -> io::Result<()> {
    // Build interactive theme from render context
    let interactive_theme = InteractiveTheme::from_render_context(&render_context);

    // Create channels for communication between UI and request threads
    let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded::<UiCommand<'env>>();
    let (resp_tx, resp_rx) = crossbeam_channel::unbounded::<RequestResponse<'env>>();

    // Spawn UI thread - it only renders and handles input
    // UI thread starts without a document - will receive initial document via channel
    let ui_handle = scope.spawn(|| -> io::Result<()> {
        ui_thread_loop(
            render_context,
            interactive_theme,
            cmd_tx,
            resp_rx,
            log_reader,
        )
    });

    // Main thread becomes request thread - populate Navigator and do all formatting
    // This is where the slow source loading happens (after UI thread is running)
    request.populate();

    // Execute initial command and send to UI
    let document = initial_command
        .unwrap_or_else(Commands::list)
        .execute(request);

    let _ = resp_tx.send(RequestResponse::Document(document));

    // Run request thread loop
    request_thread_loop(request, cmd_rx, resp_tx);

    // Wait for UI thread to complete and return its result
    ui_handle.join().unwrap()?;

    Ok(())
}

/// UI thread loop - handles terminal rendering and input events only
fn ui_thread_loop<'a>(
    render_context: RenderContext,
    interactive_theme: InteractiveTheme,
    cmd_tx: crossbeam_channel::Sender<UiCommand<'a>>,
    resp_rx: crossbeam_channel::Receiver<RequestResponse<'a>>,
    log_reader: LogReader,
) -> io::Result<()> {
    // Set up terminal
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    // Create interactive state with static loading document - don't wait for sources
    let mut state = InteractiveState::new(
        initial_document(),
        None, // No history entry for loading screen
        cmd_tx,
        resp_rx,
        render_context,
        interactive_theme,
        log_reader,
    );

    // Spawn event reader thread that blocks on crossterm events
    let (event_tx, event_rx) = crossbeam_channel::unbounded();
    let _event_reader = thread::spawn(move || {
        while let Ok(evt) = event::read() {
            if event_tx.send(evt).is_err() {
                // UI thread dropped receiver, exit
                break;
            }
        }
    });

    // Timer for spinner animation during loading - fires every 30ms
    let timer_tick = crossbeam_channel::tick(std::time::Duration::from_millis(30));

    // Initial render before entering event loop
    terminal.draw(|frame| state.render_frame(frame))?;
    state.update_cursor(&mut terminal);

    // Main event loop using select! for efficient blocking
    let result = loop {
        select! {
            // Log notifications from request thread
            recv(state.log_reader.notify_receiver()) -> _ => {
                // We already received the notification in select!, so directly peek
                if let Some(latest) = state.log_reader.peek_latest() {
                    // Only update if we're in normal mode (don't override input prompts)
                    if matches!(state.ui_mode, UiMode::Normal) {
                        state.ui.debug_message = latest.into();
                    }
                }
            }

            // Timer ticks for spinner animation - only render if loading
            recv(timer_tick) -> _ => {
                if !state.loading.pending_request {
                    continue; // Skip render if not loading
                }
                // Fall through to render below
            }

            // Request responses (documents, errors, shutdown)
            recv(state.resp_rx) -> response => {
                match response {
                    Ok(response) => {
                        if state.handle_response(response) {
                            break Ok(());
                        }
                    }
                    Err(_) => {
                        // Request thread dropped sender, exit
                        break Ok(());
                    }
                }
            }

            // Keyboard and mouse events
            recv(event_rx) -> event => {
                match event {
                    Ok(Event::Key(key)) => {
                        if state.handle_key_event(key, &mut terminal) {
                            break Ok(());
                        }
                    }
                    Ok(Event::Mouse(mouse_event)) => {
                        state.handle_mouse_event(mouse_event, &terminal);
                    }
                    Ok(_) => {}
                    Err(_) => {
                        // Event reader thread exited
                        break Ok(());
                    }
                }
            }
        }

        // Update UI state
        state.handle_hover();
        state.handle_click();

        // Render
        terminal.draw(|frame| state.render_frame(frame))?;
        state.update_cursor(&mut terminal);
    };

    // Clean up terminal
    disable_raw_mode()?;

    // Restore default cursor shape before exiting
    if state.ui.supports_cursor {
        set_cursor_shape(terminal.backend_mut(), "default");
    }

    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

#[cfg(test)]
pub fn render_to_test_backend(
    document: Document<'_>,
    render_context: RenderContext,
) -> ratatui::backend::TestBackend {
    use ratatui::{Terminal, backend::TestBackend};

    let (cmd_tx, _cmd_rx) = crossbeam_channel::unbounded();
    let (_resp_tx, resp_rx) = crossbeam_channel::unbounded();
    let theme = InteractiveTheme::from_render_context(&render_context);

    // Create a dummy log reader for tests
    let (_, log_reader) = crate::logging::StatusLogBackend::new(100);
    // Don't install it globally for tests, just pass the reader

    let mut state = state::InteractiveState::new(
        document,
        None,
        cmd_tx,
        resp_rx,
        render_context,
        theme,
        log_reader,
    );
    let backend = TestBackend::new(80, 200); // Tall virtual terminal to capture all content
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|frame| state.render_frame(frame)).unwrap();
    terminal.backend().clone()
}
