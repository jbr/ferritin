use super::state::{InputMode, UiMode};
use super::theme::InteractiveTheme;
use ratatui::{buffer::Buffer, layout::Rect};

/// Render status bar at the bottom of the screen
pub(super) fn render_status_bar(
    buf: &mut Buffer,
    area: Rect,
    message: &str,
    ui_mode: &UiMode,
    current_crate: Option<&str>,
    theme: &InteractiveTheme,
    loading: bool,
    frame_count: u32,
) {
    let style = theme.status_style;
    let hint_style = theme.status_hint_style;

    // Clear the status line
    for x in 0..area.width {
        buf.cell_mut((x, area.y)).unwrap().reset();
        buf.cell_mut((x, area.y)).unwrap().set_style(style);
    }

    // Spinner characters that cycle
    const SPINNER: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
    let spinner_char = if loading {
        SPINNER[(frame_count as usize / 2) % SPINNER.len()]
    } else {
        ' '
    };

    // Determine what to display based on UI mode
    let (mut display_text, hint_text) = match ui_mode {
        UiMode::Normal | UiMode::Help => (message.to_string(), None),
        UiMode::Input(InputMode::GoTo { buffer }) => (format!("Go to: {}", buffer), None),
        UiMode::Input(InputMode::Search { buffer, all_crates }) => {
            let scope = if *all_crates {
                "all crates".to_string()
            } else {
                current_crate
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "current crate".to_string())
            };
            (
                format!("Search in {}: {}", scope, buffer),
                Some("[tab] toggle scope"),
            )
        }
    };

    // Prepend spinner if loading
    if loading {
        display_text = format!("{} {}", spinner_char, display_text);
    }

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
        ("", "FERRITIN INTERACTIVE MODE - KEYBINDINGS", title_style),
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
        ("  c", "Toggle source code display", key_style),
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
