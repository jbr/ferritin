//! Configuration extracted from RenderContext for UI thread rendering

use crate::color_scheme::ColorScheme;
use crate::render_context::RenderContext;
use syntect::highlighting::Theme;
use syntect::parsing::SyntaxSet;

/// Rendering configuration for the UI thread
///
/// This contains only what's needed to render already-formatted documents.
/// Formatting happens on the request thread with the full FormatContext.
pub struct UiRenderConfig {
    /// Syntax highlighting theme
    pub(super) syntax_theme: Theme,

    /// Syntax set for parsing code blocks
    pub(super) syntax_set: SyntaxSet,

    /// Color scheme for styled spans
    pub(super) color_scheme: ColorScheme,
}

impl UiRenderConfig {
    /// Extract rendering config from RenderContext
    pub fn from_render_context(rc: &RenderContext) -> Self {
        Self {
            syntax_theme: rc.theme().clone(),
            syntax_set: rc.syntax_set().clone(),
            color_scheme: rc.color_scheme().clone(),
        }
    }

    pub fn theme(&self) -> &Theme {
        &self.syntax_theme
    }

    pub fn syntax_set(&self) -> &SyntaxSet {
        &self.syntax_set
    }

    pub fn color_scheme(&self) -> &ColorScheme {
        &self.color_scheme
    }
}
