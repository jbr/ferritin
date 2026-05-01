use ratatui::{buffer::Buffer, layout::Rect, style::Color};

use super::InteractiveState;

// Scrollbar appearance configuration
const SCROLLBAR_TRACK: char = ' ';
const SCROLLBAR_THUMB_TOP: char = '╻';
const SCROLLBAR_THUMB_MIDDLE: char = '┃';
const SCROLLBAR_THUMB_BOTTOM: char = '╹'; //╿

/// Brighten a color by a factor (0.0 = unchanged, 1.0 = white)
fn brighten_color(color: Color, factor: f32) -> Color {
    match color {
        Color::Rgb(r, g, b) => {
            let r = (r as f32 + (255.0 - r as f32) * factor).min(255.0) as u8;
            let g = (g as f32 + (255.0 - g as f32) * factor).min(255.0) as u8;
            let b = (b as f32 + (255.0 - b as f32) * factor).min(255.0) as u8;
            Color::Rgb(r, g, b)
        }
        _ => color, // Can't brighten indexed colors easily
    }
}

impl<'a> InteractiveState<'a> {
    /// Render scrollbar in the rightmost column if document is taller than viewport
    pub(super) fn render_scrollbar(&self, buf: &mut Buffer, area: Rect, document_height: u16) {
        let viewport_height = area.height;
        let scrollbar_x = area.x + area.width; // area.width already reduced by 1, so this is the reserved column

        // Fill entire scrollbar column with document background
        for y in 0..viewport_height {
            if let Some(cell) = buf.cell_mut((scrollbar_x, area.y + y)) {
                cell.set_style(self.theme.document_bg_style);
                cell.set_char(' ');
            }
        }

        // Only show scrollbar if document is taller than viewport
        if document_height <= viewport_height {
            return;
        }

        // Determine if scrollbar is hovered or being dragged
        let hovered = self.viewport.scrollbar_hovered;
        let dragging = self.viewport.scrollbar_dragging;

        // Calculate brightness factor based on state
        let brightness_factor = if dragging {
            0.4 // Brightest when dragging
        } else if hovered {
            0.25 // Bright when hovered
        } else {
            0.0 // Normal brightness
        };

        // Get base scrollbar style and brighten if needed
        let mut scrollbar_style = self.theme.muted_style;
        if brightness_factor > 0.0
            && let Some(fg) = scrollbar_style.fg
        {
            scrollbar_style = scrollbar_style.fg(brighten_color(fg, brightness_factor));
        }

        // Calculate thumb size (proportional to viewport/document ratio)
        let thumb_size = ((viewport_height as f32 / document_height as f32)
            * viewport_height as f32)
            .ceil()
            .max(1.0) as u16;

        // Calculate thumb position
        let scrollable_range = document_height.saturating_sub(viewport_height);
        let thumb_travel = viewport_height.saturating_sub(thumb_size);
        let thumb_start = if scrollable_range > 0 {
            ((self.viewport.scroll_offset as f32 / scrollable_range as f32) * thumb_travel as f32)
                .round() as u16
        } else {
            0
        };

        let thumb_end = thumb_start + thumb_size;

        // Render scrollbar
        for y in 0..viewport_height {
            let cell = buf.cell_mut((scrollbar_x, area.y + y));
            if let Some(cell) = cell {
                cell.set_style(scrollbar_style);

                if y < thumb_start || y >= thumb_end {
                    // Track area
                    cell.set_char(SCROLLBAR_TRACK);
                } else if y == thumb_start && thumb_size > 1 {
                    // Top of thumb
                    cell.set_char(SCROLLBAR_THUMB_TOP);
                } else if y == thumb_end - 1 && thumb_size > 1 {
                    // Bottom of thumb
                    cell.set_char(SCROLLBAR_THUMB_BOTTOM);
                } else {
                    // Middle of thumb
                    cell.set_char(SCROLLBAR_THUMB_MIDDLE);
                }
            }
        }
    }
}
