use ratatui::layout::{Position, Rect};

use super::channels::{RequestResponse, UiCommand};
use super::history::{History, HistoryEntry};
use super::theme::InteractiveTheme;
use super::ui_config::UiRenderConfig;
use super::utils::supports_cursor_shape;
use crate::styled_string::{Document, TuiAction};
use std::sync::mpsc::{Receiver, Sender};

/// UI mode - makes the modal structure of the interface explicit
#[derive(Debug)]
pub(super) enum UiMode {
    /// Normal browsing mode
    Normal,
    /// Help screen
    Help,
    /// Input mode (go-to or search)
    Input(InputMode),
}

/// Input mode with mode-specific state
#[derive(Debug)]
pub(super) enum InputMode {
    /// Go-to mode (g pressed) - navigate to an item by path
    GoTo { buffer: String },
    /// Search mode (s pressed) - search for items
    Search { buffer: String, all_crates: bool },
}

/// Document and navigation state
#[derive(Debug)]
pub(super) struct DocumentState<'a> {
    pub document: Document<'a>,
    pub history: History<'a>,
}

/// Viewport and scroll tracking
#[derive(Debug)]
pub(super) struct ViewportState {
    pub scroll_offset: u16,
    pub cursor_pos: Option<Position>,
    pub clicked_position: Option<Position>,
}

/// Rendering state computed each frame
#[derive(Debug)]
pub(super) struct RenderCache<'a> {
    pub actions: Vec<(Rect, TuiAction<'a>)>,
}

/// UI display state
#[derive(Debug)]
pub(super) struct UiState {
    pub mouse_enabled: bool,
    pub debug_message: String,
    pub is_hovering: bool,
    pub supports_cursor: bool,
    pub include_source: bool,
}

/// Request/response tracking state
#[derive(Debug)]
pub(super) struct LoadingState {
    pub pending_request: bool,
    pub was_loading: bool,
    pub frame_count: u32,
}

/// Main interactive state - composes all UI state
#[derive(Debug)]
pub(super) struct InteractiveState<'a> {
    pub document: DocumentState<'a>,
    pub viewport: ViewportState,
    pub render_cache: RenderCache<'a>,
    pub ui_mode: UiMode,
    pub ui: UiState,
    pub loading: LoadingState,

    // Thread communication
    pub cmd_tx: Sender<UiCommand<'a>>,
    pub resp_rx: Receiver<RequestResponse<'a>>,

    // Immutable config
    pub ui_config: UiRenderConfig,
    pub theme: InteractiveTheme,
}

impl<'a> InteractiveState<'a> {
    /// Create new interactive state from initial components
    pub(super) fn new(
        initial_document: Document<'a>,
        initial_entry: Option<HistoryEntry<'a>>,
        cmd_tx: Sender<UiCommand<'a>>,
        resp_rx: Receiver<RequestResponse<'a>>,
        ui_config: UiRenderConfig,
        theme: InteractiveTheme,
    ) -> Self {
        Self {
            document: DocumentState {
                document: initial_document,
                history: History::new(initial_entry),
            },
            viewport: ViewportState {
                scroll_offset: 0,
                cursor_pos: None,
                clicked_position: None,
            },
            render_cache: RenderCache {
                actions: Vec::new(),
            },
            ui_mode: UiMode::Normal,
            ui: UiState {
                mouse_enabled: true,
                debug_message: String::from(
                    "ferritin - q:quit ?:help ←/→:history g:go s:search l:list c:code",
                ),
                is_hovering: false,
                supports_cursor: supports_cursor_shape(),
                include_source: false,
            },
            loading: LoadingState {
                pending_request: false,
                was_loading: false,
                frame_count: 0,
            },
            cmd_tx,
            resp_rx,
            ui_config,
            theme,
        }
    }
}
