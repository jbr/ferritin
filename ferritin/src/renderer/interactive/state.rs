use ratatui::layout::{Position, Rect};
use std::borrow::Cow;
use std::time::Instant;

use super::channels::{RequestResponse, UiCommand};
use super::history::{History, HistoryEntry};
use super::theme::InteractiveTheme;
use super::utils::supports_cursor_shape;
use crate::logging::LogReader;
use crate::render_context::{RenderContext, ThemeError};
use crate::styled_string::{Document, NodePath, TuiAction};
use crossbeam_channel::{Receiver, Sender};

/// UI mode - makes the modal structure of the interface explicit
#[derive(Debug)]
pub(super) enum UiMode<'a> {
    /// Normal browsing mode
    Normal,
    /// Help screen
    Help,
    /// Developer log viewer (undocumented debug feature)
    /// Stores the previous state so we can restore it on exit
    DevLog {
        previous_document: Document<'a>,
        previous_scroll: u16,
    },
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

/// Cached document layout information
#[derive(Debug, Clone, Copy)]
pub(super) struct DocumentLayoutCache {
    pub render_width: u16,
    pub document_height: u16,
}

/// Keyboard cursor state for link navigation
///
/// Models the cursor as always being in one of three positions:
/// - **VirtualTop**: Conceptually above all content (initial state, or scrolled up past first link)
/// - **Focused**: On a specific link (which may be visible or off-screen due to mouse scrolling)
/// - **VirtualBottom**: Conceptually below all content (scrolled down past last link)
///
/// The cursor position is preserved across scrolling (mouse/Page Up/Down) and only
/// resets to VirtualTop when navigating to a new document.
///
/// Navigation keys (j/k) move through this state machine:
/// - From VirtualTop: j focuses first visible link (if any)
/// - From Focused: j/k move to adjacent links or scroll to reveal more
/// - From VirtualBottom: k focuses last visible link (if any)
///
/// When focus is off-screen (scrolled away via mouse), navigation "re-enters" from
/// the appropriate edge: moving away from off-screen focus jumps to nearest visible link,
/// while moving towards it scrolls and focuses newly revealed links.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum KeyboardCursor {
    /// Positioned conceptually above the document (initial state)
    VirtualTop,
    /// Focused on a specific link (index into render_cache.actions)
    /// The link may be visible or scrolled off-screen
    Focused { action_index: usize },
    /// Positioned conceptually below the document
    VirtualBottom,
}

/// Viewport and scroll tracking
#[derive(Debug)]
pub(super) struct ViewportState {
    pub scroll_offset: u16,
    pub cursor_pos: Option<Position>,
    pub clicked_position: Option<Position>,
    pub cached_layout: Option<DocumentLayoutCache>,
    /// Last known viewport height for scroll clamping
    pub last_viewport_height: u16,
    /// Scrollbar hover/drag state
    pub scrollbar_hovered: bool,
    pub scrollbar_dragging: bool,
    /// Keyboard navigation cursor
    pub keyboard_cursor: KeyboardCursor,
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
    pub debug_message: Cow<'static, str>,
    pub is_hovering: bool,
    pub supports_cursor: bool,
    pub include_source: bool,
}

/// Request/response tracking state
#[derive(Debug)]
pub(super) struct LoadingState {
    pub pending_request: bool,
    pub was_loading: bool,
    pub started_at: Instant,
}

impl LoadingState {
    pub fn start(&mut self) {
        self.pending_request = true;
        self.started_at = Instant::now();
    }
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
    pub ui_mode: UiMode<'a>,
    pub ui: UiState,
    pub loading: LoadingState,

    // Thread communication
    pub cmd_tx: Sender<UiCommand<'a>>,
    pub resp_rx: Receiver<RequestResponse<'a>>,
    pub log_reader: LogReader,

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
        log_reader: LogReader,
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
                cached_layout: None,
                last_viewport_height: 0,
                scrollbar_hovered: false,
                scrollbar_dragging: false,
                keyboard_cursor: KeyboardCursor::VirtualTop,
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
                debug_message: "ferritin - q:quit ?:help ←/→:history g:go s:search l:list c:code"
                    .into(),
                is_hovering: false,
                supports_cursor: supports_cursor_shape(),
                include_source: false,
            },
            loading: LoadingState {
                pending_request: true,
                was_loading: false,
                started_at: Instant::now(),
            },
            cmd_tx,
            resp_rx,
            log_reader,
            render_context,
            theme,
            current_theme_name,
        }
    }

    pub(super) fn set_debug_message(&mut self, message: impl Into<Cow<'static, str>>) {
        if !self.loading.pending_request {
            self.ui.debug_message = message.into();
        }
    }

    /// Apply a theme by name, rebuilding the interactive theme
    pub(super) fn apply_theme(&mut self, theme_name: &str) -> Result<(), ThemeError> {
        self.render_context.set_theme_name(theme_name)?;
        self.theme = InteractiveTheme::from_render_context(&self.render_context);
        self.current_theme_name = Some(theme_name.to_string());
        Ok(())
    }

    /// Set scroll offset with automatic clamping to valid range
    pub(super) fn set_scroll_offset(&mut self, offset: u16) {
        self.viewport.scroll_offset = offset;
        // Clamp to valid range if we have layout info
        if let Some(cache) = self.viewport.cached_layout {
            let max_scroll = cache
                .document_height
                .saturating_sub(self.viewport.last_viewport_height);
            self.viewport.scroll_offset = self.viewport.scroll_offset.min(max_scroll);
        }
    }

    /// Check if position is in the scrollbar column
    pub(super) fn is_in_scrollbar(&self, pos: Position, content_area_width: u16) -> bool {
        // Scrollbar is at content_area_width (which is frame.width - 1)
        pos.x == content_area_width && pos.y < self.viewport.last_viewport_height
    }

    /// Check if scrollbar should be visible (document taller than viewport)
    pub(super) fn scrollbar_visible(&self) -> bool {
        self.viewport
            .cached_layout
            .map(|cache| cache.document_height > self.viewport.last_viewport_height)
            .unwrap_or(false)
    }

    /// Check if a link (by action index) is visible in the current viewport
    ///
    /// Used to determine whether keyboard-focused links need special handling
    /// (off-screen links trigger re-entry logic when navigated to).
    pub(super) fn is_link_visible(&self, action_index: usize) -> Option<bool> {
        let (rect, _) = self.render_cache.actions.get(action_index)?;
        let viewport_top = self.viewport.scroll_offset;
        let viewport_bottom = viewport_top + self.viewport.last_viewport_height;

        // Link is visible if its rect overlaps with the viewport
        Some(rect.y < viewport_bottom && rect.bottom() > viewport_top)
    }

    /// Determine if a focused link is above or below the viewport
    ///
    /// Returns Some(true) if above, Some(false) if below, None if visible or invalid index.
    /// Used to implement "re-entry" behavior: when navigating while focus is off-screen,
    /// the cursor conceptually "snaps to the edge" before processing the navigation key.
    pub(super) fn is_link_off_screen(&self, action_index: usize) -> Option<bool> {
        let (rect, _) = self.render_cache.actions.get(action_index)?;
        let viewport_top = self.viewport.scroll_offset;
        let viewport_bottom = viewport_top + self.viewport.last_viewport_height;

        if rect.bottom() <= viewport_top {
            Some(true) // Above viewport
        } else if rect.y >= viewport_bottom {
            Some(false) // Below viewport
        } else {
            None // Visible
        }
    }

    /// Find the first visible link index
    ///
    /// Used for re-entry from VirtualTop or when focused link is off-screen above.
    pub(super) fn first_visible_link(&self) -> Option<usize> {
        let viewport_top = self.viewport.scroll_offset;
        let viewport_bottom = viewport_top + self.viewport.last_viewport_height;

        self.render_cache
            .actions
            .iter()
            .enumerate()
            .find(|(_, (rect, _))| rect.y >= viewport_top && rect.y < viewport_bottom)
            .map(|(idx, _)| idx)
    }

    /// Find the last visible link index
    ///
    /// Used for re-entry from VirtualBottom or when focused link is off-screen below.
    pub(super) fn last_visible_link(&self) -> Option<usize> {
        let viewport_top = self.viewport.scroll_offset;
        let viewport_bottom = viewport_top + self.viewport.last_viewport_height;

        self.render_cache
            .actions
            .iter()
            .enumerate()
            .rev()
            .find(|(_, (rect, _))| rect.y >= viewport_top && rect.y < viewport_bottom)
            .map(|(idx, _)| idx)
    }

    /// Find the next visible link below the current one
    ///
    /// Used for j/k navigation and auto-focus: when scrolling down reveals a link
    /// below the current focus, we automatically focus it for seamless navigation.
    pub(super) fn next_visible_link(&self, current_index: usize) -> Option<usize> {
        let viewport_top = self.viewport.scroll_offset;
        let viewport_bottom = viewport_top + self.viewport.last_viewport_height;

        self.render_cache
            .actions
            .iter()
            .enumerate()
            .skip(current_index + 1)
            .find(|(_, (rect, _))| rect.y >= viewport_top && rect.y < viewport_bottom)
            .map(|(idx, _)| idx)
    }

    /// Find the previous visible link above the current one
    ///
    /// Mirror of next_visible_link for upward navigation and auto-focus.
    pub(super) fn prev_visible_link(&self, current_index: usize) -> Option<usize> {
        let viewport_top = self.viewport.scroll_offset;
        let viewport_bottom = viewport_top + self.viewport.last_viewport_height;

        self.render_cache
            .actions
            .iter()
            .enumerate()
            .take(current_index)
            .rev()
            .find(|(_, (rect, _))| rect.y >= viewport_top && rect.y < viewport_bottom)
            .map(|(idx, _)| idx)
    }

    /// Reset keyboard cursor (called on navigation to new document)
    pub(super) fn reset_keyboard_cursor(&mut self) {
        self.viewport.keyboard_cursor = KeyboardCursor::VirtualTop;
    }
}
