use ratatui::style::{Color as RatatuiColor, Modifier, Style};
use syntect::highlighting::{Color, ThemeSettings};

use crate::render_context::RenderContext;

/// Pre-computed theme styles for interactive UI elements
#[derive(Debug)]
pub(super) struct InteractiveTheme {
    /// Breadcrumb bar (navigation history) - normal state
    pub breadcrumb_style: Style,
    /// Breadcrumb bar - current item (italic)
    pub breadcrumb_current_style: Style,
    /// Breadcrumb bar - hovered item (reversed)
    pub breadcrumb_hover_style: Style,

    /// Status bar - main text
    pub status_style: Style,
    /// Status bar - hint text (dimmed)
    pub status_hint_style: Style,
    /// Status bar - background color for loading animation (contrasting color)
    pub status_loading_bg: RatatuiColor,
    /// Status bar - foreground color that contrasts with loading_bg
    pub status_loading_fg: RatatuiColor,

    /// Help screen - background
    pub help_bg_style: Style,
    /// Help screen - titles
    pub help_title_style: Style,
    /// Help screen - key bindings
    pub help_key_style: Style,
    /// Help screen - descriptions
    pub help_desc_style: Style,

    /// Muted/dimmed text (ellipsis, etc.)
    pub muted_style: Style,

    /// Document background style
    pub document_bg_style: Style,

    /// Code block border style
    pub code_block_border_style: Style,
}

impl InteractiveTheme {
    /// Build theme from RenderContext at renderer startup
    pub(super) fn from_render_context(render_context: &RenderContext) -> Self {
        let theme = render_context.theme();
        let settings = &theme.settings;
        let default_fg = render_context.color_scheme().default_foreground();
        let default_bg = render_context.color_scheme().default_background();

        // Derive colors with intelligent fallbacks, validating fg/bg pairs for contrast
        let (breadcrumb_bg, breadcrumb_fg) =
            derive_breadcrumb_colors(settings, default_fg, default_bg);
        let (status_bg, status_fg) = derive_status_colors(settings, default_fg, default_bg);
        let status_loading_bg = derive_status_loading_bg(settings, status_bg);
        let status_loading_fg = derive_contrasting_fg(status_loading_bg, status_fg);
        let muted_fg = derive_muted_fg(settings, default_fg);
        let accent_fg = derive_accent_fg(settings, default_fg);
        let secondary_accent_fg = derive_secondary_accent_fg(settings, accent_fg);
        let code_block_border = derive_code_block_border(settings, muted_fg);

        Self {
            breadcrumb_style: Style::default()
                .bg(to_ratatui(breadcrumb_bg))
                .fg(to_ratatui(breadcrumb_fg)),
            breadcrumb_current_style: Style::default()
                .bg(to_ratatui(breadcrumb_bg))
                .fg(to_ratatui(breadcrumb_fg))
                .add_modifier(Modifier::ITALIC),
            breadcrumb_hover_style: Style::default()
                .bg(to_ratatui(breadcrumb_bg))
                .fg(to_ratatui(breadcrumb_fg))
                .add_modifier(Modifier::REVERSED),

            status_style: Style::default()
                .bg(to_ratatui(status_bg))
                .fg(to_ratatui(status_fg)),
            status_hint_style: Style::default()
                .bg(to_ratatui(status_bg))
                .fg(to_ratatui(muted_fg)),
            status_loading_bg: to_ratatui(status_loading_bg),
            status_loading_fg: to_ratatui(status_loading_fg),

            help_bg_style: Style::default()
                .bg(to_ratatui(default_bg))
                .fg(to_ratatui(default_fg)),
            help_title_style: Style::default()
                .bg(to_ratatui(default_bg))
                .fg(to_ratatui(accent_fg))
                .add_modifier(Modifier::BOLD),
            help_key_style: Style::default()
                .bg(to_ratatui(default_bg))
                .fg(to_ratatui(secondary_accent_fg))
                .add_modifier(Modifier::BOLD),
            help_desc_style: Style::default()
                .bg(to_ratatui(default_bg))
                .fg(to_ratatui(default_fg)),

            muted_style: Style::default().fg(to_ratatui(muted_fg)),

            document_bg_style: Style::default()
                .bg(to_ratatui(default_bg))
                .fg(to_ratatui(default_fg)),

            code_block_border_style: Style::default().fg(to_ratatui(code_block_border)),
        }
    }
}

/// Convert syntect Color to ratatui Color
fn to_ratatui(color: Color) -> RatatuiColor {
    RatatuiColor::Rgb(color.r, color.g, color.b)
}

/// Dim a color by reducing brightness
fn dim_color(color: Color, factor: f32) -> Color {
    Color {
        r: (color.r as f32 * factor) as u8,
        g: (color.g as f32 * factor) as u8,
        b: (color.b as f32 * factor) as u8,
        a: color.a,
    }
}

/// Brighten a color (move towards white)
fn brighten_color(color: Color, factor: f32) -> Color {
    Color {
        r: (color.r as f32 + (255.0 - color.r as f32) * factor).min(255.0) as u8,
        g: (color.g as f32 + (255.0 - color.g as f32) * factor).min(255.0) as u8,
        b: (color.b as f32 + (255.0 - color.b as f32) * factor).min(255.0) as u8,
        a: color.a,
    }
}

/// Calculate relative luminance for contrast checking (simplified)
fn luminance(color: Color) -> f32 {
    // Simplified relative luminance calculation
    (0.299 * color.r as f32 + 0.587 * color.g as f32 + 0.114 * color.b as f32) / 255.0
}

/// Check if two colors have sufficient contrast (WCAG AA minimum ~4.5:1)
fn has_good_contrast(fg: Color, bg: Color) -> bool {
    let l1 = luminance(fg);
    let l2 = luminance(bg);
    let contrast = if l1 > l2 {
        (l1 + 0.05) / (l2 + 0.05)
    } else {
        (l2 + 0.05) / (l1 + 0.05)
    };
    contrast >= 3.0 // Relaxed threshold for UI chrome
}

/// Try a list of (bg, fg) option pairs, returning the first valid pair with good contrast
fn try_color_pairs(
    pairs: &[(Option<Color>, Option<Color>)],
    fallback_bg: Color,
    fallback_fg: Color,
) -> (Color, Color) {
    for (bg_opt, fg_opt) in pairs {
        if let (Some(bg), Some(fg)) = (bg_opt, fg_opt) {
            if has_good_contrast(*fg, *bg) {
                return (*bg, *fg);
            }
        }
    }
    (fallback_bg, fallback_fg)
}

/// Derive breadcrumb colors as a validated fg/bg pair
fn derive_breadcrumb_colors(
    settings: &ThemeSettings,
    default_fg: Color,
    default_bg: Color,
) -> (Color, Color) {
    // Try pairs in order of specificity, validating contrast
    let pairs = [
        // Most specific: accent with its foreground
        (settings.accent, settings.selection_foreground),
        // Specific: accent with general foreground
        (settings.accent, settings.foreground),
        // Common: selection pair
        (settings.selection, settings.selection_foreground),
        // Fallback: selection bg with general fg
        (settings.selection, settings.foreground),
    ];

    let fallback_bg = brighten_color(default_bg, 0.3);
    let fallback_fg = default_fg;

    try_color_pairs(&pairs, fallback_bg, fallback_fg)
}

/// Derive status bar colors as a validated fg/bg pair
fn derive_status_colors(
    settings: &ThemeSettings,
    default_fg: Color,
    default_bg: Color,
) -> (Color, Color) {
    // Try pairs in order of specificity
    let pairs = [
        // Most specific: gutter pair
        (settings.gutter, settings.gutter_foreground),
        // Fallback: gutter bg with general fg
        (settings.gutter, settings.foreground),
    ];

    let fallback_bg = dim_color(default_bg, 0.8);
    let fallback_fg = default_fg;

    try_color_pairs(&pairs, fallback_bg, fallback_fg)
}

/// Derive a contrasting background color for status bar loading animation
fn derive_status_loading_bg(settings: &ThemeSettings, status_bg: Color) -> Color {
    // Try to use selection color (usually quite different from status bar)
    settings
        .selection
        .or(settings.line_highlight)
        .or(settings.accent)
        .unwrap_or_else(|| {
            // Fallback: pick brighter or dimmer based on current brightness
            let lum = luminance(status_bg);
            if lum < 0.5 {
                brighten_color(status_bg, 0.3)
            } else {
                dim_color(status_bg, 0.7)
            }
        })
}

/// Derive a foreground color that contrasts well with the given background
fn derive_contrasting_fg(bg: Color, fallback_fg: Color) -> Color {
    // If the fallback already has good contrast, use it
    if has_good_contrast(fallback_fg, bg) {
        return fallback_fg;
    }

    // Otherwise, pick white or black based on background luminance
    let lum = luminance(bg);
    if lum < 0.5 {
        // Dark background - use light foreground
        Color {
            r: 230,
            g: 230,
            b: 230,
            a: 255,
        }
    } else {
        // Light background - use dark foreground
        Color {
            r: 30,
            g: 30,
            b: 30,
            a: 255,
        }
    }
}

/// Derive muted/dimmed foreground color
fn derive_muted_fg(settings: &ThemeSettings, default_fg: Color) -> Color {
    settings
        .guide
        .or(settings.inactive_selection_foreground)
        .unwrap_or_else(|| dim_color(default_fg, 0.6))
}

/// Derive accent foreground color (for titles)
fn derive_accent_fg(settings: &ThemeSettings, default_fg: Color) -> Color {
    settings
        .accent
        .or(settings.caret)
        .unwrap_or_else(|| brighten_color(default_fg, 0.4))
}

/// Derive secondary accent foreground color (for key bindings)
fn derive_secondary_accent_fg(settings: &ThemeSettings, accent: Color) -> Color {
    settings.find_highlight_foreground.unwrap_or({
        // Shift hue slightly from accent by rotating RGB components
        Color {
            r: accent.b,
            g: accent.r,
            b: accent.g,
            a: accent.a,
        }
    })
}

/// Derive code block border color
fn derive_code_block_border(settings: &ThemeSettings, muted_fg: Color) -> Color {
    settings.guide.unwrap_or(muted_fg)
}
