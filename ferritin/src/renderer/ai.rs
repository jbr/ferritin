//! Token-efficient renderer for AI/LLM consumption.
//!
//! This renderer produces compact output optimized for LLM processing, similar to the MCP
//! server output format. It uses comment-style formatting (`Name // description`) to keep
//! tokens to a minimum while maintaining readability and semantic information.

use std::fmt::{Result, Write};

use crate::styled_string::{
    Document, DocumentNode, HeadingLevel, ListItem, ShowWhen, Span, TableCell, TruncationLevel,
};

/// Escape characters that have meaning inside a markdown table cell:
/// `|` ends the cell, `\` escapes the next char, and embedded newlines
/// would break the row across lines.
fn escape_md_cell(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str(r"\\"),
            '|' => out.push_str(r"\|"),
            '\n' => out.push_str("<br>"),
            _ => out.push(ch),
        }
    }
    out
}

/// AI-friendly renderer state
struct AiRenderer<'w, W: Write> {
    output: &'w mut W,
    indent: String,
}

/// Render a document in AI-friendly format
pub fn render(document: &Document, output: &mut impl Write) -> Result {
    let mut renderer = AiRenderer::new(output);
    renderer.render_block_sequence(&document.nodes)
}

impl<'w, W: Write> AiRenderer<'w, W> {
    fn new(output: &'w mut W) -> Self {
        Self {
            output,
            indent: String::new(),
        }
    }

    fn write_indent(&mut self) -> Result {
        write!(self.output, "{}", self.indent)
    }

    /// Render a sequence of block nodes with blank lines between top-level blocks
    fn render_block_sequence(&mut self, nodes: &[DocumentNode]) -> Result {
        for (idx, node) in nodes.iter().enumerate() {
            if idx > 0 {
                writeln!(self.output)?; // Blank line between top-level blocks
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
                writeln!(self.output)?;
                Ok(())
            }
            DocumentNode::Heading { level, spans } => {
                self.write_indent()?;
                self.render_spans(spans)?;
                writeln!(self.output)?;
                // Add underlines for headings (similar to plain renderer but compact)
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
                }
                // Render section items compactly
                for node in nodes {
                    self.render_node(node)?;
                }
                Ok(())
            }
            DocumentNode::List { items } => {
                // Render list items with blank lines between them
                for (idx, item) in items.iter().enumerate() {
                    if idx > 0 {
                        writeln!(self.output)?; // Blank line between items
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
                writeln!(self.output)?;
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
                for node in nodes {
                    self.write_indent()?;
                    write!(self.output, "> ")?;
                    let saved_indent = self.indent.clone();
                    self.indent.push_str("  ");
                    self.render_node(node)?;
                    self.indent = saved_indent;
                }
                Ok(())
            }
            DocumentNode::Table { header, rows } => self.render_table(header.as_deref(), rows),
            DocumentNode::TruncatedBlock { nodes, level } => {
                match level {
                    TruncationLevel::SingleLine => {
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
                                write!(self.output, " [+{} more lines]", nodes.len() - 1)?;
                            }
                        }
                        writeln!(self.output)?;
                    }
                    TruncationLevel::Brief => {
                        if let Some(first_node) = nodes.first() {
                            self.render_node(first_node)?;
                            if nodes.len() > 1 {
                                self.write_indent()?;
                                write!(self.output, "[+{} more lines]", nodes.len() - 1)?;
                                writeln!(self.output)?;
                            }
                        }
                    }
                    TruncationLevel::Full => {
                        self.render_block_sequence(nodes)?;
                    }
                }
                Ok(())
            }
            DocumentNode::Conditional { show_when, nodes } => {
                let should_show = match show_when {
                    ShowWhen::Always => true,
                    ShowWhen::Interactive => false,
                    ShowWhen::NonInteractive => true,
                };

                if should_show {
                    for node in nodes {
                        self.render_node(node)?;
                    }
                }
                Ok(())
            }
        }
    }

    /// Render a table in markdown format (`| col | col |`).
    ///
    /// Markdown tables are familiar to LLMs and far more token-efficient than
    /// box-drawing characters. Pipe and backslash characters in cell content
    /// are escaped so the table parses unambiguously.
    fn render_table(&mut self, header: Option<&[TableCell]>, rows: &[Vec<TableCell>]) -> Result {
        if rows.is_empty() && header.is_none() {
            return Ok(());
        }

        let num_cols = header
            .map(<[_]>::len)
            .or_else(|| rows.first().map(Vec::len))
            .unwrap_or(0);

        if num_cols == 0 {
            return Ok(());
        }

        // Markdown tables require a header row. If the source table omitted
        // one, use the first row as the header so the output stays valid.
        let (header_cells, body_rows): (Vec<&TableCell>, &[Vec<TableCell>]) = match header {
            Some(h) => (h.iter().collect(), rows),
            None => (rows[0].iter().collect(), &rows[1..]),
        };

        self.write_md_row(&header_cells, num_cols)?;
        self.write_indent()?;
        write!(self.output, "|")?;
        for _ in 0..num_cols {
            write!(self.output, " --- |")?;
        }
        writeln!(self.output)?;

        for row in body_rows {
            let cells: Vec<&TableCell> = row.iter().collect();
            self.write_md_row(&cells, num_cols)?;
        }
        Ok(())
    }

    fn write_md_row(&mut self, cells: &[&TableCell], num_cols: usize) -> Result {
        self.write_indent()?;
        write!(self.output, "|")?;
        for col_idx in 0..num_cols {
            write!(self.output, " ")?;
            if let Some(cell) = cells.get(col_idx) {
                for span in &cell.spans {
                    write!(self.output, "{}", escape_md_cell(&span.text))?;
                }
            }
            write!(self.output, " |")?;
        }
        writeln!(self.output)
    }

    fn render_spans(&mut self, spans: &[Span]) -> Result {
        for span in spans {
            self.render_span(span)?;
        }
        Ok(())
    }

    fn render_span(&mut self, Span { text, .. }: &Span) -> Result {
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

        // Render first node (typically the name) to a temporary string to control formatting
        let mut first_node_output = String::new();
        if let Some(first) = item.content.first() {
            let mut temp_renderer = AiRenderer {
                output: &mut first_node_output,
                indent: String::new(),
            };
            temp_renderer.render_node(first)?;
        }

        // Strip trailing whitespace from first node
        let first_text = first_node_output.trim_end();
        write!(self.output, "{}", first_text)?;

        // If there are more nodes, render them as comment-style description
        if item.content.len() > 1 {
            write!(self.output, " // ")?;

            // Render subsequent content inline or as brief description
            for node in item.content.iter().skip(1) {
                match node {
                    DocumentNode::Paragraph { spans } => {
                        self.render_spans(spans)?;
                    }
                    DocumentNode::TruncatedBlock {
                        nodes,
                        level: TruncationLevel::Brief,
                    } => {
                        if let Some(desc) = nodes.first() {
                            match desc {
                                DocumentNode::Paragraph { spans } => {
                                    self.render_spans(spans)?;
                                }
                                _ => {
                                    self.render_node(desc)?;
                                }
                            }
                        }
                        if nodes.len() > 1 {
                            write!(self.output, " [+{} more lines]", nodes.len() - 1)?;
                        }
                    }
                    _ => {
                        self.render_node(node)?;
                    }
                }
            }
        }

        writeln!(self.output)?;
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
            ListItem::new(vec![
                DocumentNode::paragraph(vec![Span::plain("First")]),
                DocumentNode::paragraph(vec![Span::plain("description")]),
            ]),
            ListItem::new(vec![
                DocumentNode::paragraph(vec![Span::plain("Second")]),
                DocumentNode::paragraph(vec![Span::plain("more description")]),
            ]),
        ])]);

        let mut output = String::new();
        render(&doc, &mut output).unwrap();

        // Should have comment-style format
        assert!(output.contains("First // "));
        assert!(output.contains("Second // "));
        // Should not have bullet points
        assert!(!output.contains("◦"));
    }

    #[test]
    fn test_render_table() {
        let doc = Document::with_nodes(vec![DocumentNode::table(
            Some(vec![
                TableCell::from_span(Span::plain("Field")),
                TableCell::from_span(Span::plain("Type")),
            ]),
            vec![vec![
                TableCell::from_span(Span::plain("x")),
                TableCell::from_span(Span::plain("u32")),
            ]],
        )]);

        let mut output = String::new();
        render(&doc, &mut output).unwrap();

        // Cell contents should appear, not just "[Table: ...]"
        assert!(output.contains("Field"));
        assert!(output.contains("u32"));
        assert!(!output.contains("[Table:"));
        // Markdown table syntax
        assert!(output.contains("| Field | Type |"));
        assert!(output.contains("| --- | --- |"));
        assert!(output.contains("| x | u32 |"));
    }

    #[test]
    fn test_render_table_escapes_pipes() {
        let doc = Document::with_nodes(vec![DocumentNode::table(
            Some(vec![TableCell::from_span(Span::plain("Pat"))]),
            vec![vec![TableCell::from_span(Span::plain("a | b"))]],
        )]);

        let mut output = String::new();
        render(&doc, &mut output).unwrap();
        // The pipe in cell content must be escaped so it doesn't end the cell.
        assert!(output.contains(r"a \| b"));
    }
}
