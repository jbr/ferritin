use std::fmt::{Result, Write};

use crate::format_context::FormatContext;
use crate::styled_string::{
    Document, DocumentNode, HeadingLevel, Span, SpanStyle, TruncationLevel,
};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span as RatatuiSpan},
};
use syntect::easy::HighlightLines;
use syntect::util::LinesWithEndings;

/// Render budget for truncation
pub(super) enum RenderBudget {
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

/// Render a document with ratatui for one-shot terminal output
pub fn render(
    document: &Document,
    format_context: &FormatContext,
    output: &mut impl Write,
) -> Result {
    // Build ratatui lines from document
    let mut budget = RenderBudget::Unlimited;
    let lines = build_lines(&document.nodes, format_context, &mut budget);

    // Write lines directly to output
    for line in lines {
        write_line_to_output(&line, output)?;
        writeln!(output)?;
    }

    Ok(())
}

/// Write a ratatui Line to output with ANSI codes
fn write_line_to_output(line: &Line, output: &mut impl Write) -> Result {
    for span in &line.spans {
        write_styled_span(span, output)?;
    }
    Ok(())
}

/// Write a styled span with ANSI codes
fn write_styled_span(span: &RatatuiSpan, output: &mut impl Write) -> Result {
    let style = span.style;

    // Build ANSI escape sequence
    let mut codes = Vec::new();

    if let Some(fg) = style.fg
        && let Color::Rgb(r, g, b) = fg
    {
        codes.push(format!("38;2;{};{};{}", r, g, b));
    }

    if style.add_modifier.contains(Modifier::BOLD) {
        codes.push("1".to_string());
    }
    if style.add_modifier.contains(Modifier::ITALIC) {
        codes.push("3".to_string());
    }
    if style.add_modifier.contains(Modifier::UNDERLINED) {
        codes.push("4".to_string());
    }
    if style.add_modifier.contains(Modifier::CROSSED_OUT) {
        codes.push("9".to_string());
    }

    if !codes.is_empty() {
        write!(output, "\x1b[{}m", codes.join(";"))?;
    }

    write!(output, "{}", span.content)?;

    if !codes.is_empty() {
        write!(output, "\x1b[0m")?;
    }

    Ok(())
}

/// Build ratatui Lines from document nodes
pub(super) fn build_lines<'a>(
    nodes: &'a [DocumentNode],
    format_context: &FormatContext,
    budget: &mut RenderBudget,
) -> Vec<Line<'a>> {
    let mut lines = Vec::new();

    for node in nodes {
        if budget.is_exhausted() {
            break;
        }
        build_node_lines(node, format_context, budget, &mut lines);
    }

    lines
}

/// Build lines for a single node
fn build_node_lines<'a>(
    node: &'a DocumentNode,
    format_context: &FormatContext,
    budget: &mut RenderBudget,
    lines: &mut Vec<Line<'a>>,
) {
    if budget.is_exhausted() {
        return;
    }

    match node {
        DocumentNode::Span(span) => {
            // Spans are added to the current line
            // This will be handled by the caller
            if let Some(truncated) = budget.should_truncate(&span.text) {
                let ratatui_span = convert_span_partial(span, truncated, format_context);
                if !lines.is_empty() {
                    lines.last_mut().unwrap().spans.push(ratatui_span);
                } else {
                    lines.push(Line::from(vec![ratatui_span]));
                }
            } else {
                let ratatui_span = convert_span(span, format_context);
                if !lines.is_empty() && !span.text.contains('\n') {
                    lines.last_mut().unwrap().spans.push(ratatui_span);
                } else {
                    // Handle multiline spans
                    for (i, line_text) in span.text.split('\n').enumerate() {
                        if i > 0 {
                            lines.push(Line::from(vec![]));
                        }
                        if !line_text.is_empty() {
                            let line_span = RatatuiSpan::styled(
                                line_text,
                                span_style_to_ratatui(span.style, format_context),
                            );
                            if i == 0 && !lines.is_empty() {
                                lines.last_mut().unwrap().spans.push(line_span);
                            } else {
                                lines.push(Line::from(vec![line_span]));
                            }
                        }
                    }
                }
            }
        }
        DocumentNode::Heading { level, spans } => {
            if matches!(budget, RenderBudget::Characters { .. }) {
                return;
            }

            let mut heading_spans = Vec::new();
            for span in spans {
                heading_spans.push(convert_span_bold(span, format_context));
            }
            lines.push(Line::from(heading_spans));

            // Add decorative underline
            let underline_char = match level {
                HeadingLevel::Title => "=",
                HeadingLevel::Section => "-",
            };
            let underline = underline_char.repeat(format_context.terminal_width());
            lines.push(Line::from(underline));
        }
        DocumentNode::Section { title, nodes } => {
            if let Some(title_spans) = title {
                let mut heading_spans = Vec::new();
                for span in title_spans {
                    heading_spans.push(convert_span_bold(span, format_context));
                }
                lines.push(Line::from(heading_spans));
            }
            lines.extend(build_lines(nodes, format_context, budget));
        }
        DocumentNode::List { items } => {
            if matches!(budget, RenderBudget::Characters { .. }) {
                return;
            }

            for item in items {
                let mut item_spans = vec![RatatuiSpan::raw("  • ")];

                if let Some(label) = &item.label {
                    for span in label {
                        item_spans.push(convert_span_bold(span, format_context));
                    }
                }

                // Add content spans to the same line
                for node in &item.content {
                    if let DocumentNode::Span(span) = node {
                        item_spans.push(convert_span(span, format_context));
                    }
                }

                lines.push(Line::from(item_spans));
            }
        }
        DocumentNode::CodeBlock { lang, code } => {
            if matches!(budget, RenderBudget::Characters { .. }) {
                return;
            }

            lines.extend(render_code_block(lang.as_deref(), code, format_context));
        }
        DocumentNode::Link { url, text, .. } => {
            let link_spans: Vec<_> = if format_context.is_interactive() {
                // Interactive mode: just render underlined text (will add TuiAction later)
                // OSC 8 hyperlinks don't work through ratatui since it controls the terminal
                text.iter()
                    .map(|span| {
                        let mut style = span_style_to_ratatui(span.style, format_context);
                        style = style.add_modifier(Modifier::UNDERLINED);
                        RatatuiSpan::styled(span.text.as_ref(), style)
                    })
                    .collect()
            } else {
                // One-shot mode: emit OSC 8 hyperlinks
                let mut spans = vec![RatatuiSpan::raw(format!("\x1b]8;;{}\x1b\\", url))];
                for span in text {
                    spans.push(convert_span(span, format_context));
                }
                spans.push(RatatuiSpan::raw("\x1b]8;;\x1b\\"));
                spans
            };

            if !lines.is_empty() {
                lines.last_mut().unwrap().spans.extend(link_spans);
            } else {
                lines.push(Line::from(link_spans));
            }
        }
        DocumentNode::HorizontalRule => {
            if matches!(budget, RenderBudget::Characters { .. }) {
                return;
            }

            let rule = "─".repeat(format_context.terminal_width());
            lines.push(Line::from(rule));
        }
        DocumentNode::BlockQuote { nodes } => {
            let inner_lines = build_lines(nodes, format_context, budget);
            for inner_line in inner_lines {
                let mut quoted_spans = vec![RatatuiSpan::raw("  │ ")];
                quoted_spans.extend(inner_line.spans);
                lines.push(Line::from(quoted_spans));
            }
        }
        DocumentNode::Table { header, rows } => {
            if matches!(budget, RenderBudget::Characters { .. }) {
                return;
            }

            let row_count = rows.len();
            let col_count = header
                .as_ref()
                .map_or_else(|| rows.first().map_or(0, |r| r.len()), |h| h.len());

            lines.push(Line::from(format!(
                "[Table: {} columns × {} rows]",
                col_count, row_count
            )));
        }
        DocumentNode::TruncatedBlock { nodes, level } => {
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

            let start_line_count = lines.len();
            lines.extend(build_lines(nodes, format_context, &mut new_budget));
            let nodes_processed = lines.len() - start_line_count;

            // Check if we truncated
            if new_budget.is_exhausted() && has_meaningful_content(&nodes[nodes_processed..]) {
                if !lines.is_empty() {
                    let dimmed_style = Style::default().fg(Color::DarkGray);
                    lines
                        .last_mut()
                        .unwrap()
                        .spans
                        .push(RatatuiSpan::styled(" [...]", dimmed_style));
                } else {
                    let dimmed_style = Style::default().fg(Color::DarkGray);
                    lines.push(Line::from(vec![RatatuiSpan::styled(
                        " [...]",
                        dimmed_style,
                    )]));
                }
            }
        }
    }
}

/// Render code block with syntax highlighting
fn render_code_block<'a>(
    lang: Option<&str>,
    code: &'a str,
    format_context: &FormatContext,
) -> Vec<Line<'a>> {
    let mut lines = Vec::new();

    // Normalize rustdoc pseudo-languages to "rust"
    let lang = match lang {
        Some("no_run") | Some("should_panic") | Some("ignore") | Some("compile_fail")
        | Some("edition2015") | Some("edition2018") | Some("edition2021") | Some("edition2024") => {
            "rust"
        }
        Some(l) => l,
        None => "rust",
    };

    if let Some(syntax) = format_context.syntax_set().find_syntax_by_token(lang) {
        let theme = format_context.theme();
        let mut highlighter = HighlightLines::new(syntax, theme);

        for line in LinesWithEndings::from(code) {
            if let Ok(ranges) = highlighter.highlight_line(line, format_context.syntax_set()) {
                let mut line_spans = Vec::new();
                for (style, text) in ranges {
                    let fg = style.foreground;
                    line_spans.push(RatatuiSpan::styled(
                        text.trim_end_matches('\n'),
                        Style::default().fg(Color::Rgb(fg.r, fg.g, fg.b)),
                    ));
                }
                lines.push(Line::from(line_spans));
            } else {
                lines.push(Line::from(line.trim_end_matches('\n')));
            }
        }
    } else {
        for line in code.lines() {
            lines.push(Line::from(line));
        }
    }

    lines.push(Line::from(""));
    lines
}

/// Convert our Span to ratatui Span
fn convert_span<'a>(span: &'a Span, format_context: &FormatContext) -> RatatuiSpan<'a> {
    RatatuiSpan::styled(
        span.text.as_ref(),
        span_style_to_ratatui(span.style, format_context),
    )
}

/// Convert span with partial text (for truncation)
fn convert_span_partial<'a>(
    span: &'a Span,
    text: &'a str,
    format_context: &FormatContext,
) -> RatatuiSpan<'a> {
    RatatuiSpan::styled(text, span_style_to_ratatui(span.style, format_context))
}

/// Convert span with bold modifier
fn convert_span_bold<'a>(span: &'a Span, format_context: &FormatContext) -> RatatuiSpan<'a> {
    let mut style = span_style_to_ratatui(span.style, format_context);
    style = style.add_modifier(Modifier::BOLD);
    RatatuiSpan::styled(span.text.as_ref(), style)
}

/// Convert SpanStyle to ratatui Style
fn span_style_to_ratatui(span_style: SpanStyle, format_context: &FormatContext) -> Style {
    match span_style {
        SpanStyle::Plain => {
            let fg = format_context.color_scheme().default_foreground();
            Style::default().fg(Color::Rgb(fg.r, fg.g, fg.b))
        }
        SpanStyle::Punctuation => Style::default(),
        SpanStyle::Strong => Style::default().add_modifier(Modifier::BOLD),
        SpanStyle::Emphasis => Style::default().add_modifier(Modifier::ITALIC),
        SpanStyle::Strikethrough => Style::default().add_modifier(Modifier::CROSSED_OUT),
        SpanStyle::InlineCode | SpanStyle::InlineRustCode => {
            let color = format_context.color_scheme().color_for(span_style);
            Style::default().fg(Color::Rgb(color.r, color.g, color.b))
        }
        _ => {
            let color = format_context.color_scheme().color_for(span_style);
            Style::default().fg(Color::Rgb(color.r, color.g, color.b))
        }
    }
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
        assert!(output.contains("=========="));
    }
}
