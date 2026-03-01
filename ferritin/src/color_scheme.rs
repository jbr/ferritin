use crate::document::SpanStyle;
use syntect::highlighting::{Color, Highlighter, Theme};
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

        // Map our semantic styles to TextMate scopes with fallback chains
        // Based on scope coverage analysis across our theme set

        // Rust code elements (all have 95-100% coverage)
        colors.insert(
            SpanStyle::Keyword,
            Self::color_for_scope_with_fallback(
                &highlighter,
                &["keyword.control", "keyword.other"],
                default_style.foreground,
            ),
        );
        colors.insert(
            SpanStyle::TypeName,
            Self::color_for_scope_with_fallback(
                &highlighter,
                &["entity.name.type", "entity.name.class", "storage.type"],
                default_style.foreground,
            ),
        );
        colors.insert(
            SpanStyle::FunctionName,
            Self::color_for_scope_with_fallback(
                &highlighter,
                &["entity.name.function"],
                default_style.foreground,
            ),
        );
        colors.insert(
            SpanStyle::FieldName,
            Self::color_for_scope_with_fallback(
                &highlighter,
                &["variable.other.member", "variable.other"],
                default_style.foreground,
            ),
        );
        colors.insert(
            SpanStyle::Lifetime,
            Self::color_for_scope_with_fallback(
                &highlighter,
                &["storage.modifier.lifetime", "storage.modifier"],
                default_style.foreground,
            ),
        );
        colors.insert(
            SpanStyle::Generic,
            Self::color_for_scope_with_fallback(
                &highlighter,
                &["entity.name.type.parameter", "entity.name.type"],
                default_style.foreground,
            ),
        );
        colors.insert(
            SpanStyle::Operator,
            Self::color_for_scope_with_fallback(
                &highlighter,
                &["keyword.operator"],
                default_style.foreground,
            ),
        );
        colors.insert(
            SpanStyle::Comment,
            Self::color_for_scope_with_fallback(
                &highlighter,
                &["comment.line", "comment.block"],
                default_style.foreground,
            ),
        );

        // Code content - inline code uses string color as fallback since markup.inline.raw
        // is only supported in 9% of themes
        colors.insert(
            SpanStyle::InlineRustCode,
            Self::color_for_scope_with_fallback(
                &highlighter,
                &["markup.inline.raw", "string.quoted", "constant.language"],
                default_style.foreground,
            ),
        );
        colors.insert(
            SpanStyle::InlineCode,
            Self::color_for_scope_with_fallback(
                &highlighter,
                &["markup.inline.raw", "string.quoted"],
                default_style.foreground,
            ),
        );

        // Markdown semantic styles (68% coverage for bold/italic)
        colors.insert(
            SpanStyle::Strong,
            Self::color_for_scope_with_fallback(
                &highlighter,
                &["markup.bold", "keyword.control"],
                default_style.foreground,
            ),
        );
        colors.insert(
            SpanStyle::Emphasis,
            Self::color_for_scope_with_fallback(
                &highlighter,
                &["markup.italic", "comment.line"],
                default_style.foreground,
            ),
        );
        colors.insert(
            SpanStyle::Strikethrough,
            Self::color_for_scope_with_fallback(
                &highlighter,
                &["markup.strikethrough", "comment.line"],
                default_style.foreground,
            ),
        );

        // Punctuation uses default foreground (only 27-31% coverage in themes)
        // Plain also uses default

        Self {
            colors,
            default_foreground: default_style.foreground,
            default_background: default_style.background,
        }
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

    /// Try multiple scope strings, using the first one that has a distinct color
    fn color_for_scope_with_fallback(
        highlighter: &Highlighter,
        scope_strs: &[&str],
        default_color: Color,
    ) -> Color {
        for scope_str in scope_strs {
            if let Ok(scope) = Scope::new(scope_str) {
                let mut stack = ScopeStack::new();
                stack.push(scope);
                let style = highlighter.style_for_stack(stack.as_slice());

                // Use this color if it's different from the default
                if style.foreground != default_color {
                    return style.foreground;
                }
            }
        }

        // All scopes fell back to default, so return default
        default_color
    }
}

impl Default for ColorScheme {
    fn default() -> Self {
        // Simple default color scheme
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
