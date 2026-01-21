use super::theme::InteractiveTheme;
use super::utils::find_paragraph_truncation_point;
use crate::format_context::FormatContext;
use crate::styled_string::{
    DocumentNode, HeadingLevel, NodePath, Span, SpanStyle, TableCell, TruncationLevel, TuiAction,
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
    theme: &InteractiveTheme,
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
            theme,
            2, // 2-space left margin for breathing room
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
    theme: &InteractiveTheme,
    left_margin: u16,
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
                left_margin,
            );
        }

        DocumentNode::Heading { level, spans } => {
            // Start new line if not at beginning
            if *col > 0 {
                *row += 1;
                *col = left_margin;
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
                    left_margin,
                );
            }

            // New line after heading
            *row += 1;
            *col = left_margin;

            // Add decorative underline (respecting left margin)
            let underline_char = match level {
                HeadingLevel::Title => '=',
                HeadingLevel::Section => '-',
            };

            if *row >= scroll && *row < scroll + area.height {
                for c in left_margin..area.width {
                    buf.cell_mut((c, *row - scroll))
                        .unwrap()
                        .set_char(underline_char);
                }
            }

            *row += 1;
            *col = left_margin;
        }

        DocumentNode::List { items } => {
            for (item_idx, item) in items.iter().enumerate() {
                // Start new line
                if *col > 0 {
                    *row += 1;
                    *col = left_margin;
                }

                // Bullet with nice unicode character
                let bullet_style = theme.muted_style;
                write_text(buf, *row, *col, "  ◦ ", scroll, area, bullet_style);
                *col += 4;

                // Capture left margin for content right after bullet
                let content_left_margin = *col;

                // Label (if any) - should wrap to content_left_margin, not parent's left_margin
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
                            content_left_margin,
                        );
                    }
                }
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
                        theme,
                        content_left_margin,
                    );
                }

                *row += 1;
                *col = left_margin;
            }
        }

        DocumentNode::Section { title, nodes } => {
            if let Some(title_spans) = title {
                if *col > 0 {
                    *row += 1;
                    *col = left_margin;
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
                        left_margin,
                    );
                }

                *row += 1;
                *col = left_margin;
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
                    theme,
                    left_margin,
                );
            }
        }

        DocumentNode::CodeBlock { lang, code } => {
            if *col > 0 {
                *row += 1;
                *col = left_margin;
            }

            render_code_block(
                lang.as_deref(),
                code,
                format_context,
                area,
                buf,
                row,
                scroll,
                theme,
                left_margin,
            );

            *row += 1;
            *col = left_margin;
        }

        DocumentNode::Link { url, text, item } => {
            // Determine the action based on whether this is an internal or external link
            let action = if let Some(doc_ref) = item {
                TuiAction::Navigate(*doc_ref)
            } else {
                TuiAction::OpenUrl(url.clone())
            };

            // Calculate total length of link text to avoid splitting it across lines
            let total_length: usize = text.iter().map(|s| s.text.len()).sum();
            let available_width = area.width.saturating_sub(*col);

            // If link won't fit on current line, wrap to next line first
            if total_length as u16 > available_width && *col > left_margin {
                *row += 1;
                *col = left_margin;
            }

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
                    left_margin,
                );
            }
        }

        DocumentNode::HorizontalRule => {
            if *col > 0 {
                *row += 1;
                *col = left_margin;
            }

            if *row >= scroll && *row < scroll + area.height {
                let rule_style = theme.muted_style;
                // Use a decorative pattern: ─── • ───
                let pattern = ['─', '─', '─', ' ', '•', ' '];
                for c in 0..area.width {
                    let ch = pattern[(c as usize) % pattern.len()];
                    if let Some(cell) = buf.cell_mut((c, *row - scroll)) {
                        cell.set_char(ch);
                        cell.set_style(rule_style);
                    }
                }
            }

            *row += 1;
            *col = left_margin;
        }

        DocumentNode::BlockQuote { nodes } => {
            for (idx, child_node) in nodes.iter().enumerate() {
                if *col == left_margin {
                    // Use a thicker vertical bar for quotes
                    let quote_style = theme.muted_style;
                    write_text(buf, *row, *col, "  ┃ ", scroll, area, quote_style);
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
                    theme,
                    left_margin,
                );
            }
        }

        DocumentNode::Table { header, rows } => {
            if *col > 0 {
                *row += 1;
                *col = left_margin;
            }

            render_table(
                header.as_deref(),
                rows,
                format_context,
                area,
                buf,
                row,
                scroll,
                theme,
            );

            *row += 1;
            *col = left_margin;
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
            let border_style = theme.muted_style;

            // For SingleLine with heading as first node, just show the heading text
            let render_nodes = if matches!(level, TruncationLevel::SingleLine) {
                // Check if first node is a heading
                if let Some(DocumentNode::Heading { spans, .. }) = nodes.first() {
                    // Just render the spans without the heading decoration
                    for span in spans {
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
                            left_margin,
                        );
                    }
                    rendered_all = nodes.len() <= 1;
                    false // Skip normal rendering
                } else {
                    true
                }
            } else {
                true
            };

            if render_nodes {
                // For Brief mode, try to find a good truncation point at second paragraph break
                let truncate_at = if matches!(level, TruncationLevel::Brief) {
                    find_paragraph_truncation_point(nodes, line_limit, area.width)
                } else {
                    None
                };

                // Increase left margin for content to make room for border
                let content_left_margin = if !matches!(level, TruncationLevel::Full) {
                    left_margin + 2
                } else {
                    left_margin
                };

                // Track last row with actual content (to trim trailing blank lines)
                let mut last_content_row = start_row;

                // Render nodes
                for (idx, child_node) in nodes.iter().enumerate() {
                    // Check if we've hit our truncation point
                    if let Some(cutoff) = truncate_at
                        && idx >= cutoff {
                            rendered_all = false;
                            break;
                        }

                    // Check if we've exceeded the line limit (fallback)
                    if *row - start_row >= line_limit && !matches!(level, TruncationLevel::Full) {
                        rendered_all = false;
                        break;
                    }

                    // Skip headings in the middle of truncated content (not first node)
                    // Only do this for Brief/SingleLine, not Full
                    if idx > 0
                        && !matches!(level, TruncationLevel::Full)
                        && matches!(child_node, DocumentNode::Heading { .. })
                    {
                        rendered_all = false;
                        break;
                    }

                    // If we're at the left margin, move to content area
                    if !matches!(level, TruncationLevel::Full) && *col == left_margin {
                        *col = content_left_margin;
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
                        theme,
                        content_left_margin,
                    );

                    // Track last row with content (not just blank lines)
                    if *col > content_left_margin {
                        last_content_row = *row;
                    }

                    // If this is the last node, we rendered everything
                    if idx == nodes.len() - 1 {
                        rendered_all = true;
                    }
                }

                // Draw left border on all lines with content (trim trailing blank lines)
                if !matches!(level, TruncationLevel::Full) {
                    // Draw borders only up to the last row with actual content
                    let end_row = last_content_row + 1;

                    for r in start_row..end_row {
                        if r >= scroll && r < scroll + area.height {
                            write_text(buf, r, left_margin, "│ ", scroll, area, border_style);
                        }
                    }

                    // Move to the row after last content for the closing border
                    *row = last_content_row + 1;
                    *col = left_margin;
                }
            }

            // Show bottom border with [...] if we didn't render all nodes
            if !rendered_all && !matches!(level, TruncationLevel::Full) {
                let ellipsis_text = "╰─[...]";
                let ellipsis_row = *row;

                // Check if hovered
                let is_hovered = cursor_pos.map_or_else(
                    || false,
                    |(cx, cy)| {
                        cy == ellipsis_row
                            && cx >= left_margin
                            && cx < left_margin + ellipsis_text.len() as u16
                    },
                );

                let final_style = if is_hovered {
                    border_style.add_modifier(Modifier::REVERSED)
                } else {
                    border_style
                };

                // Write the border with ellipsis
                write_text(
                    buf,
                    ellipsis_row,
                    left_margin,
                    ellipsis_text,
                    scroll,
                    area,
                    final_style,
                );
                *col = left_margin + ellipsis_text.len() as u16;

                // Track the action with the current path
                let rect = Rect::new(left_margin, ellipsis_row, ellipsis_text.len() as u16, 1);
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
    left_margin: u16,
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
        left_margin,
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
    left_margin: u16,
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
            *col = left_margin;
        }

        // Word wrap if line is too long
        let mut remaining = line;
        while !remaining.is_empty() {
            // Calculate available width: columns from current to edge (exclusive)
            // area.width is the total width, so valid columns are 0 to area.width-1
            let available_width = area.width.saturating_sub(*col);

            if available_width == 0 {
                // No space left on this line, wrap to next
                *row += 1;
                *col = left_margin;
                continue;
            }

            if remaining.len() <= available_width as usize {
                // Fits on current line
                write_text(buf, *row, *col, remaining, scroll, area, style);
                *col += remaining.len() as u16;
                break;
            } else {
                // Need to wrap - find best break point
                let truncate_at = available_width as usize;

                // First try to find a good break point (whitespace or after punctuation)
                let wrap_pos = find_wrap_position(remaining, truncate_at);

                if let Some(pos) = wrap_pos {
                    let (chunk, rest) = remaining.split_at(pos);
                    write_text(buf, *row, *col, chunk, scroll, area, style);
                    *row += 1;
                    *col = left_margin;
                    remaining = rest.trim_start(); // Skip leading whitespace on next line
                } else {
                    // No good break point within available width
                    // Look for the next break point beyond the available width
                    // This creates ragged right margins but avoids splitting words
                    if let Some(next_space) = remaining.find(char::is_whitespace) {
                        // Check if the word will fit on the current line
                        if next_space <= available_width as usize {
                            // Word fits on current line, write it
                            let (chunk, rest) = remaining.split_at(next_space);
                            write_text(buf, *row, *col, chunk, scroll, area, style);
                            *row += 1;
                            *col = left_margin;
                            remaining = rest.trim_start();
                        } else {
                            // Word doesn't fit, wrap to next line first
                            *row += 1;
                            *col = left_margin;
                            // Don't modify remaining, continue on next line and try again
                        }
                    } else {
                        // No whitespace at all in remaining text
                        // If it fits, write it; otherwise wrap first
                        if remaining.len() <= available_width as usize {
                            write_text(buf, *row, *col, remaining, scroll, area, style);
                            *col += remaining.len() as u16;
                            break;
                        } else {
                            // Doesn't fit, wrap to next line
                            *row += 1;
                            *col = left_margin;
                            // Continue to try writing on next line
                        }
                    }
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
    theme: &InteractiveTheme,
    left_margin: u16,
) {
    let lang_display = match lang {
        Some("no_run") | Some("should_panic") | Some("ignore") | Some("compile_fail")
        | Some("edition2015") | Some("edition2018") | Some("edition2021") | Some("edition2024") => {
            "rust"
        }
        Some(l) => l,
        None => "rust",
    };

    // Calculate code block dimensions accounting for left margin
    let available_width = area.width.saturating_sub(left_margin);
    let max_line_width = code
        .lines()
        .map(|line| line.len())
        .max()
        .unwrap_or(0)
        .min((available_width.saturating_sub(4)) as usize); // Leave room for border and padding

    // Account for language label in border width: ╭───❬rust❭─╮
    let lang_label = format!("❬{}❭", lang_display);
    // Count actual display width (number of grapheme clusters, not bytes)
    let label_display_width = lang_label.chars().count();
    let min_border_for_label = label_display_width as u16 + 6; // label + some padding
    let border_width = ((max_line_width + 4).max(min_border_for_label as usize))
        .min(available_width as usize) as u16;

    let border_style = theme.code_block_border_style;

    // Top border with language label: ╭─────❬rust❭─╮
    if *row >= scroll && *row < scroll + area.height {
        write_text(buf, *row, left_margin, "╭", scroll, area, border_style);

        // Calculate position for language label (right side, with one dash before corner)
        // Label ends at border_width - 2 (leaving space for ─╮)
        let label_start = left_margin + border_width.saturating_sub(label_display_width as u16 + 2);

        // Draw left dashes (up to label)
        for i in 1..label_start.saturating_sub(left_margin) {
            write_text(buf, *row, left_margin + i, "─", scroll, area, border_style);
        }

        // Draw language label
        write_text(
            buf,
            *row,
            label_start,
            &lang_label,
            scroll,
            area,
            border_style,
        );

        // Draw dashes from end of label to corner
        // The label takes label_display_width columns, so next position is label_start + label_display_width
        let label_end_col = label_start + label_display_width as u16;
        for i in label_end_col..left_margin + border_width.saturating_sub(1) {
            write_text(buf, *row, i, "─", scroll, area, border_style);
        }

        // Draw corner
        write_text(
            buf,
            *row,
            left_margin + border_width.saturating_sub(1),
            "╮",
            scroll,
            area,
            border_style,
        );
    }
    *row += 1;

    // Render code content with side borders (no background color)
    if let Some(syntax) = format_context
        .syntax_set()
        .find_syntax_by_token(lang_display)
    {
        let theme = format_context.theme();
        let mut highlighter = HighlightLines::new(syntax, theme);

        for line in LinesWithEndings::from(code) {
            if *row >= scroll && *row < scroll + area.height {
                // Left border and padding
                write_text(buf, *row, left_margin, "│ ", scroll, area, border_style);

                let mut col = left_margin + 2;

                if let Ok(ranges) = highlighter.highlight_line(line, format_context.syntax_set()) {
                    for (style, text) in ranges {
                        let fg = style.foreground;
                        let ratatui_style = Style::default().fg(Color::Rgb(fg.r, fg.g, fg.b));
                        let text = text.trim_end_matches('\n');

                        write_text(buf, *row, col, text, scroll, area, ratatui_style);
                        col += text.len() as u16;
                    }
                } else {
                    write_text(
                        buf,
                        *row,
                        left_margin + 2,
                        line.trim_end_matches('\n'),
                        scroll,
                        area,
                        Style::default(),
                    );
                }

                // Right border and padding
                write_text(
                    buf,
                    *row,
                    left_margin + border_width.saturating_sub(2),
                    " │",
                    scroll,
                    area,
                    border_style,
                );
            }

            *row += 1;
        }
    } else {
        for line in code.lines() {
            if *row >= scroll && *row < scroll + area.height {
                // Left border and padding
                write_text(buf, *row, left_margin, "│ ", scroll, area, border_style);

                // Code content
                write_text(
                    buf,
                    *row,
                    left_margin + 2,
                    line,
                    scroll,
                    area,
                    Style::default(),
                );

                // Right border and padding
                write_text(
                    buf,
                    *row,
                    left_margin + border_width.saturating_sub(2),
                    " │",
                    scroll,
                    area,
                    border_style,
                );
            }
            *row += 1;
        }
    }

    // Bottom border: ╰─────╯
    if *row >= scroll && *row < scroll + area.height {
        write_text(buf, *row, left_margin, "╰", scroll, area, border_style);
        for i in 1..border_width.saturating_sub(1) {
            write_text(buf, *row, left_margin + i, "─", scroll, area, border_style);
        }
        write_text(
            buf,
            *row,
            left_margin + border_width.saturating_sub(1),
            "╯",
            scroll,
            area,
            border_style,
        );
    }
    *row += 1;
}

/// Render table with unicode borders
pub(super) fn render_table<'a>(
    header: Option<&[TableCell<'a>]>,
    rows: &[Vec<TableCell<'a>>],
    format_context: &FormatContext,
    area: Rect,
    buf: &mut Buffer,
    row: &mut u16,
    scroll: u16,
    _theme: &InteractiveTheme,
) {
    if rows.is_empty() && header.is_none() {
        return;
    }

    let border_style = Style::default().fg(Color::Rgb(60, 60, 70));

    // Calculate column widths based on content
    let num_cols = header
        .map(|h| h.len())
        .or_else(|| rows.first().map(|r| r.len()))
        .unwrap_or(0);

    if num_cols == 0 {
        return;
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

    // Cap column widths to reasonable sizes and calculate total width
    let max_col_width = 40;
    for width in &mut col_widths {
        *width = (*width).min(max_col_width);
    }

    // Top border: ┌─────┬─────┐
    if *row >= scroll && *row < scroll + area.height {
        let mut col_pos = 0u16;
        write_text(buf, *row, col_pos, "┌", scroll, area, border_style);
        col_pos += 1;

        for (idx, &width) in col_widths.iter().enumerate() {
            for _ in 0..width {
                write_text(buf, *row, col_pos, "─", scroll, area, border_style);
                col_pos += 1;
            }
            if idx < col_widths.len() - 1 {
                write_text(buf, *row, col_pos, "┬", scroll, area, border_style);
                col_pos += 1;
            }
        }

        write_text(buf, *row, col_pos, "┐", scroll, area, border_style);
    }
    *row += 1;

    // Render header if present
    if let Some(header_cells) = header {
        if *row >= scroll && *row < scroll + area.height {
            let mut col_pos = 0u16;
            write_text(buf, *row, col_pos, "│", scroll, area, border_style);
            col_pos += 1;

            for (col_idx, cell) in header_cells.iter().enumerate() {
                // Render cell content (bold for headers)
                let mut cell_col = col_pos;
                for span in &cell.spans {
                    let span_text = if span.text.len() > col_widths[col_idx] {
                        &span.text[..col_widths[col_idx]]
                    } else {
                        &span.text
                    };

                    let mut style = span_style_to_ratatui(span.style, format_context);
                    style = style.add_modifier(Modifier::BOLD);

                    write_text(buf, *row, cell_col, span_text, scroll, area, style);
                    cell_col += span_text.len() as u16;
                }

                // Pad to column width
                while cell_col < col_pos + col_widths[col_idx] as u16 {
                    write_text(buf, *row, cell_col, " ", scroll, area, Style::default());
                    cell_col += 1;
                }

                col_pos = cell_col;
                write_text(buf, *row, col_pos, "│", scroll, area, border_style);
                col_pos += 1;
            }
        }
        *row += 1;

        // Header separator: ├─────┼─────┤
        if *row >= scroll && *row < scroll + area.height {
            let mut col_pos = 0u16;
            write_text(buf, *row, col_pos, "├", scroll, area, border_style);
            col_pos += 1;

            for (idx, &width) in col_widths.iter().enumerate() {
                for _ in 0..width {
                    write_text(buf, *row, col_pos, "─", scroll, area, border_style);
                    col_pos += 1;
                }
                if idx < col_widths.len() - 1 {
                    write_text(buf, *row, col_pos, "┼", scroll, area, border_style);
                    col_pos += 1;
                }
            }

            write_text(buf, *row, col_pos, "┤", scroll, area, border_style);
        }
        *row += 1;
    }

    // Render rows
    for row_cells in rows.iter() {
        if *row >= scroll && *row < scroll + area.height {
            let mut col_pos = 0u16;
            write_text(buf, *row, col_pos, "│", scroll, area, border_style);
            col_pos += 1;

            for (col_idx, cell) in row_cells.iter().enumerate() {
                if col_idx >= num_cols {
                    break;
                }

                // Render cell content
                let mut cell_col = col_pos;
                for span in &cell.spans {
                    let span_text = if span.text.len() > col_widths[col_idx] {
                        &span.text[..col_widths[col_idx]]
                    } else {
                        &span.text
                    };

                    let style = span_style_to_ratatui(span.style, format_context);
                    write_text(buf, *row, cell_col, span_text, scroll, area, style);
                    cell_col += span_text.len() as u16;
                }

                // Pad to column width
                while cell_col < col_pos + col_widths[col_idx] as u16 {
                    write_text(buf, *row, cell_col, " ", scroll, area, Style::default());
                    cell_col += 1;
                }

                col_pos = cell_col;
                write_text(buf, *row, col_pos, "│", scroll, area, border_style);
                col_pos += 1;
            }
        }
        *row += 1;
    }

    // Bottom border: └─────┴─────┘
    if *row >= scroll && *row < scroll + area.height {
        let mut col_pos = 0u16;
        write_text(buf, *row, col_pos, "└", scroll, area, border_style);
        col_pos += 1;

        for (idx, &width) in col_widths.iter().enumerate() {
            for _ in 0..width {
                write_text(buf, *row, col_pos, "─", scroll, area, border_style);
                col_pos += 1;
            }
            if idx < col_widths.len() - 1 {
                write_text(buf, *row, col_pos, "┴", scroll, area, border_style);
                col_pos += 1;
            }
        }

        write_text(buf, *row, col_pos, "┘", scroll, area, border_style);
    }
}

/// Find the best position to wrap text within a given width
/// Returns the position after which to break, or None if no good break point exists
fn find_wrap_position(text: &str, max_width: usize) -> Option<usize> {
    if max_width == 0 || text.is_empty() {
        return None;
    }

    let search_range = &text[..max_width.min(text.len())];

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
