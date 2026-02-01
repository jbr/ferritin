mod channels;
mod events;
mod history;
mod keyboard;
mod mouse;
mod render;
mod render_frame;
mod request_thread;
mod response;
mod state;
mod theme;
mod ui;
mod ui_config;
mod utils;

#[cfg(test)]
mod tests;

use events::handle_action;
use render::render_document;
use theme::InteractiveTheme;

pub use history::HistoryEntry;
use ui::{render_help_screen, render_status_bar};
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
use ui_config::UiRenderConfig;

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
        ui_config,
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
