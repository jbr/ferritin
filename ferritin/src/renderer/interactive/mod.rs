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
mod events;
mod history;
mod keyboard;
mod mouse;
mod render_code_block;
mod render_document;
mod render_frame;
mod render_help_screen;
mod render_node;
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

#[cfg(test)]
use crate::styled_string::Document;
use utils::set_cursor_shape;

use crate::{
    commands::Commands,
    render_context::RenderContext,
    renderer::interactive::state::{InputMode, InteractiveState, UiMode},
    request::Request,
};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::{
    io::{self, stdout},
    sync::mpsc::{Receiver, Sender, channel},
    thread,
    time::Duration,
};

use channels::{RequestResponse, UiCommand};
use request_thread::request_thread_loop;

/// Render a document in interactive mode with scrolling and hover tracking
pub fn render_interactive(
    request: &Request,
    render_context: RenderContext,
    initial_command: Option<Commands>,
) -> io::Result<()> {
    // Use scoped threads so request can be borrowed by both threads
    thread::scope(|scope| render_interactive_impl(scope, request, render_context, initial_command))
}

fn render_interactive_impl<'scope, 'env: 'scope>(
    scope: &'scope thread::Scope<'scope, 'env>,
    request: &'env Request,
    render_context: RenderContext,
    initial_command: Option<Commands>,
) -> io::Result<()> {
    // Build interactive theme from render context
    let interactive_theme = InteractiveTheme::from_render_context(&render_context);

    // Create channels for communication between UI and request threads
    let (cmd_tx, cmd_rx) = channel::<UiCommand<'env>>();
    let (resp_tx, resp_rx) = channel::<RequestResponse<'env>>();

    // Spawn UI thread - it only renders and handles input
    // UI thread starts without a document - will receive initial document via channel
    let ui_handle = scope.spawn(|| -> io::Result<()> {
        ui_thread_loop(render_context, interactive_theme, cmd_tx, resp_rx)
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
    render_context: RenderContext,
    interactive_theme: InteractiveTheme,
    cmd_tx: Sender<UiCommand<'a>>,
    resp_rx: Receiver<RequestResponse<'a>>,
) -> io::Result<()> {
    // Set up terminal
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    // Wait for initial document from request thread
    let (initial_document, initial_entry) = match resp_rx.recv() {
        Ok(RequestResponse::Document { doc, entry }) => (doc, entry),
        _ => {
            return Err(io::Error::other("Failed to receive initial document"));
        }
    };

    // Create interactive state
    let mut state = InteractiveState::new(
        initial_document,
        initial_entry,
        cmd_tx,
        resp_rx,
        render_context,
        interactive_theme,
    );

    // Main event loop
    let result = loop {
        if state.handle_messages() {
            break Ok(());
        }

        terminal.draw(|frame| state.render_frame(frame))?;

        state.update_cursor(&mut terminal);
        state.handle_hover();
        state.handle_click();

        // Handle events with timeout for hover updates
        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key) => {
                    if state.handle_key_event(key, &mut terminal) {
                        break Ok(());
                    }
                }

                Event::Mouse(mouse_event) => state.handle_mouse_event(mouse_event, &terminal),

                _ => {}
            }
        }
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
    use std::sync::mpsc::channel;

    let (cmd_tx, _cmd_rx) = channel();
    let (_resp_tx, resp_rx) = channel();
    let theme = InteractiveTheme::from_render_context(&render_context);

    let mut state =
        state::InteractiveState::new(document, None, cmd_tx, resp_rx, render_context, theme);
    let backend = TestBackend::new(80, 200); // Tall virtual terminal to capture all content
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|frame| state.render_frame(frame)).unwrap();
    terminal.backend().clone()
}
