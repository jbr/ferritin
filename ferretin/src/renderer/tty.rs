use std::fmt::{Result, Write};

use crate::format_context::FormatContext;
use crate::styled_string::{
    Document, DocumentNode, HeadingLevel, ListItem, Span, SpanStyle, TruncationLevel,
};
use owo_colors::OwoColorize;
use syntect::easy::HighlightLines;
use syntect::util::LinesWithEndings;

/// Render budget for truncation
enum RenderBudget {
    /// Stop at N characters OR first newline
    Characters { remaining: usize },

    /// Stop at N lines OR first paragraph break (\n\n)
    Lines {
        remaining: usize,
        last_was_newline: bool,
    },

    /// No limit
    Unlimited,
}

impl RenderBudget {
    fn is_exhausted(&self) -> bool {
        match self {
            Self::Characters { remaining } | Self::Lines { remaining, .. } => *remaining == 0,
            Self::Unlimited => false,
        }
    }

    /// Check if this span text should be truncated
    /// Returns Some(truncated_text) if truncation needed, None to render fully
    fn should_truncate<'a>(&mut self, text: &'a str) -> Option<&'a str> {
        match self {
            Self::Characters { remaining } => {
                // Check delimiter first - stop at newline
                if let Some(nl_pos) = text.find('\n') {
                    *remaining = 0;
                    return Some(&text[..nl_pos]);
                }

                // Check budget
                if text.len() <= *remaining {
                    *remaining -= text.len();
                    None
                } else {
                    let truncated = truncate_at_word_boundary(text, *remaining);
                    *remaining = 0;
                    Some(truncated)
                }
            }
            Self::Lines {
                remaining,
                last_was_newline,
            } => {
                // Check delimiter first - paragraph break
                if *last_was_newline && text.starts_with('\n') {
                    *remaining = 0;
                    return Some("");
                }
                if let Some(para_pos) = text.find("\n\n") {
                    *remaining = 0;
                    return Some(&text[..para_pos]);
                }

                // Check budget
                let newline_count = text.matches('\n').count();
                if newline_count > *remaining {
                    let truncated = find_nth_newline_prefix(text, *remaining);
                    *remaining = 0;
                    Some(truncated)
                } else {
                    *remaining = remaining.saturating_sub(newline_count);
                    *last_was_newline = text.ends_with('\n');
                    None
                }
            }
            Self::Unlimited => None,
        }
    }
}

/// Truncate at word boundary
fn truncate_at_word_boundary(text: &str, max_chars: usize) -> &str {
    if text.len() <= max_chars {
        return text;
    }

    // Find last whitespace before max_chars
    if let Some(pos) = text[..max_chars].rfind(char::is_whitespace) {
        &text[..pos]
    } else {
        &text[..max_chars]
    }
}

/// Find the prefix up to the Nth newline
fn find_nth_newline_prefix(text: &str, n: usize) -> &str {
    let mut count = 0;
    for (idx, c) in text.char_indices() {
        if c == '\n' {
            count += 1;
            if count == n {
                return &text[..idx];
            }
        }
    }
    text
}

/// Check if nodes contain any meaningful (non-whitespace) content
fn has_meaningful_content(nodes: &[DocumentNode]) -> bool {
    for node in nodes {
        match node {
            DocumentNode::Span(span) => {
                if !span.text.trim().is_empty() {
                    return true;
                }
            }
            DocumentNode::TruncatedBlock { nodes, .. }
            | DocumentNode::Section { nodes, .. }
            | DocumentNode::BlockQuote { nodes } => {
                if has_meaningful_content(nodes) {
                    return true;
                }
            }
            DocumentNode::List { items } => {
                for item in items {
                    if has_meaningful_content(&item.content) {
                        return true;
                    }
                }
            }
            // Any other node type is meaningful
            _ => return true,
        }
    }
    false
}

/// Render a document with ANSI escape codes for terminal display (public API)
pub fn render(
    document: &Document,
    format_context: &FormatContext,
    output: &mut impl Write,
) -> Result {
    let mut budget = RenderBudget::Unlimited;
    render_nodes(&document.nodes, format_context, output, &mut budget)?;
    Ok(())
}

/// Render nodes with budget tracking
/// Returns (truncated, nodes_processed_count)
fn render_nodes(
    nodes: &[DocumentNode],
    format_context: &FormatContext,
    output: &mut impl Write,
    budget: &mut RenderBudget,
) -> std::result::Result<(bool, usize), std::fmt::Error> {
    for (idx, node) in nodes.iter().enumerate() {
        if budget.is_exhausted() {
            return Ok((true, idx)); // Truncated at this node
        }
        if render_node(node, output, format_context, budget)? {
            return Ok((true, idx)); // Truncated in this node
        }
    }
    Ok((false, nodes.len())) // Not truncated, processed all
}

fn render_code_block(
    lang: Option<&str>,
    code: &str,
    output: &mut impl Write,
    format_context: &FormatContext,
) -> Result {
    // Normalize rustdoc pseudo-languages to "rust"
    let lang = match lang {
        Some("no_run") | Some("should_panic") | Some("ignore") | Some("compile_fail")
        | Some("edition2015") | Some("edition2018") | Some("edition2021") | Some("edition2024") => {
            "rust"
        }
        Some(l) => l,
        None => "rust", // Default to Rust
    };

    // Try to syntax highlight with the language
    if let Some(syntax) = format_context.syntax_set().find_syntax_by_token(lang) {
        let theme = format_context.theme();
        let mut highlighter = HighlightLines::new(syntax, theme);

        for line in LinesWithEndings::from(code) {
            if let Ok(ranges) = highlighter.highlight_line(line, format_context.syntax_set()) {
                for (style, text) in ranges {
                    let fg = style.foreground;
                    write!(output, "{}", text.truecolor(fg.r, fg.g, fg.b))?;
                }
            } else {
                write!(output, "{line}")?;
            }
        }
    } else {
        // Fallback: just return the code as-is
        write!(output, "{}", code.trim_end())?;
    }

    write!(output, "\n\n")?;
    Ok(())
}

/// Render a single node with budget tracking
/// Returns true if truncation occurred
fn render_node(
    node: &DocumentNode,
    output: &mut impl Write,
    format_context: &FormatContext,
    budget: &mut RenderBudget,
) -> std::result::Result<bool, std::fmt::Error> {
    if budget.is_exhausted() {
        return Ok(true);
    }

    match node {
        DocumentNode::Span(span) => {
            if let Some(truncated_text) = budget.should_truncate(&span.text) {
                let partial_span = Span {
                    text: truncated_text.into(),
                    style: span.style,
                };
                render_span(&partial_span, output, format_context)?;
                Ok(true) // Truncated
            } else {
                render_span(span, output, format_context)?;
                Ok(false) // Not truncated
            }
        }
        DocumentNode::Heading { level, spans } => {
            // Headings stop SingleLine budget but not Lines budget
            if matches!(budget, RenderBudget::Characters { .. }) {
                return Ok(true);
            }

            render_spans(spans, output, true, format_context)?;
            writeln!(output)?;

            // Add decorative underlines
            match level {
                HeadingLevel::Title => {
                    for _ in 0..format_context.terminal_width() {
                        write!(output, "=")?;
                    }
                    writeln!(output)?;
                }
                HeadingLevel::Section => {
                    for _ in 0..format_context.terminal_width() {
                        write!(output, "-")?;
                    }
                    writeln!(output)?;
                }
            }
            Ok(false)
        }
        DocumentNode::Section { title, nodes } => {
            if let Some(title_spans) = title {
                render_spans(title_spans, output, true, format_context)?;
                writeln!(output)?;
            }
            let (truncated, _) = render_nodes(nodes, format_context, output, budget)?;
            Ok(truncated)
        }
        DocumentNode::List { items } => {
            // Lists stop SingleLine budget
            if matches!(budget, RenderBudget::Characters { .. }) {
                return Ok(true);
            }

            for item in items {
                if render_list_item(item, output, format_context, budget)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        DocumentNode::CodeBlock { lang, code } => {
            // Code blocks stop SingleLine budget
            if matches!(budget, RenderBudget::Characters { .. }) {
                return Ok(true);
            }

            render_code_block(lang.as_deref(), code, output, format_context)?;
            Ok(false)
        }
        DocumentNode::Link { url, text } => {
            write!(output, "\x1b]8;;{url}\x1b\\")?;
            for span in text {
                if let Some(truncated_text) = budget.should_truncate(&span.text) {
                    let partial_span = Span {
                        text: truncated_text.into(),
                        style: span.style,
                    };
                    render_span(&partial_span, output, format_context)?;
                    write!(output, "\x1b]8;;\x1b\\")?;
                    return Ok(true); // Truncated
                } else {
                    render_span(span, output, format_context)?;
                }
            }
            write!(output, "\x1b]8;;\x1b\\")?;
            Ok(false)
        }
        DocumentNode::HorizontalRule => {
            // Horizontal rule stops SingleLine budget
            if matches!(budget, RenderBudget::Characters { .. }) {
                return Ok(true);
            }

            for _ in 0..format_context.terminal_width() {
                write!(output, "─")?;
            }
            writeln!(output)?;
            Ok(false)
        }
        DocumentNode::BlockQuote { nodes } => {
            write!(output, "  │ ")?;
            let (truncated, _) = render_nodes(nodes, format_context, output, budget)?;
            Ok(truncated)
        }
        DocumentNode::Table { header, rows } => {
            // Tables stop SingleLine budget
            if matches!(budget, RenderBudget::Characters { .. }) {
                return Ok(true);
            }

            // Placeholder for table rendering
            let row_count = rows.len();
            let col_count = header
                .as_ref()
                .map_or_else(|| rows.first().map_or(0, |r| r.len()), |h| h.len());
            writeln!(
                output,
                "[Table: {} columns × {} rows]",
                col_count, row_count
            )?;
            Ok(false)
        }
        DocumentNode::TruncatedBlock { nodes, level } => {
            // Swap to new budget for this block
            let mut new_budget = match level {
                TruncationLevel::SingleLine => RenderBudget::Characters {
                    remaining: format_context.terminal_width().saturating_sub(6),
                },
                TruncationLevel::Brief => RenderBudget::Lines {
                    remaining: 10,
                    last_was_newline: false,
                },
                TruncationLevel::Full => RenderBudget::Unlimited,
            };

            let (truncated, nodes_processed) =
                render_nodes(nodes, format_context, output, &mut new_budget)?;

            // Only show [...] if we actually truncated and there's meaningful content in remaining nodes
            if truncated && has_meaningful_content(&nodes[nodes_processed..]) {
                write!(output, "{}", " [...]".dimmed())?;
            }
            writeln!(output)?;
            Ok(false) // Don't propagate truncation to parent
        }
    }
}

fn render_spans(
    spans: &[Span],
    output: &mut impl Write,
    bold: bool,
    format_context: &FormatContext,
) -> Result {
    for span in spans {
        render_span_with_bold(span, output, bold, format_context)?;
    }
    Ok(())
}

fn render_span(span: &Span, output: &mut impl Write, format_context: &FormatContext) -> Result {
    render_span_with_bold(span, output, false, format_context)
}

fn render_span_with_bold(
    span: &Span,
    output: &mut impl Write,
    force_bold: bool,
    format_context: &FormatContext,
) -> Result {
    let text = &span.text;

    // Get color from the color scheme based on semantic style
    let color = format_context.color_scheme().color_for(span.style);

    let styled = match span.style {
        SpanStyle::Plain => {
            // Plain text uses default foreground
            let fg = format_context.color_scheme().default_foreground();
            if force_bold {
                write!(output, "{}", text.truecolor(fg.r, fg.g, fg.b).bold())?;
            } else {
                write!(output, "{}", text.truecolor(fg.r, fg.g, fg.b))?;
            }
            return Ok(());
        }
        SpanStyle::Punctuation => {
            // Punctuation uses default foreground, no color
            write!(output, "{text}")?;
            return Ok(());
        }
        SpanStyle::InlineRustCode => {
            // Inline Rust code gets syntax highlighting
            if let Some(syntax) = format_context.syntax_set().find_syntax_by_token("rust") {
                let theme = format_context.theme();
                let mut highlighter = HighlightLines::new(syntax, theme);

                if let Ok(ranges) = highlighter.highlight_line(text, format_context.syntax_set()) {
                    for (style, text_segment) in ranges {
                        let fg = style.foreground;
                        write!(output, "{}", text_segment.truecolor(fg.r, fg.g, fg.b))?;
                    }
                } else {
                    // Fallback: just output the text
                    write!(output, "{text}")?;
                }
            } else {
                // Fallback: just output the text
                write!(output, "{text}")?;
            }
            return Ok(());
        }
        SpanStyle::InlineCode => {
            // Generic inline code - use a theme color (constant color)
            // Get the constant color from the theme
            let fg = format_context
                .color_scheme()
                .color_for(SpanStyle::InlineCode);
            write!(output, "{}", text.truecolor(fg.r, fg.g, fg.b))?;
            return Ok(());
        }
        SpanStyle::Strong => {
            // Bold text
            write!(output, "{}", text.bold())?;
            return Ok(());
        }
        SpanStyle::Emphasis => {
            // Italic text
            write!(output, "{}", text.italic())?;
            return Ok(());
        }
        SpanStyle::Strikethrough => {
            // Strikethrough text
            write!(output, "{}", text.strikethrough())?;
            return Ok(());
        }
        _ => {
            // All other styles use their theme color
            text.truecolor(color.r, color.g, color.b)
        }
    };

    if force_bold {
        write!(output, "{}", styled.bold())?;
    } else {
        write!(output, "{styled}")?;
    }
    Ok(())
}

fn render_list_item(
    item: &ListItem,
    output: &mut impl Write,
    format_context: &FormatContext,
    budget: &mut RenderBudget,
) -> std::result::Result<bool, std::fmt::Error> {
    write!(output, "  • ")?;
    if let Some(label) = &item.label {
        render_spans(label, output, true, format_context)?;
    }
    let (truncated, _) = render_nodes(&item.content, format_context, output, budget)?;
    writeln!(output)?;
    Ok(truncated)
}

#[cfg(test)]
mod tests {
    use crate::renderer::OutputMode;

    use super::*;

    #[test]
    fn test_render_spans() {
        let doc = Document::with_nodes(vec![
            DocumentNode::Span(Span::keyword("struct")),
            DocumentNode::Span(Span::plain(" ")),
            DocumentNode::Span(Span::type_name("Foo")),
        ]);
        let mut output = String::new();
        let format_context = FormatContext::new().with_output_mode(OutputMode::Tty);
        render(&doc, &format_context, &mut output).unwrap();
        // Should contain ANSI codes
        assert!(output.contains("\x1b"));
        // Should contain the actual text
        assert!(output.contains("struct"));
        assert!(output.contains("Foo"));
    }

    #[test]
    fn test_render_link() {
        let doc = Document::with_nodes(vec![DocumentNode::link(
            "https://example.com".to_string(),
            vec![Span::plain("Click here")],
        )]);
        let mut output = String::new();
        let format_context = FormatContext::new().with_output_mode(OutputMode::Tty);

        render(&doc, &format_context, &mut output).unwrap();
        // Should contain OSC 8 escape sequence
        assert!(output.contains("\x1b]8;;"));
        assert!(output.contains("https://example.com"));
        assert!(output.contains("Click here"));
    }

    #[test]
    fn test_render_heading() {
        let doc = Document::with_nodes(vec![DocumentNode::heading(
            HeadingLevel::Title,
            vec![Span::plain("Test")],
        )]);

        let mut output = String::new();
        let format_context = FormatContext::new()
            .with_output_mode(OutputMode::Tty)
            .with_terminal_width(10);

        render(&doc, &format_context, &mut output).unwrap();
        assert!(output.contains("Test"));
        // Should have decorative underline
        dbg!(&output);
        assert!(output.contains("=========="));
    }
}
