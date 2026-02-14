use ratatui::style::{Color, Modifier, Style};

use crate::document::SpanStyle;

use super::state::InteractiveState;

impl<'a> InteractiveState<'a> {
    /// Convert SpanStyle to ratatui Style
    pub(super) fn style(&self, span_style: SpanStyle) -> Style {
        match span_style {
            SpanStyle::Plain => {
                let fg = self.render_context.color_scheme().default_foreground();
                Style::default().fg(Color::Rgb(fg.r, fg.g, fg.b))
            }
            SpanStyle::Punctuation => Style::default(),
            SpanStyle::Strong => Style::default().add_modifier(Modifier::BOLD),
            SpanStyle::Emphasis => Style::default().add_modifier(Modifier::ITALIC),
            SpanStyle::Strikethrough => Style::default().add_modifier(Modifier::CROSSED_OUT),
            SpanStyle::InlineCode | SpanStyle::InlineRustCode => {
                let color = self.render_context.color_scheme().color_for(span_style);
                Style::default().fg(Color::Rgb(color.r, color.g, color.b))
            }
            _ => {
                let color = self.render_context.color_scheme().color_for(span_style);
                Style::default().fg(Color::Rgb(color.r, color.g, color.b))
            }
        }
    }
}
