use crate::color_scheme::ColorScheme;
use crate::renderer::OutputMode;
use fieldwork::Fieldwork;
use std::path::Path;
use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::SyntaxSet;
use thiserror::Error;

// Include the generated themes module
mod themes {
    include!(concat!(env!("OUT_DIR"), "/themes.rs"));
}

#[derive(Debug, Error)]
pub(crate) enum ThemeError {
    #[error("Theme '{0}' not found.\n\nAvailable themes: {1}")]
    ThemeNotFound(String, String),
    #[error("Failed to load theme from file '{0}': {1}")]
    FileLoadError(String, String),
}

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
    /// Syntax set for parsing code blocks
    syntax_set: SyntaxSet,
    /// The loaded theme for syntax highlighting
    theme: Theme,
    /// The name of the currently loaded theme
    current_theme_name: Option<String>,
}

impl RenderContext {
    /// Get the list of available theme names
    pub(crate) fn available_themes() -> Vec<String> {
        themes::THEME_NAMES.iter().map(|s| s.to_string()).collect()
    }

    pub(crate) fn with_theme_name(mut self, theme_name_or_path: &str) -> Result<Self, ThemeError> {
        self.set_theme_name(theme_name_or_path)?;
        Ok(self)
    }

    pub(crate) fn set_theme_name(
        &mut self,
        theme_name_or_path: &str,
    ) -> Result<&mut Self, ThemeError> {
        // Check if it's a file path to a .tmTheme file
        let path = Path::new(&theme_name_or_path);
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("tmTheme") {
            // Load theme from file
            let theme = ThemeSet::get_theme(path).map_err(|e| {
                ThemeError::FileLoadError(theme_name_or_path.to_string(), e.to_string())
            })?;

            self.color_scheme = ColorScheme::from_syntect_theme(&theme);
            self.theme = theme;
            self.current_theme_name = Some(theme_name_or_path.to_string());
            return Ok(self);
        }

        // Try to load it as a theme name from the embedded set
        if let Some(theme) = themes::load_theme(theme_name_or_path) {
            self.color_scheme = ColorScheme::from_syntect_theme(&theme);
            self.theme = theme;
            self.current_theme_name = Some(theme_name_or_path.to_string());
            Ok(self)
        } else {
            Err(ThemeError::ThemeNotFound(
                theme_name_or_path.to_string(),
                themes::THEME_NAMES.join(", "),
            ))
        }
    }

    pub(crate) fn new() -> Self {
        // Load a default theme (first available theme)
        let default_theme_name = themes::THEME_NAMES[0];
        let default_theme =
            themes::load_theme(default_theme_name).expect("At least one theme should be available");

        Self {
            color_scheme: ColorScheme::default(),
            terminal_width: 80,
            output_mode: OutputMode::TestMode,
            interactive: false,
            syntax_set: SyntaxSet::load_defaults_newlines(),
            theme: default_theme,
            current_theme_name: Some(default_theme_name.to_string()),
        }
    }
}
