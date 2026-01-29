use crate::{format_context::FormatContext, request::Request, styled_string::Document};
use std::{
    fmt::Write,
    io::{self, IsTerminal},
};

mod interactive;
mod plain;
mod test_mode;
mod tty;

pub use interactive::{HistoryEntry, render_interactive};

/// Output mode for rendering documents
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    /// ANSI escape codes for terminal colors/styles
    Tty,
    /// Plain text, no decoration
    Plain,
    /// Pseudo-XML tags for testing (e.g., <keyword>struct</keyword>)
    TestMode,
}

impl OutputMode {
    /// Detect the appropriate output mode based on environment
    pub fn detect() -> Self {
        if std::env::var("FERRITIN_TEST_MODE").is_ok() {
            OutputMode::TestMode
        } else if io::stdout().is_terminal() {
            OutputMode::Tty
        } else {
            OutputMode::Plain
        }
    }
}

/// Render a document to a string based on the output mode
pub fn render(
    document: &Document,
    format_context: &FormatContext,
    output: &mut impl Write,
) -> std::fmt::Result {
    match format_context.output_mode() {
        OutputMode::Tty => tty::render(document, format_context, output),
        OutputMode::Plain => plain::render(document, output),
        OutputMode::TestMode => test_mode::render(document, output),
    }
}

impl Request {
    pub fn render(&self, document: &Document, output: &mut impl Write) -> std::fmt::Result {
        render(document, &self.format_context(), output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::styled_string::{DocumentNode, HeadingLevel, Span};

    #[test]
    fn test_render_modes() {
        let doc = Document::with_nodes(vec![
            DocumentNode::heading(
                HeadingLevel::Title,
                vec![Span::plain("Test"), Span::keyword("struct")],
            ),
            DocumentNode::Span(Span::type_name("Foo")),
        ]);

        let mut tty_output = String::new();
        let mut plain_output = String::new();
        let mut test_output = String::new();

        // Test that all modes produce output without panicking
        render(
            &doc,
            &FormatContext::new().with_output_mode(OutputMode::Tty),
            &mut tty_output,
        )
        .unwrap();
        render(
            &doc,
            &FormatContext::new().with_output_mode(OutputMode::Plain),
            &mut plain_output,
        )
        .unwrap();
        render(
            &doc,
            &FormatContext::new().with_output_mode(OutputMode::TestMode),
            &mut test_output,
        )
        .unwrap();

        assert!(!tty_output.is_empty());
        assert!(!plain_output.is_empty());
        assert!(!test_output.is_empty());
    }
}
