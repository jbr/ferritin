//! Token-efficient renderer for coding agents and other LLM consumers.
//!
//! Produces compact, markdown-flavored output optimized for LLM readers.
//! LLMs are heavily trained on markdown, so we lean into markdown conventions
//! (`#` headers, `---` horizontal rules, `-` list bullets) instead of ASCII
//! decorations (80-char underlines, box-drawing).
//!
//! Design goals (in priority order):
//! 1. Preserve all semantic information from the IR.
//! 2. Minimize token count — no decorative ASCII, no redundant blank lines.
//! 3. Use formats LLMs already parse fluently (markdown).
//! 4. Disambiguate nesting cleanly — section depth tracked via header level.

use std::fmt::{Result, Write};

use crate::styled_string::{
    Document, DocumentNode, HeadingLevel, ListItem, MetadataField, ShowWhen, Span, TableCell,
    TruncationLevel,
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

/// State for the AI renderer.
///
/// `section_depth` tracks how deeply nested we are inside `Section` containers
/// so that nested section titles can use deeper markdown header levels (### vs
/// ## etc). `indent` tracks continuation indent for nested list / blockquote
/// content.
struct AiRenderer<'w, W: Write> {
    output: &'w mut W,
    indent: String,
    /// Depth into nested Section containers. Starts at 0 (top level). Each
    /// titled Section bumps the heading hash count by 1, capped at h6.
    section_depth: usize,
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
            section_depth: 0,
        }
    }

    fn write_indent(&mut self) -> Result {
        write!(self.output, "{}", self.indent)
    }

    /// Markdown-style header prefix (#, ##, ###, ...). Caps at h6.
    fn header_hashes(level: usize) -> &'static str {
        match level {
            0 | 1 => "#",
            2 => "##",
            3 => "###",
            4 => "####",
            5 => "#####",
            _ => "######",
        }
    }

    /// Render a sequence of block nodes with a single blank line between them.
    fn render_block_sequence(&mut self, nodes: &[DocumentNode]) -> Result {
        for (idx, node) in nodes.iter().enumerate() {
            if idx > 0 {
                writeln!(self.output)?;
            }
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
            DocumentNode::Metadata { fields } => self.render_metadata(fields),
            DocumentNode::Heading { level, spans } => {
                // Prose headings nest one level below the surrounding
                // structural section. At the document's top level
                // (section_depth = 0) this puts prose `#` at `##` and prose
                // `##` at `###`, so structural sections (`## Methods`) stay
                // visibly above any prose subsection headers. Deeper
                // structural nesting pushes prose headings deeper in turn.
                let level_offset = match level {
                    HeadingLevel::Title => 2,
                    HeadingLevel::Section => 3,
                };
                let hashes = Self::header_hashes(self.section_depth + level_offset);
                self.write_indent()?;
                write!(self.output, "{hashes} ")?;
                self.render_spans(spans)?;
                writeln!(self.output)?;
                Ok(())
            }
            DocumentNode::Section { title, nodes } => {
                // Sections start at h2 (## ) at top level, and increase one
                // level per nesting depth. This gives LLMs the same hierarchy
                // they'd see in a hand-written markdown doc.
                if let Some(title_spans) = title {
                    let hashes = Self::header_hashes(self.section_depth + 2);
                    // Render title to a scratch buffer so we can strip a
                    // trailing colon ("Fields:" → "Fields") — section titles
                    // shouldn't carry punctuation when promoted to headers.
                    let mut buf = String::new();
                    let mut scratch = AiRenderer {
                        output: &mut buf,
                        indent: String::new(),
                        section_depth: self.section_depth,
                    };
                    scratch.render_spans(title_spans)?;
                    let title = buf.trim_end().trim_end_matches(':');
                    self.write_indent()?;
                    writeln!(self.output, "{hashes} {title}")?;
                    writeln!(self.output)?; // blank line after section title
                }
                self.section_depth += 1;
                let result = self.render_block_sequence(nodes);
                self.section_depth -= 1;
                result
            }
            DocumentNode::List { items } => self.render_list(items),
            DocumentNode::CodeBlock { code, lang, .. } => {
                self.write_indent()?;
                match lang.as_deref() {
                    Some(lang) if !lang.is_empty() => writeln!(self.output, "```{lang}")?,
                    _ => writeln!(self.output, "```")?,
                }
                for line in code.lines() {
                    self.write_indent()?;
                    writeln!(self.output, "{line}")?;
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
                writeln!(self.output, "---")?;
                Ok(())
            }
            DocumentNode::BlockQuote { nodes } => {
                // Render each contained block with `> ` prefix per markdown.
                let saved_indent = self.indent.clone();
                for (idx, node) in nodes.iter().enumerate() {
                    if idx > 0 {
                        writeln!(self.output, "{saved_indent}>")?;
                    }
                    self.indent = format!("{saved_indent}> ");
                    self.render_node(node)?;
                }
                self.indent = saved_indent;
                Ok(())
            }
            DocumentNode::Table { header, rows } => self.render_table(header.as_deref(), rows),
            DocumentNode::TruncatedBlock { nodes, level } => self.render_truncated(nodes, *level),
            DocumentNode::Conditional { show_when, nodes } => {
                let should_show = match show_when {
                    ShowWhen::Always => true,
                    ShowWhen::Interactive => false,
                    ShowWhen::NonInteractive => true,
                };
                if should_show {
                    self.render_block_sequence(nodes)?;
                }
                Ok(())
            }
        }
    }

    /// Render a metadata block as a compact one-line summary. The format
    /// puts the most-load-bearing info first:
    ///
    /// `Kind path::to::Item (visibility) — in crate-name version`
    ///
    /// Falls back to one line per field for any fields we don't recognize so
    /// future format-layer additions still surface, just less densely.
    fn render_metadata(&mut self, fields: &[MetadataField]) -> Result {
        let mut kind: Option<&str> = None;
        let mut path_field: Option<&MetadataField> = None;
        let mut visibility: Option<&MetadataField> = None;
        let mut crate_field: Option<&MetadataField> = None;
        let mut name_field: Option<&MetadataField> = None;
        let mut other_fields: Vec<&MetadataField> = vec![];

        for field in fields {
            match &*field.label {
                "Kind" => kind = field.value.first().map(|s| s.text.as_ref()),
                "Item" => name_field = Some(field),
                "Defined at" => path_field = Some(field),
                "Visibility" => visibility = Some(field),
                "In crate" => crate_field = Some(field),
                _ => other_fields.push(field),
            }
        }

        self.write_indent()?;

        // Kind prefix (e.g. "Struct ").
        if let Some(kind) = kind {
            write!(self.output, "{kind} ")?;
        }

        // Path, or fall back to bare name if no path was available.
        if let Some(path) = path_field {
            self.render_spans(&path.value)?;
        } else if let Some(name) = name_field {
            self.render_spans(&name.value)?;
        }

        // Visibility parenthetical. Skip the trivial "Public" since that's
        // the implicit default for documented items.
        if let Some(vis) = visibility {
            let vis_text: String = vis.value.iter().map(|s| s.text.as_ref()).collect();
            let vis_text = vis_text.trim();
            if !vis_text.is_empty() && !vis_text.eq_ignore_ascii_case("Public") {
                write!(self.output, " ({vis_text})")?;
            }
        }

        // Crate suffix.
        if let Some(c) = crate_field {
            write!(self.output, " — in ")?;
            self.render_spans(&c.value)?;
        }

        writeln!(self.output)?;

        // Any unknown fields, one per line, in `Label: value` form.
        for field in other_fields {
            self.write_indent()?;
            write!(self.output, "{}: ", field.label)?;
            self.render_spans(&field.value)?;
            writeln!(self.output)?;
        }

        Ok(())
    }

    /// Render a truncated block. Level controls how aggressively we collapse.
    fn render_truncated(&mut self, nodes: &[DocumentNode], level: TruncationLevel) -> Result {
        match level {
            TruncationLevel::SingleLine => {
                if let Some(first_node) = nodes.first() {
                    match first_node {
                        DocumentNode::Paragraph { spans } | DocumentNode::Heading { spans, .. } => {
                            self.write_indent()?;
                            self.render_spans(spans)?;
                        }
                        _ => self.render_node(first_node)?,
                    }
                    if nodes.len() > 1 {
                        write!(self.output, " [+{} lines]", nodes.len() - 1)?;
                    }
                }
                writeln!(self.output)?;
                Ok(())
            }
            TruncationLevel::Brief => {
                if let Some(first_node) = nodes.first() {
                    self.render_node(first_node)?;
                    if nodes.len() > 1 {
                        self.write_indent()?;
                        writeln!(self.output, "[+{} lines]", nodes.len() - 1)?;
                    }
                }
                Ok(())
            }
            TruncationLevel::Full => self.render_block_sequence(nodes),
        }
    }

    /// Render a list of items.
    ///
    /// Format choice is per-list, not per-item, because user testing flagged
    /// that mixed one-line and two-line entries in the same list broke
    /// reading rhythm. Inside a single list, all items share one shape.
    ///
    /// Three shapes are possible:
    /// - Compact one-line: `- name — description` (when every item fits).
    /// - Two-line: `- signature\n  description` (when any item has a
    ///   long-enough first node that the em-dash form would get lost in the
    ///   trailing `<…>` and `+` of a where-clause).
    /// - Block: full multi-paragraph items separated by blank lines (when
    ///   any item has more than two content nodes).
    fn render_list(&mut self, items: &[ListItem]) -> Result {
        let block_style = items.iter().any(|item| !self.item_is_single_line(item));
        let force_two_lines =
            !block_style && items.iter().any(|item| self.item_needs_two_lines(item));

        for (idx, item) in items.iter().enumerate() {
            if idx > 0 && block_style {
                writeln!(self.output)?;
            }
            self.render_list_item(item, force_two_lines)?;
        }
        Ok(())
    }

    /// True when an item's `name — description` form would push past the
    /// threshold or contain a signature too noisy for an inline em-dash to
    /// stay legible. Used to promote a whole list to two-line form.
    fn item_needs_two_lines(&self, item: &ListItem) -> bool {
        const LONG_FIRST_NODE_THRESHOLD: usize = 60;
        let [first, second] = item.content.as_slice() else {
            return false;
        };
        if !self.is_single_line_description(second) {
            return false;
        }
        // Force 2-line whenever the first node carries Rust syntax (signature,
        // impl head, etc.) — these grow unpredictably and look uniform when
        // all rendered the same shape.
        if matches!(first, DocumentNode::GeneratedCode { .. }) {
            // Measure the first node's rendered width.
            let mut buf = String::new();
            let mut scratch = AiRenderer {
                output: &mut buf,
                indent: String::new(),
                section_depth: self.section_depth,
            };
            // Best-effort; on write failure we conservatively assume long.
            if scratch.render_inline_node(first).is_err() {
                return true;
            }
            return buf.trim_end().len() > LONG_FIRST_NODE_THRESHOLD;
        }
        false
    }

    /// True if a list item can be rendered on a single output line
    /// (either as `- name` or `- name — description`).
    fn item_is_single_line(&self, item: &ListItem) -> bool {
        match item.content.as_slice() {
            [] => true,
            [_only] => true,
            [_first, second] => self.is_single_line_description(second),
            _ => false,
        }
    }

    /// True if a node can render as a one-line description suitable for
    /// `- name — desc` collapse. Accepts paragraphs, headings, and
    /// single-line truncated blocks (regardless of inner node kind) since
    /// the renderer can extract a single line of text from any of these.
    fn is_single_line_description(&self, node: &DocumentNode) -> bool {
        match node {
            DocumentNode::Paragraph { spans } | DocumentNode::Heading { spans, .. } => {
                !spans.iter().any(|s| s.text.contains('\n'))
            }
            DocumentNode::GeneratedCode { spans } => !spans.iter().any(|s| s.text.contains('\n')),
            DocumentNode::TruncatedBlock {
                level: TruncationLevel::SingleLine,
                ..
            } => true,
            _ => false,
        }
    }

    fn render_list_item(&mut self, item: &ListItem, force_two_lines: bool) -> Result {
        self.write_indent()?;
        write!(self.output, "- ")?;

        match item.content.as_slice() {
            [] => {
                writeln!(self.output)?;
                Ok(())
            }
            [only] => {
                // Render to scratch buffer so trailing whitespace from the
                // formatter (e.g. a trailing Span::plain(" ")) doesn't bleed
                // into the line.
                let mut buf = String::new();
                let mut scratch = AiRenderer {
                    output: &mut buf,
                    indent: String::new(),
                    section_depth: self.section_depth,
                };
                scratch.render_inline_node(only)?;
                writeln!(self.output, "{}", buf.trim_end())?;
                Ok(())
            }
            [first, second] if self.is_single_line_description(second) => {
                let mut name_buf = String::new();
                let mut scratch = AiRenderer {
                    output: &mut name_buf,
                    indent: String::new(),
                    section_depth: self.section_depth,
                };
                scratch.render_inline_node(first)?;
                let name = name_buf.trim_end();

                let mut desc_buf = String::new();
                let mut scratch = AiRenderer {
                    output: &mut desc_buf,
                    indent: String::new(),
                    section_depth: self.section_depth,
                };
                scratch.render_description_inline(second)?;
                let desc = desc_buf.trim();

                if desc.is_empty() {
                    writeln!(self.output, "{name}")?;
                } else if force_two_lines {
                    writeln!(self.output, "{name}")?;
                    self.write_indent()?;
                    writeln!(self.output, "  {desc}")?;
                } else {
                    writeln!(self.output, "{name} — {desc}")?;
                }
                Ok(())
            }
            _ => {
                // Render first node inline with bullet, then subsequent nodes
                // on following lines indented by 2 spaces.
                let (first, rest) = item.content.split_first().unwrap();
                self.render_inline_node_then_newline(first)?;
                let saved_indent = self.indent.clone();
                self.indent.push_str("  ");
                for node in rest {
                    self.render_node(node)?;
                }
                self.indent = saved_indent;
                Ok(())
            }
        }
    }

    /// Render a node inline (no leading indent, no trailing newline) — used
    /// for the part of a list item that follows `- `.
    fn render_inline_node(&mut self, node: &DocumentNode) -> Result {
        match node {
            DocumentNode::Paragraph { spans } | DocumentNode::GeneratedCode { spans } => {
                self.render_spans_no_leading_indent(spans)
            }
            DocumentNode::Heading { level, spans } => {
                let hashes = match level {
                    HeadingLevel::Title => "#",
                    HeadingLevel::Section => "##",
                };
                write!(self.output, "{hashes} ")?;
                self.render_spans_no_leading_indent(spans)
            }
            // For container nodes we fall back to rendering on a new line
            // (with indent). Caller ensures the indent is set correctly.
            _ => self.render_node(node),
        }
    }

    fn render_inline_node_then_newline(&mut self, node: &DocumentNode) -> Result {
        self.render_inline_node(node)?;
        writeln!(self.output)?;
        Ok(())
    }

    /// Render a description node inline (extracting text content for compact
    /// `- name — desc` form). Headings are stripped of their `#` prefix
    /// since the description is being inlined as glossary text, not as a
    /// header.
    fn render_description_inline(&mut self, node: &DocumentNode) -> Result {
        match node {
            DocumentNode::Paragraph { spans }
            | DocumentNode::Heading { spans, .. }
            | DocumentNode::GeneratedCode { spans } => self.render_spans_no_leading_indent(spans),
            DocumentNode::TruncatedBlock {
                nodes,
                level: TruncationLevel::SingleLine,
            } => {
                if let Some(first) = nodes.first() {
                    self.render_description_inline(first)?;
                }
                if nodes.len() > 1 {
                    write!(self.output, " [+{} lines]", nodes.len() - 1)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Render a table in markdown format (`| col | col |`).
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
        // Renders spans into the current line. Embedded newlines in span text
        // re-trigger the current indent so blocks stay aligned.
        for span in spans {
            self.render_span(span)?;
        }
        Ok(())
    }

    /// Render spans without writing the leading indent — caller already
    /// positioned the cursor (e.g. just after `- `).
    fn render_spans_no_leading_indent(&mut self, spans: &[Span]) -> Result {
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
        assert!(output.contains("# Item: Vec"));
        // No 80-char underlines
        assert!(!output.contains("===="));
    }

    #[test]
    fn test_render_section_uses_md_header() {
        let doc = Document::with_nodes(vec![DocumentNode::section(
            vec![Span::plain("Fields")],
            vec![DocumentNode::paragraph(vec![Span::plain("contents")])],
        )]);
        let mut output = String::new();
        render(&doc, &mut output).unwrap();
        assert!(output.contains("## Fields"));
    }

    #[test]
    fn test_render_list_compact() {
        let doc = Document::with_nodes(vec![DocumentNode::list(vec![
            ListItem::new(vec![DocumentNode::paragraph(vec![Span::plain("First")])]),
            ListItem::new(vec![DocumentNode::paragraph(vec![Span::plain("Second")])]),
        ])]);

        let mut output = String::new();
        render(&doc, &mut output).unwrap();
        assert!(output.contains("- First"));
        assert!(output.contains("- Second"));
        // Single-node items should be compact: no blank line between them
        assert!(!output.contains("- First\n\n- Second"));
    }

    #[test]
    fn test_render_list_with_descriptions() {
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

        // Compact "name  description" format on a single line
        assert!(output.contains("- First"));
        assert!(output.contains("description"));
        assert!(output.contains("- Second"));
        assert!(output.contains("more description"));
    }

    #[test]
    fn test_render_horizontal_rule() {
        let doc = Document::with_nodes(vec![DocumentNode::horizontal_rule()]);
        let mut output = String::new();
        render(&doc, &mut output).unwrap();
        assert_eq!(output, "---\n");
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
        assert!(output.contains(r"a \| b"));
    }
}
