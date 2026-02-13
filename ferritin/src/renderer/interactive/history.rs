use ferritin_common::DocRef;
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use rustdoc_types::Item;

use super::channels::UiCommand;
use super::render_document::BASELINE_LEFT_MARGIN;
use super::theme::InteractiveTheme;
use std::borrow::Cow;
use std::fmt::{self, Display, Formatter};
use std::ops::Range;

/// Entry in navigation history
#[derive(Debug, Clone, PartialEq)]
pub enum HistoryEntry<'a> {
    /// Regular item navigation
    Item(DocRef<'a, Item>),
    /// Search result page
    Search {
        query: String,
        crate_name: Option<String>,
    },
    /// List crates page
    List {
        /// The default crate (if any) - used for scoped search
        default_crate: Option<&'a str>,
    },
}

impl Display for HistoryEntry<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            HistoryEntry::Item(item) => f.write_str(item.name().unwrap_or("<unnamed>")),
            HistoryEntry::Search { query, crate_name } => {
                if query.is_empty() {
                    // Empty query - show "Search in crate_name" or just "Search"
                    if let Some(crate_name) = crate_name {
                        f.write_fmt(format_args!("Search in {}", crate_name))
                    } else {
                        f.write_str("Search")
                    }
                } else {
                    // Non-empty query - show quoted query
                    if let Some(crate_name) = crate_name {
                        f.write_fmt(format_args!("\"{}\" in {}", query, crate_name))
                    } else {
                        f.write_fmt(format_args!("\"{}\"", query))
                    }
                }
            }
            HistoryEntry::List { .. } => f.write_str("List"),
        }
    }
}

impl<'a> HistoryEntry<'a> {
    pub(super) fn item(&self) -> Option<DocRef<'a, Item>> {
        if let Self::Item(item) = self {
            Some(*item)
        } else {
            None
        }
    }

    /// Get a display name for this history entry
    pub(super) fn display_name(&self) -> String {
        self.to_string()
    }

    /// Get the crate name if this is an item entry
    pub(super) fn crate_name(&self) -> Option<&str> {
        match self {
            HistoryEntry::Item(item) => Some(item.crate_docs().name()),
            HistoryEntry::Search { crate_name, .. } => crate_name.as_deref(),
            HistoryEntry::List { default_crate } => default_crate.as_deref(),
        }
    }

    /// Convert this history entry to a command that can be sent to the request thread
    pub(super) fn to_command(&self) -> UiCommand<'a> {
        match self {
            HistoryEntry::Item(item) => UiCommand::Navigate(*item),
            HistoryEntry::Search { query, crate_name } => UiCommand::Search {
                query: Cow::Owned(query.clone()),
                crate_name: crate_name.as_ref().map(|c| Cow::Owned(c.clone())),
                limit: 20,
            },
            HistoryEntry::List { .. } => UiCommand::List,
        }
    }
}

/// Navigation history component - encapsulates history and breadcrumb state
#[derive(Debug)]
pub(super) struct History<'a> {
    entries: Vec<HistoryEntry<'a>>,
    current_index: usize,
    // Breadcrumb rendering state (owned by history since it's breadcrumb-specific)
    clickable_areas: Vec<(usize, Range<u16>)>,
    hover_pos: Option<Position>,
}

impl<'a> History<'a> {
    pub(super) fn new(initial_entry: Option<HistoryEntry<'a>>) -> Self {
        let mut entries = Vec::new();
        if let Some(entry) = initial_entry {
            entries.push(entry);
        }
        Self {
            entries,
            current_index: 0,
            clickable_areas: Vec::new(),
            hover_pos: None,
        }
    }

    /// Push a new entry to history, truncating forward history
    pub(super) fn push(&mut self, entry: HistoryEntry<'a>) {
        if self.entries.is_empty() || self.current() != Some(&entry) {
            self.entries.truncate(self.current_index + 1);
            self.entries.push(entry);
            self.current_index = self.entries.len() - 1;
        }
    }

    /// Navigate backward in history
    pub(super) fn go_back(&mut self) -> Option<&HistoryEntry<'a>> {
        if self.current_index > 0 {
            self.current_index -= 1;
            Some(&self.entries[self.current_index])
        } else {
            None
        }
    }

    /// Navigate forward in history
    pub(super) fn go_forward(&mut self) -> Option<&HistoryEntry<'a>> {
        if self.current_index + 1 < self.entries.len() {
            self.current_index += 1;
            Some(&self.entries[self.current_index])
        } else {
            None
        }
    }

    /// Get the current history entry
    pub(super) fn current(&self) -> Option<&HistoryEntry<'a>> {
        self.entries.get(self.current_index)
    }

    /// Check if there's history to go back to
    pub(super) fn can_go_back(&self) -> bool {
        self.current_index > 0
    }

    /// Check if there's history to go forward to
    pub(super) fn can_go_forward(&self) -> bool {
        self.current_index + 1 < self.entries.len()
    }

    /// Render the breadcrumb bar
    pub(super) fn render(&mut self, buf: &mut Buffer, area: Rect, theme: &InteractiveTheme) {
        self.clickable_areas.clear();
        let history: &[HistoryEntry<'a>] = &self.entries;
        let current_idx = self.current_index;
        let clickable_areas: &mut Vec<(usize, std::ops::Range<u16>)> = &mut self.clickable_areas;
        let hover_pos = self.hover_pos;
        let bg_style = theme.breadcrumb_style;

        // Clear the breadcrumb line
        for x in 0..area.width {
            buf.cell_mut((x, area.y)).unwrap().reset();
            buf.cell_mut((x, area.y)).unwrap().set_style(bg_style);
        }

        if history.is_empty() {
            let text = " 🦀  <no history>";
            let mut col = BASELINE_LEFT_MARGIN;
            for ch in text.chars() {
                if col >= area.width {
                    break;
                }
                buf.cell_mut((col, area.y))
                    .unwrap()
                    .set_char(ch)
                    .set_style(bg_style);
                col += 1;
            }
            return;
        }

        // Build breadcrumb trail: a → b → c with current item italicized
        let mut col = BASELINE_LEFT_MARGIN;

        // Start with icon
        let icon = " 🦀  ";
        for ch in icon.chars() {
            if col >= area.width {
                break;
            }
            buf.cell_mut((col, area.y))
                .unwrap()
                .set_char(ch)
                .set_style(bg_style);
            col += 1;
        }

        for (idx, item) in history.iter().enumerate() {
            if col >= area.width {
                break;
            }

            // Add arrow separator (except for first item)
            if idx > 0 {
                let arrow = " → ";
                for ch in arrow.chars() {
                    if col >= area.width {
                        break;
                    }
                    buf.cell_mut((col, area.y))
                        .unwrap()
                        .set_char(ch)
                        .set_style(bg_style);
                    col += 1;
                }
            }

            // Render item name with appropriate style
            let name = item.display_name();
            let start_col = col;
            let name_len = name.chars().count().min((area.width - start_col) as usize);
            let end_col = start_col + name_len as u16;

            // Check if this item is being hovered
            let is_hovered = hover_pos.is_some_and(|pos| pos.x >= start_col && pos.x < end_col);

            let item_style = if is_hovered {
                // Hovered: reversed colors for visual feedback
                theme.breadcrumb_hover_style
            } else if idx == current_idx {
                // Current item: italic
                theme.breadcrumb_current_style
            } else {
                // Other items: normal
                theme.breadcrumb_style
            };

            for ch in name.chars() {
                if col >= area.width {
                    break;
                }
                buf.cell_mut((col, area.y))
                    .unwrap()
                    .set_char(ch)
                    .set_style(item_style);
                col += 1;
            }

            // Track clickable area for this item
            if end_col > start_col {
                clickable_areas.push((idx, start_col..end_col));
            }
        }
    }

    /// Update hover state based on mouse position
    pub(super) fn handle_hover(&mut self, pos: Position) {
        let hovering = self
            .clickable_areas
            .iter()
            .any(|(_, range)| range.contains(&pos.x));
        self.hover_pos = if hovering { Some(pos) } else { None };
    }

    /// Clear hover state
    pub(super) fn clear_hover(&mut self) {
        self.hover_pos = None;
    }

    /// Handle click on breadcrumb, returning the clicked entry if any
    pub(super) fn handle_click(&mut self, pos: Position) -> Option<&HistoryEntry<'a>> {
        if let Some((idx, _)) = self
            .clickable_areas
            .iter()
            .find(|(_, range)| range.contains(&pos.x))
        {
            self.current_index = *idx;
            self.current()
        } else {
            None
        }
    }

    /// Check if mouse is currently hovering over a breadcrumb
    pub(super) fn is_hovering(&self) -> bool {
        self.hover_pos.is_some()
    }
}
