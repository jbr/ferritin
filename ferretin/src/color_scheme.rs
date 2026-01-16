use crate::styled_string::SpanStyle;
use syntect::highlighting::{Color, Highlighter, Theme, ThemeSet};
use syntect::parsing::{Scope, ScopeStack};

/// A color scheme mapping semantic span styles to colors
#[derive(Debug, Clone)]
pub struct ColorScheme {
    /// Foreground colors for each span style
    colors: std::collections::HashMap<SpanStyle, Color>,
    /// Default text color
    default_foreground: Color,
    /// Default background color
    default_background: Color,
}

impl ColorScheme {
    /// Create a color scheme from a syntect theme
    pub fn from_syntect_theme(theme: &Theme) -> Self {
        let highlighter = Highlighter::new(theme);
        let default_style = highlighter.get_default();

        let mut colors = std::collections::HashMap::new();

        // Map our semantic styles to TextMate scopes
        // These scope names follow standard TextMate conventions
        colors.insert(
            SpanStyle::Keyword,
            Self::color_for_scope(&highlighter, "keyword.control"),
        );
        colors.insert(
            SpanStyle::TypeName,
            Self::color_for_scope(&highlighter, "entity.name.type"),
        );
        colors.insert(
            SpanStyle::FunctionName,
            Self::color_for_scope(&highlighter, "entity.name.function"),
        );
        colors.insert(
            SpanStyle::FieldName,
            Self::color_for_scope(&highlighter, "variable.other.member"),
        );
        colors.insert(
            SpanStyle::Lifetime,
            Self::color_for_scope(&highlighter, "storage.modifier.lifetime"),
        );
        colors.insert(
            SpanStyle::Generic,
            Self::color_for_scope(&highlighter, "entity.name.type.parameter"),
        );
        colors.insert(
            SpanStyle::Operator,
            Self::color_for_scope(&highlighter, "keyword.operator"),
        );
        colors.insert(
            SpanStyle::Comment,
            Self::color_for_scope(&highlighter, "comment.line"),
        );
        // Plain and Punctuation use default foreground

        Self {
            colors,
            default_foreground: default_style.foreground,
            default_background: default_style.background,
        }
    }

    /// Load a named theme from the default theme set
    pub fn from_theme_name(name: &str) -> Result<Self, String> {
        let theme_set = ThemeSet::load_defaults();
        theme_set
            .themes
            .get(name)
            .map(Self::from_syntect_theme)
            .ok_or_else(|| format!("Theme '{}' not found", name))
    }

    /// Get available theme names
    pub fn available_themes() -> Vec<String> {
        let theme_set = ThemeSet::load_defaults();
        theme_set.themes.keys().cloned().collect()
    }

    /// Get the color for a specific span style
    pub fn color_for(&self, style: SpanStyle) -> Color {
        self.colors
            .get(&style)
            .copied()
            .unwrap_or(self.default_foreground)
    }

    /// Get the default foreground color
    pub fn default_foreground(&self) -> Color {
        self.default_foreground
    }

    /// Get the default background color
    pub fn default_background(&self) -> Color {
        self.default_background
    }

    /// Helper to get color for a scope string
    fn color_for_scope(highlighter: &Highlighter, scope_str: &str) -> Color {
        // Parse the scope string into a Scope
        let scope = Scope::new(scope_str).unwrap_or_else(|_| {
            // Fallback to empty scope if parsing fails
            Scope::new("").unwrap()
        });

        // Create a scope stack with just this scope
        let mut stack = ScopeStack::new();
        stack.push(scope);

        // Get the style for this scope
        let style = highlighter.style_for_stack(stack.as_slice());
        style.foreground
    }
}

impl Default for ColorScheme {
    fn default() -> Self {
        // Default to base16-ocean.dark theme (same as markdown renderer)
        Self::from_theme_name("base16-ocean.dark").unwrap_or_else(|_| {
            // Fallback if theme loading fails
            Self {
                colors: std::collections::HashMap::new(),
                default_foreground: Color {
                    r: 200,
                    g: 200,
                    b: 200,
                    a: 255,
                },
                default_background: Color {
                    r: 0,
                    g: 0,
                    b: 0,
                    a: 255,
                },
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_theme() {
        let scheme = ColorScheme::from_theme_name("base16-ocean.dark");
        assert!(scheme.is_ok());
    }

    #[test]
    fn test_available_themes() {
        let themes = ColorScheme::available_themes();
        assert!(!themes.is_empty());
        assert!(themes.contains(&"base16-ocean.dark".to_string()));
    }

    #[test]
    fn test_color_for_style() {
        let scheme = ColorScheme::default();

        // Should return colors for semantic styles
        let keyword_color = scheme.color_for(SpanStyle::Keyword);
        let type_color = scheme.color_for(SpanStyle::TypeName);

        // Colors should be different from default (theme should apply styling)
        assert!(keyword_color.r != 0 || keyword_color.g != 0 || keyword_color.b != 0);
        assert!(type_color.r != 0 || type_color.g != 0 || type_color.b != 0);
    }

    #[test]
    fn test_default_colors() {
        let scheme = ColorScheme::default();
        let fg = scheme.default_foreground();
        let bg = scheme.default_background();

        // Should have valid RGB values
        assert!(fg.a == 255);
        assert!(bg.a == 255);
    }
}
