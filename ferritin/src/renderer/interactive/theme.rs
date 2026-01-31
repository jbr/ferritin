use ratatui::style::{Color as RatatuiColor, Modifier, Style};
use syntect::highlighting::{Color, ThemeSettings};

use crate::render_context::RenderContext;

/// Pre-computed theme styles for interactive UI elements
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

        // Derive colors with intelligent fallbacks
        let breadcrumb_bg = derive_breadcrumb_bg(settings, default_fg);
        let breadcrumb_fg = derive_breadcrumb_fg(settings, default_fg);
        let status_bg = derive_status_bg(settings, default_bg);
        let status_fg = derive_status_fg(settings, default_fg);
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

/// Derive breadcrumb background color
fn derive_breadcrumb_bg(settings: &ThemeSettings, default_fg: Color) -> Color {
    settings
        .selection
        .or(settings.accent)
        .unwrap_or_else(|| brighten_color(default_fg, 0.3))
}

/// Derive breadcrumb foreground color
fn derive_breadcrumb_fg(settings: &ThemeSettings, default_fg: Color) -> Color {
    settings
        .selection_foreground
        .or(settings.foreground)
        .unwrap_or(default_fg)
}

/// Derive status bar background color
fn derive_status_bg(settings: &ThemeSettings, default_bg: Color) -> Color {
    settings
        .gutter
        .unwrap_or_else(|| dim_color(default_bg, 0.8))
}

/// Derive status bar foreground color
fn derive_status_fg(settings: &ThemeSettings, default_fg: Color) -> Color {
    settings
        .gutter_foreground
        .or(settings.foreground)
        .unwrap_or(default_fg)
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
