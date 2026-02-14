//! Plain text renderer for non-TTY output (e.g., piping).
//!
//! This renderer produces readable plain text output without any terminal control codes
//! or styling. It's used when stdout is not a TTY, making it safe for piping to files
//! or other programs.
//!
//! The output is markdown-like but not necessarily compliant markdown - it prioritizes
//! readability over strict formatting rules.
//!
//! # Layout Model
//!
//! Follows the same principles as the interactive renderer:
//! - Blocks add newlines at the end
//! - Containers add blank lines between consecutive children
//! - List items are compact (no blank lines within an item)
//! - Maintains indentation for nested content

use std::fmt::{Result, Write};

use crate::document::{
    Document, DocumentNode, HeadingLevel, ListItem, ShowWhen, Span, TruncationLevel,
};

/// Plain text renderer state
struct PlainRenderer<'w, W: Write> {
    output: &'w mut W,
    indent: String,
}

/// Render a document as plain text without any styling
pub fn render(document: &Document, output: &mut impl Write) -> Result {
    let mut renderer = PlainRenderer::new(output);
    renderer.render_block_sequence(document.nodes())
}

impl<'w, W: Write> PlainRenderer<'w, W> {
    fn new(output: &'w mut W) -> Self {
        Self {
            output,
            indent: String::new(),
        }
    }

    fn write_indent(&mut self) -> Result {
        write!(self.output, "{}", self.indent)
    }

    /// Render a sequence of block nodes with blank lines between them
    fn render_block_sequence(&mut self, nodes: &[DocumentNode]) -> Result {
        for (idx, node) in nodes.iter().enumerate() {
            if idx > 0 {
                writeln!(self.output)?; // Blank line between consecutive blocks
            }
            self.render_node(node)?;
        }
        Ok(())
    }

    fn render_nodes(&mut self, nodes: &[DocumentNode]) -> Result {
        for node in nodes {
            self.render_node(node)?;
        }
        Ok(())
    }

    fn render_node(&mut self, node: &DocumentNode) -> Result {
        match node {
            DocumentNode::Paragraph { spans } => {
                self.write_indent()?;
                self.render_spans(spans)?;
                writeln!(self.output)?; // Single newline
                Ok(())
            }
            DocumentNode::Heading { level, spans } => {
                self.write_indent()?;
                self.render_spans(spans)?;
                writeln!(self.output)?;
                // Add underlines for headings
                self.write_indent()?;
                match level {
                    HeadingLevel::Title => {
                        for _ in 0..80 {
                            write!(self.output, "=")?;
                        }
                        writeln!(self.output)?;
                    }
                    HeadingLevel::Section => {
                        for _ in 0..80 {
                            write!(self.output, "-")?;
                        }
                        writeln!(self.output)?;
                    }
                }
                Ok(())
            }
            DocumentNode::Section { title, nodes } => {
                if let Some(title_spans) = title {
                    self.write_indent()?;
                    self.render_spans(title_spans)?;
                    writeln!(self.output)?;
                    writeln!(self.output)?; // Blank line after section title
                }
                self.render_block_sequence(nodes)
            }
            DocumentNode::List { items } => {
                for (idx, item) in items.iter().enumerate() {
                    if idx > 0 {
                        writeln!(self.output)?; // Blank line between list items
                    }
                    self.render_list_item(item)?;
                }
                Ok(())
            }
            DocumentNode::CodeBlock { code, .. } => {
                self.write_indent()?;
                writeln!(self.output, "```")?;
                for line in code.lines() {
                    self.write_indent()?;
                    writeln!(self.output, "{line}")?;
                }
                if !code.ends_with('\n') && !code.is_empty() {
                    writeln!(self.output)?;
                }
                self.write_indent()?;
                writeln!(self.output, "```")?;
                Ok(())
            }
            DocumentNode::GeneratedCode { spans } => {
                self.write_indent()?;
                self.render_spans(spans)?;
                writeln!(self.output)?; // Single newline
                Ok(())
            }
            DocumentNode::HorizontalRule => {
                self.write_indent()?;
                for _ in 0..80 {
                    write!(self.output, "─")?;
                }
                writeln!(self.output)?;
                Ok(())
            }
            DocumentNode::BlockQuote { nodes } => {
                for (idx, node) in nodes.iter().enumerate() {
                    if idx > 0 {
                        writeln!(self.output)?; // Blank line between blocks in quote
                    }
                    self.write_indent()?;
                    write!(self.output, "> ")?;
                    // Add indentation for quote content
                    let saved_indent = self.indent.clone();
                    self.indent.push_str("  ");
                    self.render_node(node)?;
                    self.indent = saved_indent;
                }
                Ok(())
            }
            DocumentNode::Table { header, rows } => {
                // Placeholder for table rendering
                let row_count = rows.len();
                let col_count = header
                    .as_ref()
                    .map_or_else(|| rows.first().map_or(0, |r| r.len()), |h| h.len());
                self.write_indent()?;
                writeln!(
                    self.output,
                    "[Table: {} columns × {} rows]",
                    col_count, row_count
                )?;
                Ok(())
            }
            DocumentNode::TruncatedBlock { nodes, level } => {
                // Transparent container - just controls truncation
                match level {
                    TruncationLevel::SingleLine => {
                        // Render first node/paragraph inline
                        if let Some(first_node) = nodes.first() {
                            match first_node {
                                DocumentNode::Paragraph { spans } => {
                                    self.write_indent()?;
                                    self.render_spans(spans)?;
                                }
                                DocumentNode::Heading { spans, .. } => {
                                    self.write_indent()?;
                                    self.render_spans(spans)?;
                                }
                                _ => {
                                    self.render_node(first_node)?;
                                }
                            }
                            if nodes.len() > 1 {
                                write!(self.output, " [...]")?;
                            }
                        }
                        writeln!(self.output)?; // End the line
                    }
                    TruncationLevel::Brief => {
                        // Render first paragraph
                        if let Some(first_node) = nodes.first() {
                            self.render_node(first_node)?;
                            if nodes.len() > 1 {
                                self.write_indent()?;
                                write!(self.output, "[+{} more]", nodes.len() - 1)?;
                                writeln!(self.output)?;
                            }
                        }
                    }
                    TruncationLevel::Full => {
                        // Render everything with spacing
                        self.render_block_sequence(nodes)?;
                    }
                }
                Ok(())
            }
            DocumentNode::Conditional { show_when, nodes } => {
                // Transparent container
                let should_show = match show_when {
                    ShowWhen::Always => true,
                    ShowWhen::Interactive => false,
                    ShowWhen::NonInteractive => true,
                };

                if should_show {
                    for (idx, node) in nodes.iter().enumerate() {
                        if idx > 0 {
                            writeln!(self.output)?; // Blank line between blocks
                        }
                        self.render_node(node)?;
                    }
                }
                Ok(())
            }
        }
    }

    fn render_spans(&mut self, spans: &[Span]) -> Result {
        for span in spans {
            self.render_span(span)?;
        }
        Ok(())
    }

    fn render_span(&mut self, Span { text, .. }: &Span) -> Result {
        // Handle newlines in span text to maintain indentation
        for (idx, line) in text.split('\n').enumerate() {
            if idx > 0 {
                writeln!(self.output)?;
                self.write_indent()?;
            }
            write!(self.output, "{line}")?;
        }
        Ok(())
    }

    fn render_list_item(&mut self, item: &ListItem) -> Result {
        self.write_indent()?;
        let bullet = crate::renderer::bullet_for_indent(self.indent.len() as u16);
        write!(self.output, "  {} ", bullet)?;

        let saved_indent = self.indent.clone();

        // Render first node inline with bullet (before changing indent)
        if let Some(first) = item.content.first() {
            self.render_node(first)?;
        }

        // Add indentation for subsequent nodes
        self.indent.push_str("    "); // 4 spaces to align with content after bullet

        // Render remaining nodes with indentation
        for node in item.content.iter().skip(1) {
            self.render_node(node)?;
        }

        // Restore indent
        self.indent = saved_indent;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_heading() {
        let doc = Document::from(DocumentNode::heading(
            HeadingLevel::Title,
            vec![Span::plain("Item: "), Span::type_name("Vec")],
        ));
        let mut output = String::new();
        render(&doc, &mut output).unwrap();
        assert!(output.contains("Item: Vec"));
        assert!(output.contains("===="));
    }

    #[test]
    fn test_render_list() {
        let doc = Document::from(DocumentNode::list(vec![
            ListItem::new(vec![DocumentNode::paragraph(vec![Span::plain("First")])]),
            ListItem::new(vec![DocumentNode::paragraph(vec![Span::plain("Second")])]),
        ]));

        let mut output = String::new();
        render(&doc, &mut output).unwrap();
        dbg!(&output);

        assert!(output.contains("  ◦ First"));
        assert!(output.contains("  ◦ Second"));
    }
}
