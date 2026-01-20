use super::utils::find_paragraph_truncation_point;
use crate::format_context::FormatContext;
use crate::styled_string::{
    DocumentNode, HeadingLevel, NodePath, Span, SpanStyle, TruncationLevel, TuiAction,
};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};
use syntect::easy::HighlightLines;
use syntect::util::LinesWithEndings;

/// Render document nodes to buffer, returning action map
/// The lifetime 'doc is for the document nodes, 'action is for the TuiActions (from Request)
pub(super) fn render_document<'a>(
    nodes: &[DocumentNode<'a>],
    format_context: &FormatContext,
    area: Rect,
    buf: &mut Buffer,
    scroll: u16,
    cursor_pos: Option<(u16, u16)>,
) -> Vec<(Rect, TuiAction<'a>)> {
    let mut actions = Vec::new();
    let mut row = 0u16;
    let mut col = 0u16;

    for (idx, node) in nodes.iter().enumerate() {
        if row >= area.height + scroll {
            break; // Past visible area
        }

        // Create a fresh path for each top-level node
        let mut node_path = NodePath::new();
        node_path.push(idx);
        render_node(
            node,
            format_context,
            area,
            buf,
            &mut row,
            &mut col,
            scroll,
            cursor_pos,
            &mut actions,
            &node_path,
        );
    }

    actions
}

/// Render a single node
#[allow(clippy::too_many_arguments)]
pub(super) fn render_node<'a>(
    node: &DocumentNode<'a>,
    format_context: &FormatContext,
    area: Rect,
    buf: &mut Buffer,
    row: &mut u16,
    col: &mut u16,
    scroll: u16,
    cursor_pos: Option<(u16, u16)>,
    actions: &mut Vec<(Rect, TuiAction<'a>)>,
    path: &crate::styled_string::NodePath,
) {
    match node {
        DocumentNode::Span(span) => {
            render_span(
                span,
                format_context,
                area,
                buf,
                row,
                col,
                scroll,
                cursor_pos,
                actions,
            );
        }

        DocumentNode::Heading { level, spans } => {
            // Start new line if not at beginning
            if *col > 0 {
                *row += 1;
                *col = 0;
            }

            // Render heading spans (bold)
            for span in spans {
                render_span_with_modifier(
                    span,
                    Modifier::BOLD,
                    format_context,
                    area,
                    buf,
                    row,
                    col,
                    scroll,
                    cursor_pos,
                    actions,
                );
            }

            // New line after heading
            *row += 1;
            *col = 0;

            // Add decorative underline
            let underline_char = match level {
                HeadingLevel::Title => '=',
                HeadingLevel::Section => '-',
            };

            if *row >= scroll && *row < scroll + area.height {
                for c in 0..area.width {
                    buf.cell_mut((c, *row - scroll))
                        .unwrap()
                        .set_char(underline_char);
                }
            }

            *row += 1;
            *col = 0;
        }

        DocumentNode::List { items } => {
            for (item_idx, item) in items.iter().enumerate() {
                // Start new line
                if *col > 0 {
                    *row += 1;
                    *col = 0;
                }

                // Bullet
                write_text(buf, *row, *col, "  • ", scroll, area, Style::default());
                *col += 4;

                // Label (if any)
                if let Some(label_spans) = &item.label {
                    for span in label_spans {
                        render_span_with_modifier(
                            span,
                            Modifier::BOLD,
                            format_context,
                            area,
                            buf,
                            row,
                            col,
                            scroll,
                            cursor_pos,
                            actions,
                        );
                    }
                }

                // Content
                for (content_idx, content_node) in item.content.iter().enumerate() {
                    let mut content_path = *path;
                    content_path.push(item_idx);
                    content_path.push(content_idx);
                    render_node(
                        content_node,
                        format_context,
                        area,
                        buf,
                        row,
                        col,
                        scroll,
                        cursor_pos,
                        actions,
                        &content_path,
                    );
                }

                *row += 1;
                *col = 0;
            }
        }

        DocumentNode::Section { title, nodes } => {
            if let Some(title_spans) = title {
                if *col > 0 {
                    *row += 1;
                    *col = 0;
                }

                for span in title_spans {
                    render_span_with_modifier(
                        span,
                        Modifier::BOLD,
                        format_context,
                        area,
                        buf,
                        row,
                        col,
                        scroll,
                        cursor_pos,
                        actions,
                    );
                }

                *row += 1;
                *col = 0;
            }

            for (idx, child_node) in nodes.iter().enumerate() {
                let mut child_path = *path;
                child_path.push(idx);
                render_node(
                    child_node,
                    format_context,
                    area,
                    buf,
                    row,
                    col,
                    scroll,
                    cursor_pos,
                    actions,
                    &child_path,
                );
            }
        }

        DocumentNode::CodeBlock { lang, code } => {
            if *col > 0 {
                *row += 1;
                *col = 0;
            }

            render_code_block(
                lang.as_deref(),
                code,
                format_context,
                area,
                buf,
                row,
                scroll,
            );

            *row += 1;
            *col = 0;
        }

        DocumentNode::Link { url, text, item } => {
            // Determine the action based on whether this is an internal or external link
            let action = if let Some(doc_ref) = item {
                TuiAction::Navigate(*doc_ref)
            } else {
                TuiAction::OpenUrl(url.clone())
            };

            // Render underlined text with the action attached
            for span in text {
                let span_with_action = Span {
                    text: span.text.clone(),
                    style: span.style,
                    action: Some(action.clone()),
                };
                render_span_with_modifier(
                    &span_with_action,
                    Modifier::UNDERLINED,
                    format_context,
                    area,
                    buf,
                    row,
                    col,
                    scroll,
                    cursor_pos,
                    actions,
                );
            }
        }

        DocumentNode::HorizontalRule => {
            if *col > 0 {
                *row += 1;
                *col = 0;
            }

            if *row >= scroll && *row < scroll + area.height {
                for c in 0..area.width {
                    buf.cell_mut((c, *row - scroll)).unwrap().set_char('─');
                }
            }

            *row += 1;
            *col = 0;
        }

        DocumentNode::BlockQuote { nodes } => {
            for (idx, child_node) in nodes.iter().enumerate() {
                if *col == 0 {
                    write_text(buf, *row, *col, "  │ ", scroll, area, Style::default());
                    *col += 4;
                }

                let mut child_path = *path;
                child_path.push(idx);
                render_node(
                    child_node,
                    format_context,
                    area,
                    buf,
                    row,
                    col,
                    scroll,
                    cursor_pos,
                    actions,
                    &child_path,
                );
            }
        }

        DocumentNode::Table { .. } => {
            if *col > 0 {
                *row += 1;
                *col = 0;
            }

            write_text(buf, *row, *col, "[Table]", scroll, area, Style::default());

            *row += 1;
            *col = 0;
        }

        DocumentNode::TruncatedBlock { nodes, level } => {
            // Determine line limit based on truncation level
            let line_limit = match level {
                TruncationLevel::SingleLine => 3,  // Show ~3 lines for single-line
                TruncationLevel::Brief => 8,       // Show ~8 lines for brief (actual wrapped lines)
                TruncationLevel::Full => u16::MAX, // Show everything
            };

            let start_row = *row;
            let mut rendered_all = true;

            // For Brief mode, try to find a good truncation point at second paragraph break
            let truncate_at = if matches!(level, TruncationLevel::Brief) {
                find_paragraph_truncation_point(nodes, line_limit, area.width)
            } else {
                None
            };

            // Render nodes until we hit the truncation point or line limit
            for (idx, child_node) in nodes.iter().enumerate() {
                // Check if we've hit our truncation point
                if let Some(cutoff) = truncate_at {
                    if idx >= cutoff {
                        rendered_all = false;
                        break;
                    }
                }

                // Check if we've exceeded the line limit (fallback)
                if *row - start_row >= line_limit && !matches!(level, TruncationLevel::Full) {
                    rendered_all = false;
                    break;
                }

                let mut child_path = *path;
                child_path.push(idx);
                render_node(
                    child_node,
                    format_context,
                    area,
                    buf,
                    row,
                    col,
                    scroll,
                    cursor_pos,
                    actions,
                    &child_path,
                );

                // If this is the last node, we rendered everything
                if idx == nodes.len() - 1 {
                    rendered_all = true;
                }
            }

            // Only show [...] if we didn't render all nodes and not already Full
            if !rendered_all && !matches!(level, TruncationLevel::Full) {
                let start_col = *col;
                let ellipsis_row = *row;
                let ellipsis_text = " [...]";

                // Style for dimmed ellipsis
                let style = Style::default().fg(Color::DarkGray);

                // Check if hovered
                let is_hovered = cursor_pos.map_or_else(
                    || false,
                    |(cx, cy)| {
                        cy == ellipsis_row
                            && cx >= start_col
                            && cx < start_col + ellipsis_text.len() as u16
                    },
                );

                let final_style = if is_hovered {
                    style.add_modifier(Modifier::REVERSED)
                } else {
                    style
                };

                // Write the text
                write_text(
                    buf,
                    ellipsis_row,
                    *col,
                    ellipsis_text,
                    scroll,
                    area,
                    final_style,
                );
                *col += ellipsis_text.len() as u16;

                // Track the action with the current path
                let rect = Rect::new(start_col, ellipsis_row, ellipsis_text.len() as u16, 1);
                actions.push((rect, TuiAction::ExpandBlock(*path)));
            }
        }
    }
}

/// Render a span with optional action tracking
#[allow(clippy::too_many_arguments)]
pub(super) fn render_span<'a>(
    span: &Span<'a>,
    format_context: &FormatContext,
    area: Rect,
    buf: &mut Buffer,
    row: &mut u16,
    col: &mut u16,
    scroll: u16,
    cursor_pos: Option<(u16, u16)>,
    actions: &mut Vec<(Rect, TuiAction<'a>)>,
) {
    render_span_with_modifier(
        span,
        Modifier::empty(),
        format_context,
        area,
        buf,
        row,
        col,
        scroll,
        cursor_pos,
        actions,
    );
}

/// Render a span with additional style modifier
#[allow(clippy::too_many_arguments)]
pub(super) fn render_span_with_modifier<'a>(
    span: &Span<'a>,
    modifier: Modifier,
    format_context: &FormatContext,
    area: Rect,
    buf: &mut Buffer,
    row: &mut u16,
    col: &mut u16,
    scroll: u16,
    cursor_pos: Option<(u16, u16)>,
    actions: &mut Vec<(Rect, TuiAction<'a>)>,
) {
    let mut style = span_style_to_ratatui(span.style, format_context);
    style = style.add_modifier(modifier);

    let start_col = *col;
    let start_row = *row;

    // Check if this span is hovered
    let is_hovered = if span.action.is_some() {
        cursor_pos.map_or_else(
            || false,
            |(cx, cy)| cy == *row && cx >= *col && cx < *col + span.text.len() as u16,
        )
    } else {
        false
    };

    // If hovered, invert colors
    if is_hovered {
        style = style.add_modifier(Modifier::REVERSED);
    }

    // Handle newlines in span text
    for (line_idx, line) in span.text.split('\n').enumerate() {
        if line_idx > 0 {
            *row += 1;
            *col = 0;
        }

        // Word wrap if line is too long
        let mut remaining = line;
        while !remaining.is_empty() {
            let available_width = area.width.saturating_sub(*col);

            if available_width == 0 {
                // No space left on this line, wrap to next
                *row += 1;
                *col = 0;
                continue;
            }

            if remaining.len() <= available_width as usize {
                // Fits on current line
                write_text(buf, *row, *col, remaining, scroll, area, style);
                *col += remaining.len() as u16;
                break;
            } else {
                // Need to wrap - find last space within available width
                let truncate_at = available_width as usize;
                if let Some(wrap_pos) = remaining[..truncate_at].rfind(char::is_whitespace) {
                    // Wrap at word boundary
                    let (chunk, rest) = remaining.split_at(wrap_pos);
                    write_text(buf, *row, *col, chunk, scroll, area, style);
                    *row += 1;
                    *col = 0;
                    remaining = rest.trim_start(); // Skip leading whitespace on next line
                } else {
                    // No spaces found, hard wrap
                    let (chunk, rest) = remaining.split_at(truncate_at);
                    write_text(buf, *row, *col, chunk, scroll, area, style);
                    *row += 1;
                    *col = 0;
                    remaining = rest;
                }
            }
        }
    }

    // Track action if present
    if let Some(action) = &span.action {
        // Calculate width handling wrapping (col might be less than start_col if we wrapped)
        let width = if *row > start_row {
            // Multi-line span - use full width of first line as clickable area
            area.width.saturating_sub(start_col).max(1)
        } else {
            // Single line - use actual span width
            col.saturating_sub(start_col).max(1)
        };

        let rect = Rect::new(start_col, start_row, width, (*row - start_row + 1).max(1));
        actions.push((rect, action.clone()));
    }
}

/// Write text to buffer at position
fn write_text(
    buf: &mut Buffer,
    row: u16,
    col: u16,
    text: &str,
    scroll: u16,
    area: Rect,
    style: Style,
) {
    if row < scroll || row >= scroll + area.height {
        return; // Outside visible area
    }

    let screen_row = row - scroll;
    let mut current_col = col;

    for ch in text.chars() {
        if current_col >= area.width {
            break; // Past right edge
        }

        if let Some(cell) = buf.cell_mut((current_col, screen_row)) {
            cell.set_char(ch);
            cell.set_style(style);
        }

        current_col += 1;
    }
}

/// Render code block with syntax highlighting
pub(super) fn render_code_block(
    lang: Option<&str>,
    code: &str,
    format_context: &FormatContext,
    area: Rect,
    buf: &mut Buffer,
    row: &mut u16,
    scroll: u16,
) {
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
            if *row >= scroll && *row < scroll + area.height {
                let mut col = 0u16;

                if let Ok(ranges) = highlighter.highlight_line(line, format_context.syntax_set()) {
                    for (style, text) in ranges {
                        let fg = style.foreground;
                        let ratatui_style = Style::default().fg(Color::Rgb(fg.r, fg.g, fg.b));
                        write_text(
                            buf,
                            *row,
                            col,
                            text.trim_end_matches('\n'),
                            scroll,
                            area,
                            ratatui_style,
                        );
                        col += text.len() as u16;
                    }
                } else {
                    write_text(
                        buf,
                        *row,
                        0,
                        line.trim_end_matches('\n'),
                        scroll,
                        area,
                        Style::default(),
                    );
                }
            }

            *row += 1;
        }
    } else {
        for line in code.lines() {
            if *row >= scroll && *row < scroll + area.height {
                write_text(buf, *row, 0, line, scroll, area, Style::default());
            }
            *row += 1;
        }
    }
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
