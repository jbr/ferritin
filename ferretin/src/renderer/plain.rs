use std::fmt::{Result, Write};

use crate::styled_string::{Document, DocumentNode, HeadingLevel, ListItem, Span, TruncationLevel};

/// Render a document as plain text without any styling
pub fn render(document: &Document, output: &mut impl Write) -> Result {
    render_nodes(&document.nodes, output)
}

fn render_nodes(nodes: &[DocumentNode], output: &mut impl Write) -> Result {
    for node in nodes {
        render_node(node, output)?;
    }
    Ok(())
}

fn render_node(node: &DocumentNode, output: &mut impl Write) -> Result {
    match node {
        DocumentNode::Span(span) => render_span(span, output),
        DocumentNode::Heading { level, spans } => {
            render_spans(spans, output)?;
            writeln!(output)?;
            // Add underlines for headings
            match level {
                HeadingLevel::Title => {
                    for _ in 0..80 {
                        write!(output, "=")?;
                    }
                    writeln!(output)?;
                }
                HeadingLevel::Section => {
                    for _ in 0..80 {
                        write!(output, "-")?;
                    }
                    writeln!(output)?;
                }
            }
            Ok(())
        }
        DocumentNode::Section { title, nodes } => {
            if let Some(title_spans) = title {
                render_spans(title_spans, output)?;
                writeln!(output)?;
            }
            render_nodes(nodes, output)
        }
        DocumentNode::List { items } => {
            for item in items {
                render_list_item(item, output)?;
            }
            Ok(())
        }
        DocumentNode::CodeBlock { code, .. } => {
            writeln!(output, "```\n{code}")?;
            if !code.ends_with('\n') {
                writeln!(output)?;
            }
            writeln!(output, "```\n")?;
            Ok(())
        }
        DocumentNode::Link { text, .. } => {
            // In plain mode, just render the link text
            render_spans(text, output)
        }
        DocumentNode::HorizontalRule => {
            for _ in 0..80 {
                write!(output, "─")?;
            }
            writeln!(output)?;
            Ok(())
        }
        DocumentNode::BlockQuote { nodes } => {
            write!(output, "> ")?;
            render_nodes(nodes, output)?;
            writeln!(output)?;
            Ok(())
        }
        DocumentNode::Table { header, rows } => {
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
            Ok(())
        }
        DocumentNode::TruncatedBlock { nodes, level } => {
            match level {
                TruncationLevel::SingleLine => {
                    // Render until first newline
                    let has_more = !render_until_newline(nodes, output)?;
                    if has_more {
                        write!(output, " [...]")?;
                    }
                    writeln!(output)?;
                }
                TruncationLevel::Brief => {
                    // Render until first double newline (paragraph break)
                    let remaining = render_until_paragraph_break(nodes, output)?;
                    if remaining > 0 {
                        writeln!(output)?;
                        write!(output, "[+{remaining} more paragraphs]")?;
                        writeln!(output)?;
                    }
                }
                TruncationLevel::Full => {
                    // Render everything
                    render_nodes(nodes, output)?;
                }
            }
            Ok(())
        }
    }
}

/// Render nodes until first newline
/// Returns true if we rendered everything (no newline found), false if truncated
fn render_until_newline(
    nodes: &[DocumentNode],
    output: &mut impl Write,
) -> std::result::Result<bool, std::fmt::Error> {
    for node in nodes {
        if !render_node_until_newline(node, output)? {
            return Ok(false); // Truncated
        }
    }
    Ok(true) // Rendered everything
}

fn render_node_until_newline(
    node: &DocumentNode,
    output: &mut impl Write,
) -> std::result::Result<bool, std::fmt::Error> {
    match node {
        DocumentNode::Span(span) => {
            if let Some(pos) = span.text.find('\n') {
                write!(output, "{}", &span.text[..pos])?;
                Ok(false) // Found newline, truncated
            } else {
                render_span(span, output)?;
                Ok(true) // No newline, continue
            }
        }
        DocumentNode::Heading { .. } | DocumentNode::CodeBlock { .. } => {
            // These introduce newlines, so stop here
            Ok(false)
        }
        DocumentNode::Section { nodes, .. }
        | DocumentNode::BlockQuote { nodes }
        | DocumentNode::TruncatedBlock { nodes, .. } => render_until_newline(nodes, output),
        DocumentNode::List { .. } | DocumentNode::Table { .. } | DocumentNode::HorizontalRule => {
            // These are multi-line structures, stop here
            Ok(false)
        }
        DocumentNode::Link { text, .. } => {
            for span in text {
                if !render_node_until_newline(&DocumentNode::Span(span.clone()), output)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
    }
}

/// Render until first paragraph break (\n\n)
/// Returns number of remaining paragraphs
fn render_until_paragraph_break(
    nodes: &[DocumentNode],
    output: &mut impl Write,
) -> std::result::Result<usize, std::fmt::Error> {
    let mut last_was_newline = false;
    let mut found_break = false;
    let mut remaining_paras = 0;

    for node in nodes {
        if found_break {
            remaining_paras += count_paragraphs_in_node(node);
        } else {
            let (stop, last_newline) =
                render_node_until_paragraph_break(node, output, last_was_newline)?;
            if stop {
                found_break = true;
                remaining_paras = 1; // At least one more paragraph
            }
            last_was_newline = last_newline;
        }
    }

    Ok(remaining_paras)
}

fn render_node_until_paragraph_break(
    node: &DocumentNode,
    output: &mut impl Write,
    last_was_newline: bool,
) -> std::result::Result<(bool, bool), std::fmt::Error> {
    match node {
        DocumentNode::Span(span) => {
            // Check for \n\n pattern
            if last_was_newline && span.text.starts_with('\n') {
                return Ok((true, false)); // Found paragraph break
            }
            if span.text.contains("\n\n")
                && let Some(pos) = span.text.find("\n\n")
            {
                write!(output, "{}", &span.text[..pos])?;
                return Ok((true, false)); // Found paragraph break
            }
            render_span(span, output)?;
            Ok((false, span.text.ends_with('\n')))
        }
        _ => {
            // For other nodes, just render them
            render_node(node, output)?;
            Ok((false, false))
        }
    }
}

fn count_paragraphs_in_node(node: &DocumentNode) -> usize {
    match node {
        DocumentNode::Span(span) => span.text.matches("\n\n").count(),
        DocumentNode::Section { nodes, .. }
        | DocumentNode::BlockQuote { nodes }
        | DocumentNode::TruncatedBlock { nodes, .. } => {
            nodes.iter().map(count_paragraphs_in_node).sum()
        }
        DocumentNode::List { items } => items
            .iter()
            .map(|item| {
                item.content
                    .iter()
                    .map(count_paragraphs_in_node)
                    .sum::<usize>()
            })
            .sum(),
        _ => 0,
    }
}

fn render_spans(spans: &[Span], output: &mut impl Write) -> Result {
    for span in spans {
        render_span(span, output)?;
    }
    Ok(())
}

fn render_span(Span { text, .. }: &Span, output: &mut impl Write) -> Result {
    write!(output, "{text}")?;
    Ok(())
}

fn render_list_item(item: &ListItem, output: &mut impl Write) -> Result {
    write!(output, "  • ")?;
    if let Some(label) = &item.label {
        render_spans(label, output)?;
    }
    render_nodes(&item.content, output)?;
    writeln!(output)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_spans() {
        let doc = Document::with_nodes(vec![
            DocumentNode::Span(Span::keyword("struct")),
            DocumentNode::Span(Span::plain(" ")),
            DocumentNode::Span(Span::type_name("Foo")),
        ]);

        let mut output = String::new();
        render(&doc, &mut output).unwrap();
        assert_eq!(output, "struct Foo");
    }

    #[test]
    fn test_render_heading() {
        let doc = Document::with_nodes(vec![DocumentNode::heading(
            HeadingLevel::Title,
            vec![Span::plain("Item: "), Span::type_name("Vec")],
        )]);
        let mut output = String::new();
        render(&doc, &mut output).unwrap();
        assert!(output.contains("Item: Vec"));
        assert!(output.contains("===="));
    }

    #[test]
    fn test_render_list() {
        let doc = Document::with_nodes(vec![DocumentNode::list(vec![
            ListItem::from_span(Span::plain("First")),
            ListItem::from_span(Span::plain("Second")),
        ])]);

        let mut output = String::new();
        render(&doc, &mut output).unwrap();

        assert!(output.contains("  • First"));
        assert!(output.contains("  • Second"));
    }
}
