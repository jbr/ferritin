use ratatui::layout::{Position, Rect};

use super::channels::{RequestResponse, UiCommand};
use super::history::{History, HistoryEntry};
use super::theme::InteractiveTheme;
use super::utils::supports_cursor_shape;
use crate::render_context::{RenderContext, ThemeError};
use crate::styled_string::{Document, NodePath, TuiAction};
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
    /// Theme picker modal
    ThemePicker {
        /// Index of currently selected theme
        selected_index: usize,
        /// Theme name to restore on cancel
        saved_theme_name: String,
    },
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

/// Layout state - cursor position, indentation, and viewport
/// Reset at the start of each frame render
#[derive(Debug)]
pub(super) struct LayoutState {
    pub pos: Position,
    pub indent: u16,
    pub node_path: NodePath,
    pub area: Rect,
    /// Stack of x positions where blockquote markers should be drawn
    /// When rendering content, markers are drawn at each of these positions
    pub blockquote_markers: Vec<u16>,
}

/// Main interactive state - composes all UI state
#[derive(Debug)]
pub(super) struct InteractiveState<'a> {
    pub document: DocumentState<'a>,
    pub viewport: ViewportState,
    pub render_cache: RenderCache<'a>,
    pub layout: LayoutState,
    pub ui_mode: UiMode,
    pub ui: UiState,
    pub loading: LoadingState,

    // Thread communication
    pub cmd_tx: Sender<UiCommand<'a>>,
    pub resp_rx: Receiver<RequestResponse<'a>>,

    // Rendering config
    pub render_context: RenderContext,
    pub theme: InteractiveTheme,
    pub current_theme_name: Option<String>,
}

impl<'a> InteractiveState<'a> {
    /// Create new interactive state from initial components
    pub(super) fn new(
        initial_document: Document<'a>,
        initial_entry: Option<HistoryEntry<'a>>,
        cmd_tx: Sender<UiCommand<'a>>,
        resp_rx: Receiver<RequestResponse<'a>>,
        render_context: RenderContext,
        theme: InteractiveTheme,
    ) -> Self {
        let current_theme_name = render_context
            .current_theme_name()
            .as_ref()
            .map(|s| s.to_string());
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
            layout: LayoutState {
                pos: Position::default(),
                indent: 0,
                node_path: NodePath::new(),
                area: Rect::default(),
                blockquote_markers: Vec::new(),
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
            render_context,
            theme,
            current_theme_name,
        }
    }

    /// Apply a theme by name, rebuilding the interactive theme
    pub(super) fn apply_theme(&mut self, theme_name: &str) -> Result<(), ThemeError> {
        self.render_context.set_theme_name(theme_name)?;
        self.theme = InteractiveTheme::from_render_context(&self.render_context);
        self.current_theme_name = Some(theme_name.to_string());
        Ok(())
    }
}
