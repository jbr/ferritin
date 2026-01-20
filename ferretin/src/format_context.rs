use crate::color_scheme::ColorScheme;
use crate::renderer::OutputMode;
use crate::verbosity::Verbosity;
use fieldwork::Fieldwork;
use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::SyntaxSet;

/// Context for formatting operations
#[derive(Debug, Fieldwork)]
#[fieldwork(get, with, set)]
pub(crate) struct FormatContext {
    /// Whether to include source code snippets
    include_source: bool,
    /// Whether to show recursive/nested content
    #[field(get = "is_recursive", with = with_recursion)]
    recursive: bool,
    /// Level of documentation detail to show
    #[field(copy)]
    verbosity: Verbosity,
    /// Color scheme for rendering (derived from syntect theme)
    color_scheme: ColorScheme,
    /// Terminal width for wrapping/layout
    terminal_width: usize,
    /// Output mode (TTY, Plain, TestMode)
    output_mode: OutputMode,
    /// Interactive mode (affects link rendering)
    #[field(get = "is_interactive")]
    interactive: bool,
    /// Theme name for syntax highlighting
    #[field(skip)]
    theme_name: String,
    syntax_set: SyntaxSet,
    theme_set: ThemeSet,
}

impl FormatContext {
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
            include_source: false,
            recursive: false,
            verbosity: Verbosity::Full,
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
