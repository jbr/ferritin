use crate::{document::Document, render_context::RenderContext};
use std::{
    fmt::Write,
    io::{self, IsTerminal},
};

mod interactive;
mod plain;
mod test_mode;
mod tty;

pub use interactive::{HistoryEntry, render_interactive};

/// Bullet characters for list items at different nesting levels
/// Cycles through these as lists nest deeper
const LIST_BULLETS: &[char] = &['◦', '▪', '•', '‣', '⁃'];

/// Get the bullet character for a given indentation level
///
/// The indent is the column position, with each nesting level typically
/// adding 4 columns (2 spaces + bullet + space)
pub(crate) fn bullet_for_indent(indent: u16) -> char {
    // Each list level adds approximately 4 columns of indent
    // (though blockquotes also add indent, we use this as a rough proxy)
    let nesting_level = (indent / 4) as usize;
    LIST_BULLETS[nesting_level % LIST_BULLETS.len()]
}

#[cfg(test)]
pub use interactive::render_to_test_backend;

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
    render_context: &RenderContext,
    output: &mut impl Write,
) -> std::fmt::Result {
    match render_context.output_mode() {
        OutputMode::Tty => tty::render(document, render_context, output),
        OutputMode::Plain => plain::render(document, output),
        OutputMode::TestMode => test_mode::render(document, output),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{DocumentNode, HeadingLevel, Span};

    #[test]
    fn test_render_modes() {
        let doc = Document::from(vec![
            DocumentNode::heading(
                HeadingLevel::Title,
                vec![Span::plain("Test"), Span::keyword("struct")],
            ),
            DocumentNode::paragraph(vec![Span::type_name("Foo")]),
        ]);

        let mut tty_output = String::new();
        let mut plain_output = String::new();
        let mut test_output = String::new();

        // Test that all modes produce output without panicking
        render(
            &doc,
            &RenderContext::new().with_output_mode(OutputMode::Tty),
            &mut tty_output,
        )
        .unwrap();
        render(
            &doc,
            &RenderContext::new().with_output_mode(OutputMode::Plain),
            &mut plain_output,
        )
        .unwrap();
        render(
            &doc,
            &RenderContext::new().with_output_mode(OutputMode::TestMode),
            &mut test_output,
        )
        .unwrap();

        assert!(!tty_output.is_empty());
        assert!(!plain_output.is_empty());
        assert!(!test_output.is_empty());
    }
}
