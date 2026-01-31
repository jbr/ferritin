use crate::color_scheme::ColorScheme;
use crate::renderer::OutputMode;
use fieldwork::Fieldwork;
use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::SyntaxSet;

/// Context for rendering operations
///
/// This contains configuration needed to render already-formatted Documents.
/// It's separate from FormatContext (which controls what content to include).
#[derive(Debug, Fieldwork)]
#[fieldwork(get, with)]
pub(crate) struct RenderContext {
    /// Color scheme for styled text
    color_scheme: ColorScheme,
    /// Terminal width for wrapping/layout
    terminal_width: usize,
    /// Output mode (TTY, Plain, TestMode) - determines which renderer to use
    output_mode: OutputMode,
    /// Interactive mode - affects rendering decisions (e.g., link styling)
    #[field(get = "is_interactive")]
    interactive: bool,
    /// Theme name for syntax highlighting
    #[field(skip)]
    theme_name: String,
    /// Syntax set for parsing code blocks
    syntax_set: SyntaxSet,
    /// Theme set for syntax highlighting
    theme_set: ThemeSet,
}

impl RenderContext {
    pub(crate) fn theme(&self) -> &Theme {
        &self.theme_set.themes[&self.theme_name]
    }

    pub(crate) fn with_theme(mut self, theme_name: String) -> Self {
        self.color_scheme = ColorScheme::from_syntect_theme(&self.theme_set.themes[&theme_name]);
        self.theme_name = theme_name;
        self
    }

    pub(crate) fn new() -> Self {
        Self {
            color_scheme: ColorScheme::default(),
            terminal_width: 80,
            output_mode: OutputMode::TestMode,
            interactive: false,
            theme_name: String::new(),
            syntax_set: SyntaxSet::load_defaults_newlines(),
            theme_set: ThemeSet::load_defaults(),
        }
    }
}
