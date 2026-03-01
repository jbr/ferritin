use std::fmt::{Result, Write};

use crate::document::{
    Document, DocumentNode, HeadingLevel, ListItem, ShowWhen, Span, SpanStyle, TruncationLevel,
};

/// Render a document with semantic XML-like tags for testing
pub fn render(document: &Document, output: &mut impl Write) -> Result {
    render_nodes(document.nodes(), output)
}

fn render_nodes(nodes: &[DocumentNode], output: &mut impl Write) -> Result {
    for node in nodes {
        render_node(node, output)?;
    }
    Ok(())
}

fn render_node(node: &DocumentNode, output: &mut impl Write) -> Result {
    match node {
        DocumentNode::Paragraph { spans } => {
            writeln!(output, "<p>")?;
            render_spans(spans, output)?;
            writeln!(output, "</p>")?;
            Ok(())
        }
        DocumentNode::Heading { level, spans } => {
            let tag = match level {
                HeadingLevel::Title => "title",
                HeadingLevel::Section => "section-heading",
            };
            write!(output, "<{tag}>")?;
            render_spans(spans, output)?;
            writeln!(output, "</{tag}>")?;
            Ok(())
        }
        DocumentNode::Section { title, nodes } => {
            write!(output, "<section>")?;
            if let Some(title_spans) = title {
                write!(output, "<section-title>")?;
                render_spans(title_spans, output)?;
                write!(output, "</section-title>")?;
            }
            render_nodes(nodes, output)?;
            write!(output, "</section>")?;
            Ok(())
        }
        DocumentNode::List { items } => {
            writeln!(output, "<list>")?;
            for item in items {
                render_list_item(item, output)?;
            }
            writeln!(output, "</list>")?;
            Ok(())
        }
        DocumentNode::CodeBlock { lang, code } => {
            let lang_attr = lang
                .as_ref()
                .map(|l| format!(" lang=\"{}\"", l))
                .unwrap_or_default();
            writeln!(output, "<code-block{}>", lang_attr)?;
            write!(output, "{code}")?;
            if !code.ends_with('\n') {
                writeln!(output)?;
            }
            writeln!(output, "</code-block>")?;
            Ok(())
        }
        DocumentNode::GeneratedCode { spans } => {
            writeln!(output, "<generated-code>")?;
            render_spans(spans, output)?;
            writeln!(output, "</generated-code>")?;
            Ok(())
        }
        DocumentNode::HorizontalRule => {
            writeln!(output, "<hr/>")?;
            Ok(())
        }
        DocumentNode::BlockQuote { nodes } => {
            writeln!(output, "<blockquote>")?;
            render_nodes(nodes, output)?;
            writeln!(output, "</blockquote>")?;
            Ok(())
        }
        DocumentNode::Table { header, rows } => {
            writeln!(output, "<table>")?;
            if let Some(header_cells) = header {
                write!(output, "  <thead>\n    <tr>")?;
                for cell in header_cells {
                    write!(output, "<th>")?;
                    render_spans(&cell.spans, output)?;
                    write!(output, "</th>")?;
                }
                writeln!(output, "</tr>\n  </thead>")?;
            }
            writeln!(output, "  <tbody>")?;
            for row in rows {
                write!(output, "    <tr>")?;
                for cell in row {
                    write!(output, "<td>")?;
                    render_spans(&cell.spans, output)?;
                    write!(output, "</td>")?;
                }
                writeln!(output, "</tr>")?;
            }
            writeln!(output, "  </tbody>\n</table>")?;
            Ok(())
        }
        DocumentNode::TruncatedBlock { nodes, level } => {
            let level_str = match level {
                TruncationLevel::SingleLine => "single-line",
                TruncationLevel::Brief => "brief",
                TruncationLevel::Full => "full",
            };

            // For test mode, just show structure with level attribute
            write!(output, "<truncated level=\"{}\">", level_str)?;

            // Count total content for display
            let total_chars = count_chars_in_nodes(nodes);

            // Truncate based on level
            match level {
                TruncationLevel::SingleLine => {
                    // Show first ~80 chars
                    render_truncated_nodes(nodes, 80, output)?;
                    if total_chars > 80 {
                        write!(output, " <elided chars=\"{}\"/>", total_chars - 80)?;
                    }
                }
                TruncationLevel::Brief => {
                    // Show first ~500 chars
                    render_truncated_nodes(nodes, 500, output)?;
                    if total_chars > 500 {
                        write!(output, " <elided chars=\"{}\"/>", total_chars - 500)?;
                    }
                }
                TruncationLevel::Full => {
                    // Show everything
                    render_nodes(nodes, output)?;
                }
            }

            writeln!(output, "</truncated>")?;
            Ok(())
        }
        DocumentNode::Conditional { show_when, nodes } => {
            let when_str = match show_when {
                ShowWhen::Always => "always",
                ShowWhen::Interactive => "interactive",
                ShowWhen::NonInteractive => "non-interactive",
            };
            write!(output, "<conditional when=\"{}\">", when_str)?;
            render_nodes(nodes, output)?;
            writeln!(output, "</conditional>")?;
            Ok(())
        }
    }
}

fn render_spans(spans: &[Span], output: &mut impl Write) -> Result {
    for span in spans {
        render_span(span, output)?;
    }
    Ok(())
}

fn render_span(span: &Span, output: &mut impl Write) -> Result {
    let tag = match span.style {
        SpanStyle::Keyword => "keyword",
        SpanStyle::TypeName => "type-name",
        SpanStyle::FunctionName => "function-name",
        SpanStyle::FieldName => "field-name",
        SpanStyle::Lifetime => "lifetime",
        SpanStyle::Generic => "generic",
        SpanStyle::Plain => {
            // Plain text has no tag
            write!(output, "{}", &span.text)?;
            return Ok(());
        }
        SpanStyle::Punctuation => "punctuation",
        SpanStyle::Operator => "operator",
        SpanStyle::Comment => "comment",
        SpanStyle::InlineRustCode => "inline-rust-code",
        SpanStyle::InlineCode => "inline-code",
        SpanStyle::Strong => "strong",
        SpanStyle::Emphasis => "emphasis",
        SpanStyle::Strikethrough => "strikethrough",
    };

    write!(output, "<{tag}>{}</{tag}>", span.text)?;
    Ok(())
}

fn render_list_item(item: &ListItem, output: &mut impl Write) -> Result {
    write!(output, "  <item>")?;
    render_nodes(&item.content, output)?;
    writeln!(output, "</item>")?;
    Ok(())
}

/// Count total characters in document nodes (recursively)
fn count_chars_in_nodes(nodes: &[DocumentNode]) -> usize {
    nodes.iter().map(count_chars_in_node).sum()
}

fn count_chars_in_node(node: &DocumentNode) -> usize {
    match node {
        DocumentNode::Paragraph { spans } => spans.iter().map(|s| s.text.len()).sum(),
        DocumentNode::Heading { spans, .. } => spans.iter().map(|s| s.text.len()).sum(),
        DocumentNode::Section { title, nodes } => {
            let title_len = title
                .as_ref()
                .map_or(0, |t| t.iter().map(|s| s.text.len()).sum());
            title_len + count_chars_in_nodes(nodes)
        }
        DocumentNode::List { items } => items
            .iter()
            .map(|item| count_chars_in_nodes(&item.content))
            .sum(),
        DocumentNode::CodeBlock { code, .. } => code.len(),
        DocumentNode::GeneratedCode { spans } => spans.iter().map(|s| s.text.len()).sum(),
        DocumentNode::HorizontalRule => 3, // "---"
        DocumentNode::BlockQuote { nodes } => count_chars_in_nodes(nodes),
        DocumentNode::Table { header, rows } => {
            let header_len = header.as_ref().map_or(0, |h| {
                h.iter()
                    .map(|cell| cell.spans.iter().map(|s| s.text.len()).sum::<usize>())
                    .sum()
            });
            let rows_len: usize = rows
                .iter()
                .map(|row| {
                    row.iter()
                        .map(|cell| cell.spans.iter().map(|s| s.text.len()).sum::<usize>())
                        .sum::<usize>()
                })
                .sum();
            header_len + rows_len
        }
        DocumentNode::TruncatedBlock { nodes, .. } => count_chars_in_nodes(nodes),
        DocumentNode::Conditional { nodes, .. } => count_chars_in_nodes(nodes),
    }
}

/// Render nodes up to a character limit, breaking at word boundaries
fn render_truncated_nodes(
    nodes: &[DocumentNode],
    max_chars: usize,
    output: &mut impl Write,
) -> Result {
    let mut char_count = 0;

    for node in nodes {
        let node_chars = count_chars_in_node(node);
        if char_count + node_chars > max_chars {
            // Would exceed limit, try to render partially
            render_node_partial(node, max_chars - char_count, output)?;
            break;
        }
        render_node(node, output)?;
        char_count += node_chars;
    }

    Ok(())
}

/// Render a node up to a character limit (simplified - just renders whole node or nothing)
fn render_node_partial(
    node: &DocumentNode,
    remaining_chars: usize,
    output: &mut impl Write,
) -> Result {
    if remaining_chars == 0 {
        return Ok(());
    }

    // Simplified: for now, just render whole node if it fits partially
    // A full implementation would truncate within spans at word boundaries
    match node {
        DocumentNode::Paragraph { spans } => {
            for span in spans {
                if span.text.len() <= remaining_chars {
                    render_span(span, output)?;
                } else {
                    // Truncate at word boundary
                    let truncated = truncate_at_word_boundary(&span.text, remaining_chars);
                    let truncated_span = Span {
                        text: truncated.into(),
                        style: span.style,
                        action: None,
                    };
                    render_span(&truncated_span, output)?;
                }
            }
            Ok(())
        }
        // For other node types, just skip if they don't fit
        _ => Ok(()),
    }
}

/// Truncate string at word boundary
fn truncate_at_word_boundary(text: &str, max_chars: usize) -> &str {
    if text.len() <= max_chars {
        return text;
    }

    // Find last whitespace before max_chars
    if let Some(pos) = text[..max_chars].rfind(char::is_whitespace) {
        &text[..pos]
    } else {
        // No whitespace found, just hard cut
        &text[..max_chars]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_paragraph() {
        let doc = Document::from(DocumentNode::paragraph(vec![
            Span::keyword("struct"),
            Span::plain(" "),
            Span::type_name("Foo"),
        ]));

        let mut output = String::new();
        render(&doc, &mut output).unwrap();
        assert!(output.contains("<keyword>struct</keyword>"));
        assert!(output.contains("<type-name>Foo</type-name>"));
    }

    #[test]
    fn test_render_heading() {
        let doc = Document::from(DocumentNode::heading(
            HeadingLevel::Title,
            vec![Span::plain("Item: "), Span::type_name("Vec")],
        ));

        let mut output = String::new();
        render(&doc, &mut output).unwrap();
        assert!(output.contains("<title>"));
        assert!(output.contains("Item: "));
        assert!(output.contains("<type-name>Vec</type-name>"));
        assert!(output.contains("</title>"));
    }

    #[test]
    fn test_render_code_block() {
        let doc = Document::from(DocumentNode::code_block(
            Some("rust".to_string()),
            "fn main() {}".to_string(),
        ));

        let mut output = String::new();
        render(&doc, &mut output).unwrap();
        assert!(output.contains("<code-block lang=\"rust\">"));
        assert!(output.contains("fn main() {}"));
        assert!(output.contains("</code-block>"));
    }
}
