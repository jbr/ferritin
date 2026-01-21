use super::state::{HistoryEntry, InputMode};
use super::theme::InteractiveTheme;
use ratatui::{buffer::Buffer, layout::Rect};

/// Render breadcrumb bar showing full navigation history
pub(super) fn render_breadcrumb_bar<'a>(
    buf: &mut Buffer,
    area: Rect,
    history: &[HistoryEntry<'a>],
    current_idx: usize,
    clickable_areas: &mut Vec<(usize, std::ops::Range<u16>)>,
    hover_pos: Option<(u16, u16)>,
    theme: &InteractiveTheme,
) {
    let bg_style = theme.breadcrumb_style;

    // Clear the breadcrumb line
    for x in 0..area.width {
        buf.cell_mut((x, area.y)).unwrap().reset();
        buf.cell_mut((x, area.y)).unwrap().set_style(bg_style);
    }

    if history.is_empty() {
        let text = "📍 <no history>";
        for (i, ch) in text.chars().enumerate() {
            if i >= area.width as usize {
                break;
            }
            buf.cell_mut((i as u16, area.y))
                .unwrap()
                .set_char(ch)
                .set_style(bg_style);
        }
        return;
    }

    // Build breadcrumb trail: a → b → c with current item italicized
    let mut col = 0u16;

    // Start with icon
    let icon = "📍 ";
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
        let is_hovered = hover_pos.is_some_and(|(hover_col, _)| {
            hover_col >= start_col && hover_col < end_col
        });

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

/// Render status bar at the bottom of the screen
pub(super) fn render_status_bar(
    buf: &mut Buffer,
    area: Rect,
    message: &str,
    input_mode: InputMode,
    input_buffer: &str,
    search_all_crates: bool,
    current_crate: Option<&str>,
    theme: &InteractiveTheme,
) {
    let style = theme.status_style;
    let hint_style = theme.status_hint_style;

    // Clear the status line
    for x in 0..area.width {
        buf.cell_mut((x, area.y)).unwrap().reset();
        buf.cell_mut((x, area.y)).unwrap().set_style(style);
    }

    // Determine what to display based on input mode
    let (display_text, hint_text) = match input_mode {
        InputMode::Normal => (message.to_string(), None),
        InputMode::GoTo => (format!("Go to: {}", input_buffer), None),
        InputMode::Search => {
            let scope = if search_all_crates {
                "all crates".to_string()
            } else {
                current_crate
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "current crate".to_string())
            };
            (
                format!("Search in {}: {}", scope, input_buffer),
                Some("[tab] toggle scope"),
            )
        }
    };

    // Calculate space for hint text
    let hint_len = hint_text.as_ref().map(|h| h.len()).unwrap_or(0);
    let available_width = area.width as usize;
    let text_max_width = if hint_len > 0 {
        available_width.saturating_sub(hint_len + 2) // +2 for spacing
    } else {
        available_width
    };

    // Render main text (truncate if needed)
    let truncated = if display_text.len() > text_max_width {
        &display_text[..text_max_width]
    } else {
        &display_text
    };

    let mut col = 0u16;
    for ch in truncated.chars() {
        if col >= area.width {
            break;
        }
        buf.cell_mut((col, area.y))
            .unwrap()
            .set_char(ch)
            .set_style(style);
        col += 1;
    }

    // Render right-justified hint text if present
    if let Some(hint) = hint_text {
        let hint_start = (area.width as usize).saturating_sub(hint.len()) as u16;
        let mut hint_col = hint_start;
        for ch in hint.chars() {
            if hint_col >= area.width {
                break;
            }
            buf.cell_mut((hint_col, area.y))
                .unwrap()
                .set_char(ch)
                .set_style(hint_style);
            hint_col += 1;
        }
    }
}

/// Render help screen showing all available keybindings
pub(super) fn render_help_screen(buf: &mut Buffer, area: Rect, theme: &InteractiveTheme) {
    let bg_style = theme.help_bg_style;
    let title_style = theme.help_title_style;
    let key_style = theme.help_key_style;
    let desc_style = theme.help_desc_style;

    // Clear the entire screen
    for y in 0..area.height {
        for x in 0..area.width {
            buf.cell_mut((x, y)).unwrap().reset();
            buf.cell_mut((x, y)).unwrap().set_style(bg_style);
        }
    }

    let help_text = vec![
        ("", "FERRETIN INTERACTIVE MODE - KEYBINDINGS", title_style),
        ("", "", bg_style),
        ("Navigation:", "", title_style),
        ("  j, ↓", "Scroll down", key_style),
        ("  k, ↑", "Scroll up", key_style),
        ("  Ctrl+d, PgDn", "Page down", key_style),
        ("  Ctrl+u, PgUp", "Page up", key_style),
        ("  Home", "Jump to top", key_style),
        ("  Shift+G, End", "Jump to bottom", key_style),
        ("  ←", "Navigate back in history", key_style),
        ("  →", "Navigate forward in history", key_style),
        ("", "", bg_style),
        ("Commands:", "", title_style),
        ("  g", "Go to item by path", key_style),
        ("  s", "Search (scoped to current crate)", key_style),
        (
            "    Tab",
            "  Toggle search scope (current/all crates)",
            key_style,
        ),
        ("  l", "List available crates", key_style),
        ("  Esc", "Cancel input mode / Exit help / Quit", key_style),
        ("", "", bg_style),
        ("Mouse:", "", title_style),
        ("  m", "Toggle mouse mode (for text selection)", key_style),
        ("  Click", "Navigate to item / Expand block", key_style),
        ("  Hover", "Show preview in status bar", key_style),
        ("  Scroll", "Scroll content", key_style),
        ("", "", bg_style),
        ("Help:", "", title_style),
        ("  ?, h", "Show this help screen", key_style),
        ("", "", bg_style),
        ("Other:", "", title_style),
        ("  q, Ctrl+c", "Quit", key_style),
        ("", "", bg_style),
        ("", "Press any key to close help", desc_style),
    ];

    // Calculate maximum width for consistent formatting
    let max_width = help_text
        .iter()
        .map(|(key, desc, _)| {
            if key.is_empty() {
                desc.len()
            } else {
                format!("{:20} {}", key, desc).len()
            }
        })
        .max()
        .unwrap_or(60);

    let start_row = (area.height.saturating_sub(help_text.len() as u16)) / 2;
    let start_col = (area.width.saturating_sub(max_width as u16)) / 2;

    for (i, (key, desc, style)) in help_text.iter().enumerate() {
        let row = start_row + i as u16;
        if row >= area.height {
            break;
        }

        let text = if key.is_empty() {
            format!("{:width$}", desc, width = max_width)
        } else {
            format!("{:20} {:width$}", key, desc, width = max_width - 21)
        };

        let mut col = start_col;
        for ch in text.chars() {
            if col >= area.width {
                break;
            }
            buf.cell_mut((col, row))
                .unwrap()
                .set_char(ch)
                .set_style(*style);
            col += 1;
        }
    }
}
