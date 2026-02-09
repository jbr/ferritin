//! TTY renderer for colored terminal output.
//!
//! This renderer produces ANSI-colored output for TTY terminals. It uses the same
//! layout model as the plain renderer but adds color and styling.
//!
//! # Layout Model
//!
//! Follows the same principles as plain/interactive renderers:
//! - Blocks add newlines at the end
//! - Containers add blank lines between consecutive children
//! - List items are compact (no blank lines within an item)
//! - First list item content is inline with bullet, rest indented
//! - Maintains indentation for nested content

use std::fmt::{Result, Write};

use crate::render_context::RenderContext;
use crate::styled_string::{
    Document, DocumentNode, HeadingLevel, ShowWhen, Span, SpanStyle, TruncationLevel,
};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span as RatatuiSpan},
};
use syntect::easy::HighlightLines;
use syntect::util::LinesWithEndings;

/// Render budget for truncation
#[derive(Clone)]
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

/// Find the best position to wrap text within a given width
/// Returns the position after which to break, or None if no good break point exists
fn find_wrap_position(text: &str, max_width: usize) -> Option<usize> {
    if max_width == 0 || text.is_empty() {
        return None;
    }

    // Find the byte position that corresponds to max_width characters (char-boundary-safe)
    let search_end = text
        .char_indices()
        .take(max_width)
        .last()
        .map(|(idx, ch)| idx + ch.len_utf8())
        .unwrap_or(0);

    let search_range = &text[..search_end];

    // First priority: break at whitespace
    if let Some(pos) = search_range.rfind(char::is_whitespace) {
        // Avoid breaking if it would leave a very short word (< 3 chars) on next line
        // This prevents orphans like "a" or "is" at the start of a line
        if pos > 0 && text.len() - pos > 3 {
            return Some(pos);
        }
        // If the remaining part is short enough, it's ok to break here
        if text.len() - pos <= max_width / 2 {
            return Some(pos);
        }
    }

    // Second priority: break after certain punctuation (., ,, ;, :, ), ])
    // This helps with long sentences without spaces
    for (i, ch) in search_range.char_indices().rev() {
        if matches!(ch, '.' | ',' | ';' | ':' | ')' | ']' | '}') {
            // Break after the punctuation
            if i + 1 < search_range.len() {
                return Some(i + 1);
            }
        }
    }

    // Third priority: break at word boundaries (after lowercase before uppercase)
    // This helps with camelCase or PascalCase identifiers
    for i in (1..search_range.len()).rev() {
        let chars: Vec<char> = search_range.chars().collect();
        if i < chars.len() - 1 {
            let prev = chars[i - 1];
            let curr = chars[i];
            if prev.is_lowercase() && curr.is_uppercase() {
                return Some(i);
            }
        }
    }

    None
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
            DocumentNode::Paragraph { spans } => {
                if spans.iter().any(|s| !s.text.trim().is_empty()) {
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
    render_context: &RenderContext,
    output: &mut impl Write,
) -> Result {
    // Build ratatui lines from document
    let mut budget = RenderBudget::Unlimited;
    let lines = build_lines(&document.nodes, render_context, &mut budget);

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

/// Build ratatui Lines from document nodes with blank lines between blocks
pub(super) fn build_lines<'a>(
    nodes: &'a [DocumentNode],
    render_context: &RenderContext,
    budget: &mut RenderBudget,
) -> Vec<Line<'a>> {
    let mut lines = Vec::new();

    for (idx, node) in nodes.iter().enumerate() {
        if idx > 0 {
            lines.push(Line::from(vec![])); // Blank line between consecutive blocks
        }
        if budget.is_exhausted() {
            break;
        }
        build_node_lines(node, render_context, budget, &mut lines, 0);
    }

    lines
}

/// Build lines for a single node
fn build_node_lines<'a>(
    node: &'a DocumentNode,
    render_context: &RenderContext,
    budget: &mut RenderBudget,
    lines: &mut Vec<Line<'a>>,
    indent: usize,
) {
    if budget.is_exhausted() {
        return;
    }

    match node {
        DocumentNode::Paragraph { spans } => {
            // Start a new line for paragraph
            let start_idx = lines.len();
            let terminal_width = render_context.terminal_width() as usize;

            // Track current line position for word wrapping (accounting for indent)
            let mut current_line_len = indent;

            // Render paragraph spans with word wrapping
            for span in spans {
                let mut style = span_style_to_ratatui(span.style, render_context);
                let url = span.url(); // Get URL once for this span

                // Add underline decoration if this span has a URL
                if url.is_some() {
                    style = style.add_modifier(Modifier::UNDERLINED);
                }

                // Helper to wrap text with OSC8 if URL exists
                let make_text = |chunk: &str| -> String {
                    if let Some(ref url) = url {
                        wrap_with_osc8(chunk, url)
                    } else {
                        chunk.to_string()
                    }
                };

                // Handle explicit newlines in span text
                for (line_idx, line) in span.text.split('\n').enumerate() {
                    if line_idx > 0 {
                        // Explicit newline - start new line with indent
                        current_line_len = indent;
                    }

                    // Word wrap if line is too long
                    let mut remaining = line;
                    while !remaining.is_empty() {
                        let available_width = terminal_width.saturating_sub(current_line_len);

                        if available_width == 0 {
                            // No space left on this line, wrap to next with indent
                            current_line_len = indent;
                            continue;
                        }

                        if remaining.len() <= available_width {
                            // Fits on current line
                            let span_to_add = RatatuiSpan::styled(make_text(remaining), style);
                            if lines.len() == start_idx {
                                // First line of paragraph
                                lines.push(Line::from(vec![span_to_add]));
                            } else if current_line_len == indent {
                                // New line, not first line
                                lines.push(Line::from(vec![span_to_add]));
                            } else {
                                // Continuing current line
                                lines.last_mut().unwrap().spans.push(span_to_add);
                            }
                            current_line_len += remaining.len();
                            break;
                        } else {
                            // Need to wrap - find best break point
                            let wrap_pos = find_wrap_position(remaining, available_width);

                            if let Some(wrap_at) = wrap_pos {
                                let (chunk, rest) = remaining.split_at(wrap_at);
                                let span_to_add = RatatuiSpan::styled(make_text(chunk), style);
                                if lines.len() == start_idx {
                                    lines.push(Line::from(vec![span_to_add]));
                                } else if current_line_len == indent {
                                    lines.push(Line::from(vec![span_to_add]));
                                } else {
                                    lines.last_mut().unwrap().spans.push(span_to_add);
                                }
                                current_line_len = indent;
                                remaining = rest.trim_start(); // Skip leading whitespace on next line
                            } else {
                                // No good break point within available width
                                // Look for the next break point beyond the available width
                                if let Some(next_space) = remaining.find(char::is_whitespace) {
                                    // Check if the word will fit on the current line
                                    if next_space <= available_width {
                                        // Word fits on current line, write it
                                        let (chunk, rest) = remaining.split_at(next_space);
                                        let span_to_add =
                                            RatatuiSpan::styled(make_text(chunk), style);
                                        if lines.len() == start_idx {
                                            lines.push(Line::from(vec![span_to_add]));
                                        } else if current_line_len == indent {
                                            lines.push(Line::from(vec![span_to_add]));
                                        } else {
                                            lines.last_mut().unwrap().spans.push(span_to_add);
                                        }
                                        current_line_len = indent;
                                        remaining = rest.trim_start();
                                    } else {
                                        // Word doesn't fit, wrap to next line first
                                        current_line_len = indent;
                                        // Don't modify remaining, continue on next line and try again
                                    }
                                } else {
                                    // No whitespace at all in remaining text
                                    // If it fits, write it; otherwise we need to hard-break
                                    if remaining.len() <= available_width {
                                        let span_to_add =
                                            RatatuiSpan::styled(make_text(remaining), style);
                                        if lines.len() == start_idx {
                                            lines.push(Line::from(vec![span_to_add]));
                                        } else if current_line_len == indent {
                                            lines.push(Line::from(vec![span_to_add]));
                                        } else {
                                            lines.last_mut().unwrap().spans.push(span_to_add);
                                        }
                                        current_line_len += remaining.len();
                                        break;
                                    } else {
                                        // Doesn't fit even on a new line - need to hard-break mid-word
                                        // This is a last resort to avoid infinite loops
                                        if current_line_len == indent {
                                            // Already on a fresh line, must hard-break
                                            let max_fit =
                                                terminal_width.saturating_sub(indent).max(1);
                                            let (chunk, rest) =
                                                remaining.split_at(max_fit.min(remaining.len()));
                                            let span_to_add =
                                                RatatuiSpan::styled(make_text(chunk), style);
                                            lines.push(Line::from(vec![span_to_add]));
                                            current_line_len = indent;
                                            remaining = rest;
                                        } else {
                                            // Wrap to next line first, then try again
                                            current_line_len = indent;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Handle empty paragraphs
            if lines.len() == start_idx {
                // Empty paragraph - add empty line
                lines.push(Line::from(vec![]));
            }

            // Single newline after paragraph (spacing between blocks handled by containers)
        }
        DocumentNode::Heading { level, spans } => {
            if matches!(budget, RenderBudget::Characters { .. }) {
                return;
            }

            let mut heading_spans = Vec::new();
            for span in spans {
                heading_spans.push(convert_span_bold(span, render_context));
            }
            lines.push(Line::from(heading_spans));

            // Add decorative underline
            let underline_char = match level {
                HeadingLevel::Title => "=",
                HeadingLevel::Section => "-",
            };
            let underline_width = render_context.terminal_width().saturating_sub(indent);
            let underline = underline_char.repeat(underline_width);
            lines.push(Line::from(underline));
        }
        DocumentNode::Section { title, nodes } => {
            if let Some(title_spans) = title {
                let mut heading_spans = Vec::new();
                for span in title_spans {
                    heading_spans.push(convert_span_bold(span, render_context));
                }
                lines.push(Line::from(heading_spans));
                lines.push(Line::from(vec![])); // Blank line after section title
            }

            // Render children with blank lines between them
            for (idx, node) in nodes.iter().enumerate() {
                if idx > 0 {
                    lines.push(Line::from(vec![])); // Blank line between blocks
                }
                if budget.is_exhausted() {
                    break;
                }
                build_node_lines(node, render_context, budget, lines, indent);
            }
        }
        DocumentNode::List { items } => {
            for (idx, item) in items.iter().enumerate() {
                if idx > 0 {
                    lines.push(Line::from(vec![])); // Blank line between list items
                }

                // Render item content with proper indentation
                if !item.content.is_empty() {
                    let start_idx = lines.len();

                    // Render all content nodes
                    for node in &item.content {
                        let mut item_budget = budget.clone();
                        build_node_lines(node, render_context, &mut item_budget, lines, 4);
                    }

                    // Add bullet and indentation to all lines
                    for (line_idx, line) in lines[start_idx..].iter_mut().enumerate() {
                        if line_idx == 0 {
                            // First line: add bullet based on nesting level
                            let bullet = crate::renderer::bullet_for_indent(indent as u16);
                            line.spans
                                .insert(0, RatatuiSpan::raw(format!("  {} ", bullet)));
                        } else {
                            // Subsequent lines: add indentation (4 spaces to align with content after bullet)
                            line.spans.insert(0, RatatuiSpan::raw("    "));
                        }
                    }
                }
            }
        }
        DocumentNode::CodeBlock { lang, code } => {
            if matches!(budget, RenderBudget::Characters { .. }) {
                return;
            }

            lines.extend(render_code_block(lang.as_deref(), code, render_context));
        }
        DocumentNode::GeneratedCode { spans } => {
            let code_spans: Vec<_> = spans
                .iter()
                .map(|span| convert_span(span, render_context))
                .collect();
            lines.push(Line::from(code_spans));
            // Spacing between blocks handled by containers
        }
        DocumentNode::HorizontalRule => {
            if matches!(budget, RenderBudget::Characters { .. }) {
                return;
            }

            let rule_width = render_context.terminal_width().saturating_sub(indent);
            let rule = "─".repeat(rule_width);
            lines.push(Line::from(rule));
        }
        DocumentNode::BlockQuote { nodes } => {
            for (idx, node) in nodes.iter().enumerate() {
                if idx > 0 {
                    lines.push(Line::from(vec![])); // Blank line between blocks in quote
                }

                let start_idx = lines.len();
                let mut quote_budget = budget.clone();
                build_node_lines(node, render_context, &mut quote_budget, lines, 4);

                // Add quote marker to all new lines
                for line in &mut lines[start_idx..] {
                    line.spans.insert(0, RatatuiSpan::raw("  │ "));
                }
            }
        }
        DocumentNode::Table { header, rows } => {
            if matches!(budget, RenderBudget::Characters { .. }) {
                return;
            }

            lines.extend(render_table(header.as_deref(), rows, render_context));
        }
        DocumentNode::TruncatedBlock { nodes, level } => {
            // For SingleLine with heading as first node, just show the heading text (no decoration)
            let render_nodes = if matches!(level, TruncationLevel::SingleLine) {
                match nodes.first() {
                    Some(DocumentNode::Heading { spans, .. }) => {
                        // Just render the spans without the heading decoration
                        let mut heading_spans = Vec::new();
                        for span in spans {
                            heading_spans.push(convert_span(span, render_context));
                        }
                        if !heading_spans.is_empty() {
                            lines.push(Line::from(heading_spans));
                        }
                        // Check if there's more content beyond the heading
                        if nodes.len() > 1 && has_meaningful_content(&nodes[1..]) {
                            let dimmed_style = Style::default().fg(Color::DarkGray);
                            if !lines.is_empty() {
                                lines
                                    .last_mut()
                                    .unwrap()
                                    .spans
                                    .push(RatatuiSpan::styled(" [...]", dimmed_style));
                            }
                        }
                        false // Skip normal rendering
                    }
                    _ => true, // Normal rendering for other node types
                }
            } else {
                true
            };

            if render_nodes {
                let line_limit = match level {
                    TruncationLevel::SingleLine => 3, // Show ~3 lines for single-line
                    TruncationLevel::Brief => 8,      // Show ~8 lines for brief
                    TruncationLevel::Full => usize::MAX, // Show everything
                };

                let start_line_count = lines.len();
                let mut rendered_all = true;

                // Render nodes
                for (idx, child_node) in nodes.iter().enumerate() {
                    // Skip headings in the middle of truncated content (not first node)
                    // Only do this for Brief/SingleLine, not Full
                    if idx > 0
                        && !matches!(level, TruncationLevel::Full)
                        && matches!(child_node, DocumentNode::Heading { .. })
                    {
                        rendered_all = false;
                        break;
                    }

                    // Skip code blocks and lists in Brief/SingleLine mode
                    // These are multi-line structures that don't make sense in snippets
                    if !matches!(level, TruncationLevel::Full)
                        && matches!(
                            child_node,
                            DocumentNode::CodeBlock { .. }
                                | DocumentNode::GeneratedCode { .. }
                                | DocumentNode::List { .. }
                        )
                    {
                        rendered_all = false;
                        break;
                    }

                    // Add blank line between consecutive blocks
                    if idx > 0 {
                        lines.push(Line::from(vec![]));
                    }

                    // Render the node
                    build_node_lines(child_node, render_context, budget, lines, 0);

                    // For SingleLine mode: render first paragraph completely, then stop
                    // (Show the whole first paragraph even if it's longer than 3 lines)
                    if matches!(level, TruncationLevel::SingleLine) {
                        // After rendering first node, check if there are more
                        rendered_all = idx == nodes.len() - 1;
                        break;
                    }

                    // For Brief mode, check if we've exceeded the line limit
                    if lines.len() - start_line_count >= line_limit
                        && matches!(level, TruncationLevel::Brief)
                    {
                        rendered_all = false;
                        break;
                    }

                    // If this is the last node, we rendered everything
                    if idx == nodes.len() - 1 {
                        rendered_all = true;
                    }
                }

                // Show [...] if we didn't render all nodes
                if !rendered_all {
                    let dimmed_style = Style::default().fg(Color::DarkGray);
                    if !lines.is_empty() {
                        // Append to last line
                        lines
                            .last_mut()
                            .unwrap()
                            .spans
                            .push(RatatuiSpan::styled(" [...]", dimmed_style));
                    } else {
                        // Create new line
                        lines.push(Line::from(vec![RatatuiSpan::styled("[...]", dimmed_style)]));
                    }
                }
            }
        }
        DocumentNode::Conditional { show_when, nodes } => {
            // Check if content should be shown based on render context
            let should_show = match show_when {
                ShowWhen::Always => true,
                ShowWhen::Interactive => render_context.is_interactive(),
                ShowWhen::NonInteractive => !render_context.is_interactive(),
            };

            if should_show {
                for (idx, node) in nodes.iter().enumerate() {
                    if idx > 0 {
                        lines.push(Line::from(vec![])); // Blank line between blocks
                    }
                    if budget.is_exhausted() {
                        break;
                    }
                    build_node_lines(node, render_context, budget, lines, indent);
                }
            }
        }
    }
}

/// Render table with UTF-8 borders
fn render_table<'a>(
    header: Option<&[crate::styled_string::TableCell<'a>]>,
    rows: &[Vec<crate::styled_string::TableCell<'a>>],
    render_context: &RenderContext,
) -> Vec<Line<'a>> {
    let mut lines = Vec::new();

    if rows.is_empty() && header.is_none() {
        return lines;
    }

    let border_style = Style::default().fg(Color::DarkGray);

    // Calculate column widths based on content
    let num_cols = header
        .map(|h| h.len())
        .or_else(|| rows.first().map(|r| r.len()))
        .unwrap_or(0);

    if num_cols == 0 {
        return lines;
    }

    let mut col_widths = vec![0usize; num_cols];

    // Measure header widths
    if let Some(header_cells) = header {
        for (col_idx, cell) in header_cells.iter().enumerate() {
            let width = cell.spans.iter().map(|s| s.text.len()).sum::<usize>();
            col_widths[col_idx] = col_widths[col_idx].max(width);
        }
    }

    // Measure row widths
    for row_cells in rows {
        for (col_idx, cell) in row_cells.iter().enumerate() {
            if col_idx < num_cols {
                let width = cell.spans.iter().map(|s| s.text.len()).sum::<usize>();
                col_widths[col_idx] = col_widths[col_idx].max(width);
            }
        }
    }

    // Cap column widths to reasonable sizes
    let max_col_width = 40;
    for width in &mut col_widths {
        *width = (*width).min(max_col_width);
    }

    // Top border: ┌─────┬─────┐
    let mut top_border = String::new();
    top_border.push('┌');
    for (idx, &width) in col_widths.iter().enumerate() {
        top_border.push_str(&"─".repeat(width));
        if idx < col_widths.len() - 1 {
            top_border.push('┬');
        }
    }
    top_border.push('┐');
    lines.push(Line::from(vec![RatatuiSpan::styled(
        top_border,
        border_style,
    )]));

    // Render header if present
    if let Some(header_cells) = header {
        let mut header_spans = vec![RatatuiSpan::styled("│", border_style)];

        for (col_idx, cell) in header_cells.iter().enumerate() {
            let mut cell_text = String::new();
            for span in &cell.spans {
                let span_text = if span.text.len() > col_widths[col_idx] {
                    &span.text[..col_widths[col_idx]]
                } else {
                    &span.text
                };
                cell_text.push_str(span_text);
            }

            // Pad to column width
            while cell_text.len() < col_widths[col_idx] {
                cell_text.push(' ');
            }

            let mut style = span_style_to_ratatui(
                cell.spans
                    .first()
                    .map(|s| s.style)
                    .unwrap_or(crate::styled_string::SpanStyle::Plain),
                render_context,
            );
            style = style.add_modifier(Modifier::BOLD);
            header_spans.push(RatatuiSpan::styled(cell_text, style));
            header_spans.push(RatatuiSpan::styled("│", border_style));
        }
        lines.push(Line::from(header_spans));

        // Header separator: ├─────┼─────┤
        let mut header_sep = String::new();
        header_sep.push('├');
        for (idx, &width) in col_widths.iter().enumerate() {
            header_sep.push_str(&"─".repeat(width));
            if idx < col_widths.len() - 1 {
                header_sep.push('┼');
            }
        }
        header_sep.push('┤');
        lines.push(Line::from(vec![RatatuiSpan::styled(
            header_sep,
            border_style,
        )]));
    }

    // Render rows
    for row_cells in rows.iter() {
        let mut row_spans = vec![RatatuiSpan::styled("│", border_style)];

        for (col_idx, cell) in row_cells.iter().enumerate() {
            if col_idx >= num_cols {
                break;
            }

            let mut cell_text = String::new();
            for span in &cell.spans {
                let span_text = if span.text.len() > col_widths[col_idx] {
                    &span.text[..col_widths[col_idx]]
                } else {
                    &span.text
                };
                cell_text.push_str(span_text);
            }

            // Pad to column width
            while cell_text.len() < col_widths[col_idx] {
                cell_text.push(' ');
            }

            let style = span_style_to_ratatui(
                cell.spans
                    .first()
                    .map(|s| s.style)
                    .unwrap_or(crate::styled_string::SpanStyle::Plain),
                render_context,
            );
            row_spans.push(RatatuiSpan::styled(cell_text, style));
            row_spans.push(RatatuiSpan::styled("│", border_style));
        }
        lines.push(Line::from(row_spans));
    }

    // Bottom border: └─────┴─────┘
    let mut bottom_border = String::new();
    bottom_border.push('└');
    for (idx, &width) in col_widths.iter().enumerate() {
        bottom_border.push_str(&"─".repeat(width));
        if idx < col_widths.len() - 1 {
            bottom_border.push('┴');
        }
    }
    bottom_border.push('┘');
    lines.push(Line::from(vec![RatatuiSpan::styled(
        bottom_border,
        border_style,
    )]));

    // Add blank line after table
    lines.push(Line::from(vec![]));

    lines
}

/// Render code block with syntax highlighting
fn render_code_block<'a>(
    lang: Option<&str>,
    code: &'a str,
    render_context: &RenderContext,
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

    if let Some(syntax) = render_context.syntax_set().find_syntax_by_token(lang) {
        let theme = render_context.theme();
        let mut highlighter = HighlightLines::new(syntax, theme);

        for line in LinesWithEndings::from(code) {
            if let Ok(ranges) = highlighter.highlight_line(line, render_context.syntax_set()) {
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

/// Convert our Span to ratatui Span, wrapping with OSC8 links if needed
fn convert_span<'a>(span: &'a Span, render_context: &RenderContext) -> RatatuiSpan<'a> {
    let text = if let Some(url) = span.url() {
        wrap_with_osc8(span.text.as_ref(), &url)
    } else {
        span.text.to_string()
    };

    RatatuiSpan::styled(text, span_style_to_ratatui(span.style, render_context))
}

/// Convert span with partial text (for truncation), wrapping with OSC8 if needed
fn convert_span_partial<'a>(
    span: &'a Span,
    text: &'a str,
    render_context: &RenderContext,
) -> RatatuiSpan<'a> {
    let text = if let Some(url) = span.url() {
        wrap_with_osc8(text, &url)
    } else {
        text.to_string()
    };

    RatatuiSpan::styled(text, span_style_to_ratatui(span.style, render_context))
}

/// Convert span with bold modifier, wrapping with OSC8 if needed
fn convert_span_bold<'a>(span: &'a Span, render_context: &RenderContext) -> RatatuiSpan<'a> {
    let mut style = span_style_to_ratatui(span.style, render_context);
    style = style.add_modifier(Modifier::BOLD);

    let text = if let Some(url) = span.url() {
        wrap_with_osc8(span.text.as_ref(), &url)
    } else {
        span.text.to_string()
    };

    RatatuiSpan::styled(text, style)
}

/// Wrap text with OSC8 hyperlink escape codes
fn wrap_with_osc8(text: &str, url: &str) -> String {
    format!("\x1b]8;;{}\x1b\\{}\x1b]8;;\x1b\\", url, text)
}

/// Convert SpanStyle to ratatui Style
fn span_style_to_ratatui(span_style: SpanStyle, render_context: &RenderContext) -> Style {
    match span_style {
        SpanStyle::Plain => {
            let fg = render_context.color_scheme().default_foreground();
            Style::default().fg(Color::Rgb(fg.r, fg.g, fg.b))
        }
        SpanStyle::Punctuation => Style::default(),
        SpanStyle::Strong => Style::default().add_modifier(Modifier::BOLD),
        SpanStyle::Emphasis => Style::default().add_modifier(Modifier::ITALIC),
        SpanStyle::Strikethrough => Style::default().add_modifier(Modifier::CROSSED_OUT),
        SpanStyle::InlineCode | SpanStyle::InlineRustCode => {
            let color = render_context.color_scheme().color_for(span_style);
            Style::default().fg(Color::Rgb(color.r, color.g, color.b))
        }
        _ => {
            let color = render_context.color_scheme().color_for(span_style);
            Style::default().fg(Color::Rgb(color.r, color.g, color.b))
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::renderer::OutputMode;

    use super::*;

    #[test]
    fn test_render_paragraph() {
        let doc = Document::with_nodes(vec![DocumentNode::paragraph(vec![
            Span::keyword("struct"),
            Span::plain(" "),
            Span::type_name("Foo"),
        ])]);
        let mut output = String::new();
        let render_context = RenderContext::new().with_output_mode(OutputMode::Tty);
        render(&doc, &render_context, &mut output).unwrap();
        // Should contain ANSI codes
        assert!(output.contains("\x1b"));
        // Should contain the actual text
        assert!(output.contains("struct"));
        assert!(output.contains("Foo"));
    }

    #[test]
    fn test_render_heading() {
        let doc = Document::with_nodes(vec![DocumentNode::heading(
            HeadingLevel::Title,
            vec![Span::plain("Test")],
        )]);

        let mut output = String::new();
        let render_context = RenderContext::new()
            .with_output_mode(OutputMode::Tty)
            .with_terminal_width(10);

        render(&doc, &render_context, &mut output).unwrap();
        assert!(output.contains("Test"));
        // Should have decorative underline
        assert!(output.contains("=========="));
    }
}
