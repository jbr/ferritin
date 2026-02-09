use ratatui::{buffer::Buffer, layout::Rect, style::Color};
use std::f32::consts::TAU;

use super::state::InteractiveState;

/// Extract RGB values from a Color, returning None for non-RGB colors
pub(super) fn color_to_rgb(color: Color) -> Option<(u8, u8, u8)> {
    match color {
        Color::Rgb(r, g, b) => Some((r, g, b)),
        _ => None,
    }
}

/// Interpolate between two RGB colors using a factor from 0.0 to 1.0
pub(super) fn interpolate_rgb(rgb1: (u8, u8, u8), rgb2: (u8, u8, u8), factor: f32) -> Color {
    let r = (rgb1.0 as f32 * (1.0 - factor) + rgb2.0 as f32 * factor) as u8;
    let g = (rgb1.1 as f32 * (1.0 - factor) + rgb2.1 as f32 * factor) as u8;
    let b = (rgb1.2 as f32 * (1.0 - factor) + rgb2.2 as f32 * factor) as u8;
    Color::Rgb(r, g, b)
}

/// Calculate animation factor for a given x position (0.0 = bg1/fg1, 1.0 = bg2/fg2)
pub(super) fn animation_factor(x: u16, elapsed_ms: u128) -> f32 {
    let wavelength = 100.0;
    let speed = 0.125;
    let position = x as f32 - (elapsed_ms as f32 * speed);
    ((position / wavelength) * TAU).sin() * 0.5 + 0.5
}

/// Calculate animated background color for a given x position
pub(super) fn animated_background(x: u16, elapsed_ms: u128, bg1: Color, bg2: Color) -> Color {
    match (color_to_rgb(bg1), color_to_rgb(bg2)) {
        (Some(rgb1), Some(rgb2)) => {
            let factor = animation_factor(x, elapsed_ms);
            interpolate_rgb(rgb1, rgb2, factor)
        }
        _ => {
            // Fallback to hard stripes
            hard_stripe_bg(x, elapsed_ms, bg1, bg2)
        }
    }
}

/// Calculate animated foreground color for visibility
fn animated_foreground(x: u16, elapsed_ms: u128, fg1: Color, fg2: Color) -> Color {
    match (color_to_rgb(fg1), color_to_rgb(fg2)) {
        (Some(rgb1), Some(rgb2)) => {
            let factor = animation_factor(x, elapsed_ms);
            interpolate_rgb(rgb1, rgb2, factor)
        }
        _ => fg1, // Fallback
    }
}

/// Calculate hard stripe background color
fn hard_stripe_bg(x: u16, elapsed_ms: u128, bg1: Color, bg2: Color) -> Color {
    const STRIPE_WIDTH: u16 = 8;
    const STRIPE_ON_WIDTH: u16 = 4;
    let offset = (elapsed_ms / 40) as u16;
    let position = (x + offset) % STRIPE_WIDTH;

    if position < STRIPE_ON_WIDTH { bg2 } else { bg1 }
}

impl<'a> InteractiveState<'a> {
    /// Render loading animation bar (replaces breadcrumb bar during loading)
    pub(super) fn render_loading_bar(&mut self, buf: &mut Buffer, area: Rect) {
        let elapsed_ms = self.loading.started_at.elapsed().as_millis();

        // Get animation colors from theme
        let status_bg = self.theme.status_style.bg.unwrap_or(Color::Reset);
        let loading_bg = self.theme.status_loading_bg;
        let doc_bg = self.theme.document_bg_style.bg.unwrap_or(Color::Reset);

        const LINE_CHAR: char = '▂';

        // Render full-width animated bar with varying foreground color on document background
        for x in 0..area.width {
            let fg = animated_background(x, elapsed_ms, status_bg, loading_bg);
            let cell = buf.cell_mut((x, area.y)).unwrap();
            cell.reset();
            cell.set_char(LINE_CHAR);
            cell.set_fg(fg);
            cell.set_bg(doc_bg);
        }
    }
}
