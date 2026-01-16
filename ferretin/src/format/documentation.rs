use super::*;
use crate::styled_string::{DocumentNode, TruncationLevel};
use crate::{generate_docsrs_url::generate_docsrs_url, markdown::MarkdownRenderer};
use rustdoc_core::intra_doc_links::{ResolvedLink, resolve_link};

/// Information about documentation text with truncation details
#[derive(Debug, Clone, Default)]
pub(crate) struct DocInfo {
    /// The truncated documentation text (may be complete if not truncated)
    pub(crate) text: String,
    /// Total number of lines in the original documentation
    pub(crate) total_lines: usize,
    /// Number of lines included in the truncated text
    pub(crate) displayed_lines: usize,
    /// Whether the documentation was truncated
    pub(crate) is_truncated: bool,
}

impl DocInfo {
    /// Get the number of lines that were elided (hidden)
    pub(crate) fn elided_lines(&self) -> usize {
        self.total_lines.saturating_sub(self.displayed_lines)
    }

    /// Format the elided line count for display (e.g., "[+5 lines]")
    pub(crate) fn elided_indicator(&self) -> Option<String> {
        if self.is_truncated {
            Some(format!("[+{} lines elided]", self.elided_lines()))
        } else {
            None
        }
    }
}

impl Request {
    /// Render markdown documentation to structured DocumentNodes
    pub(crate) fn render_docs<'a>(
        &self,
        item: DocRef<'_, Item>,
        markdown: &str,
    ) -> Vec<DocumentNode<'a>> {
        // Create a link resolver that can resolve intra-doc links
        let link_resolver =
            |url: &str| -> Option<String> { self.resolve_intra_doc_link(item, url) };

        MarkdownRenderer::render_with_resolver(markdown, link_resolver)
    }

    /// Resolve an intra-doc link to a docs.rs URL or navigation hint
    fn resolve_intra_doc_link(&self, item: DocRef<'_, Item>, url: &str) -> Option<String> {
        // Use the centralized intra-doc link resolver
        // render_docs() needs to be refactored to accept an origin parameter

        let resolved = resolve_link(self, item, url);

        match resolved {
            ResolvedLink::Item(item) => Some(generate_docsrs_url(item)),
            ResolvedLink::Fragment(_) | ResolvedLink::External(_) | ResolvedLink::Unresolved => {
                // Keep these as-is (return None to use original URL)
                None
            }
        }
    }

    /// Get documentation to show for an item
    ///
    /// Returns None if no docs should be shown, Some(docs) if docs should be displayed.
    /// Docs are wrapped in a TruncatedBlock with appropriate level hint.
    pub(crate) fn docs_to_show<'a>(
        &self,
        item: DocRef<'_, Item>,
        truncation_level: TruncationLevel,
    ) -> Option<Vec<DocumentNode<'a>>> {
        // Extract docs from item
        let docs = item.docs.as_deref()?;
        if docs.is_empty() {
            return None;
        }

        let nodes = self.render_docs(item, docs);
        Some(vec![DocumentNode::truncated_block(nodes, truncation_level)])
    }

    /// Count the number of lines in a text string
    pub(crate) fn count_lines(&self, text: &str) -> usize {
        if text.is_empty() {
            0
        } else {
            text.lines().count()
        }
    }

    /// Truncate text to first paragraph or max_lines, whichever comes first
    pub(crate) fn truncate_to_paragraph_or_lines(&self, text: &str, max_lines: usize) -> String {
        // Look for the second occurrence of "\n\n" (second paragraph break)
        if let Some(first_break) = text.find("\n\n") {
            let after_first_break = &text[first_break + 2..];
            if let Some(second_break_offset) = after_first_break.find("\n\n") {
                // Found second paragraph break - truncate there
                let second_break_pos = first_break + 2 + second_break_offset;
                let first_section = &text[..second_break_pos];
                let first_section_lines = self.count_lines(first_section);

                // If first section is within line limit, use it
                if first_section_lines <= max_lines {
                    return first_section.to_string();
                }
            }
        }

        // Fall back to line-based truncation (no second paragraph break found, or too long)
        let lines: Vec<&str> = text.lines().collect();
        let cutoff = max_lines.min(lines.len());
        lines[..cutoff].join("\n")
    }
}
