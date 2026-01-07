use super::*;

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
    /// Get documentation to show for an item, handling verbosity and truncation
    ///
    /// Returns None if no docs should be shown, Some(docs) if docs should be displayed.
    /// The `is_listing` parameter affects truncation behavior - listing items get more
    /// aggressive truncation than primary items.
    pub(crate) fn docs_to_show(
        &self,
        item: DocRef<'_, Item>,
        is_listing: bool,
        context: &FormatContext,
    ) -> Option<String> {
        // Extract docs from item
        let docs = item.docs.as_deref()?;
        if docs.is_empty() {
            return None;
        }

        // Apply truncation based on verbosity and context
        match (context.verbosity(), is_listing) {
            (Verbosity::Minimal, _) => None,
            (_, true) => {
                // For listings, even in Full mode, just show first non-empty line with indicator
                let first_line = docs
                    .lines()
                    .find(|line| !line.trim().is_empty())
                    .map(|line| line.trim().to_string())?;

                let total_lines = self.count_lines(docs);
                if total_lines > 1 {
                    Some(format!("{} [+{} more lines]", first_line, total_lines - 1))
                } else {
                    Some(first_line)
                }
            }
            (Verbosity::Full, _) => Some(docs.to_string()),
            (Verbosity::Brief, _) => {
                // For primary items, use paragraph-aware truncation
                let total_lines = self.count_lines(docs);
                let truncated_text = self.truncate_to_paragraph_or_lines(docs, 16);
                let displayed_lines = self.count_lines(&truncated_text);
                let is_truncated = displayed_lines < total_lines;

                let doc_info = DocInfo {
                    text: truncated_text,
                    total_lines,
                    displayed_lines,
                    is_truncated,
                };

                if doc_info.is_truncated {
                    Some(format!(
                        "{}\n{}",
                        doc_info.text,
                        doc_info.elided_indicator().unwrap_or_default()
                    ))
                } else {
                    Some(doc_info.text)
                }
            }
        }
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
