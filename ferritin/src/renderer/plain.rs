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

use crate::styled_string::{
    Document, DocumentNode, HeadingLevel, ListItem, ShowWhen, Span, TableCell, TruncationLevel,
};

use super::table_layout::{self, DEFAULT_FALLBACK_WIDTH, span_display_width};

/// Plain text renderer state
struct PlainRenderer<'w, W: Write> {
    output: &'w mut W,
    indent: String,
}

/// Render a document as plain text without any styling
pub fn render(document: &Document, output: &mut impl Write) -> Result {
    let mut renderer = PlainRenderer::new(output);
    renderer.render_block_sequence(&document.nodes)
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
            DocumentNode::Metadata { fields } => {
                // Render as labeled lines, matching the long-standing
                // appearance ("Item: foo\nKind: Struct\n..."). The label is
                // bold-style historically but plain text here.
                for field in fields {
                    self.write_indent()?;
                    write!(self.output, "{}: ", field.label)?;
                    self.render_spans(&field.value)?;
                    writeln!(self.output)?;
                }
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
            DocumentNode::Table { header, rows } => self.render_table(header.as_deref(), rows),
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

    /// Render a table with Unicode box-drawing borders. Long cells wrap onto
    /// multiple lines and column widths shrink to fit within
    /// [`DEFAULT_FALLBACK_WIDTH`] when the natural layout would overflow.
    fn render_table(&mut self, header: Option<&[TableCell]>, rows: &[Vec<TableCell>]) -> Result {
        if rows.is_empty() && header.is_none() {
            return Ok(());
        }

        let available = DEFAULT_FALLBACK_WIDTH.saturating_sub(self.indent.len());
        let layout = table_layout::lay_out(header, rows, available);
        if layout.num_cols() == 0 {
            return Ok(());
        }

        self.write_horizontal_border(&layout.col_widths, '┌', '┬', '┐')?;
        if let Some(header_cells) = layout.header.as_deref() {
            self.write_table_row(header_cells, &layout.col_widths)?;
            self.write_horizontal_border(&layout.col_widths, '├', '┼', '┤')?;
        }
        let separate_rows = layout.any_wrapped();
        let last = layout.rows.len().saturating_sub(1);
        for (idx, row) in layout.rows.iter().enumerate() {
            self.write_table_row(row, &layout.col_widths)?;
            if separate_rows && idx < last {
                self.write_blank_row(&layout.col_widths)?;
            }
        }
        self.write_horizontal_border(&layout.col_widths, '└', '┴', '┘')?;
        Ok(())
    }

    /// Empty content row preserving the column borders. Used between body
    /// rows to give wrapped cells visual breathing room.
    fn write_blank_row(&mut self, col_widths: &[usize]) -> Result {
        self.write_indent()?;
        write!(self.output, "│")?;
        for &w in col_widths {
            for _ in 0..w {
                write!(self.output, " ")?;
            }
            write!(self.output, "│")?;
        }
        writeln!(self.output)
    }

    fn write_horizontal_border(
        &mut self,
        col_widths: &[usize],
        left: char,
        mid: char,
        right: char,
    ) -> Result {
        self.write_indent()?;
        write!(self.output, "{left}")?;
        for (idx, &width) in col_widths.iter().enumerate() {
            for _ in 0..width {
                write!(self.output, "─")?;
            }
            let sep = if idx == col_widths.len() - 1 {
                right
            } else {
                mid
            };
            write!(self.output, "{sep}")?;
        }
        writeln!(self.output)
    }

    /// Write one logical row, expanding to as many physical lines as the
    /// tallest cell after wrapping.
    fn write_table_row(
        &mut self,
        cells: &[table_layout::LaidOutCell<'_>],
        col_widths: &[usize],
    ) -> Result {
        let row_height = cells
            .iter()
            .map(|c| c.lines.len().max(1))
            .max()
            .unwrap_or(1);
        for line_idx in 0..row_height {
            self.write_indent()?;
            write!(self.output, "│")?;
            for (col_idx, &width) in col_widths.iter().enumerate() {
                let cell = cells.get(col_idx);
                let line: &[Span] = cell
                    .and_then(|c| c.lines.get(line_idx).map(Vec::as_slice))
                    .unwrap_or(&[]);
                let mut written = 0usize;
                for span in line {
                    write!(self.output, "{}", span.text)?;
                    written += span_display_width(span);
                }
                for _ in written..width {
                    write!(self.output, " ")?;
                }
                write!(self.output, "│")?;
            }
            writeln!(self.output)?;
        }
        Ok(())
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
            ListItem::new(vec![DocumentNode::paragraph(vec![Span::plain("First")])]),
            ListItem::new(vec![DocumentNode::paragraph(vec![Span::plain("Second")])]),
        ])]);

        let mut output = String::new();
        render(&doc, &mut output).unwrap();
        dbg!(&output);

        assert!(output.contains("  ◦ First"));
        assert!(output.contains("  ◦ Second"));
    }

    #[test]
    fn test_render_table() {
        let doc = Document::with_nodes(vec![DocumentNode::table(
            Some(vec![
                TableCell::from_span(Span::plain("Field")),
                TableCell::from_span(Span::plain("Type")),
            ]),
            vec![
                vec![
                    TableCell::from_span(Span::plain("x")),
                    TableCell::from_span(Span::plain("u32")),
                ],
                vec![
                    TableCell::from_span(Span::plain("y")),
                    TableCell::from_span(Span::plain("u32")),
                ],
            ],
        )]);

        let mut output = String::new();
        render(&doc, &mut output).unwrap();

        // Cell contents should appear, not just "[Table: ...]"
        assert!(output.contains("Field"));
        assert!(output.contains("Type"));
        assert!(output.contains("u32"));
        assert!(!output.contains("[Table:"));
        // Box-drawing borders
        assert!(output.contains("┌"));
        assert!(output.contains("┼"));
        assert!(output.contains("└"));
    }
}
