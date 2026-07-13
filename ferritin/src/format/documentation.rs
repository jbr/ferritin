use std::borrow::Cow;

use super::*;
use crate::docsrs_url::{DocsRsLink, crate_base_url, generate_docsrs_url, resolve_relative};
use crate::markdown::MarkdownRenderer;
use crate::styled_string::{DocumentNode, LinkTarget, TruncationLevel};

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

impl<'a> Request<'a> {
    /// Render markdown documentation to structured DocumentNodes
    pub(crate) fn render_docs(
        &self,
        item: DocRef<'a, Item>,
        markdown: &str,
    ) -> Vec<DocumentNode<'a>> {
        MarkdownRenderer::render_with_resolver(markdown, |url| -> Option<LinkTarget<'a>> {
            self.extract_link_target(item, url)
        })
    }

    /// Extract the link target from an intra-doc link without loading external crates
    ///
    /// Returns either a resolved DocRef (for same-crate items) or an unresolved path string
    /// (for external items), avoiding the need to load external crates just for rendering.
    /// URL generation is deferred to the renderer that needs it.
    fn extract_link_target(&self, origin: DocRef<'a, Item>, url: &str) -> Option<LinkTarget<'a>> {
        // Handle fragment-only links
        if url.starts_with('#') {
            return None; // Keep as-is
        }

        // Handle external URLs. A rendered-documentation URL (docs.rs,
        // doc.rust-lang.org) names an item we can resolve, so it navigates in-app
        // rather than opening a browser — plenty of crates write these out by hand
        // instead of using an intra-doc link. We keep the original URL rather than
        // regenerating one, since it is authoritative about version and anchor.
        if url.starts_with("http://") || url.starts_with("https://") {
            return DocsRsLink::parse(url).map(|link| LinkTarget::Path {
                path: Cow::Owned(link.to_string()),
                url: Some(Cow::Owned(url.to_string())),
            });
        }

        // Split off any fragment/anchor
        let (path, _fragment) = url.split_once('#').unwrap_or((url, ""));

        // Check if this is a relative HTML URL (e.g., "task/index.html", "../attr.main.html")
        // These are hand-written markdown links in the source that point to HTML docs
        if path.ends_with(".html") || path.contains("/") {
            log::trace!("extract_link_target: parsing relative URL '{}'", url);
            return self.resolve_relative_html(origin, url);
        }

        log::trace!("extract_link_target: processing link '{}'", path);

        // Try to get the path from rustdoc's pre-resolved links map
        // Rustdoc sometimes stores links with backticks, sometimes without
        // Try both formats
        let link_id = origin
            .links
            .get(path)
            .or_else(|| origin.links.get(&format!("`{}`", path)));

        if let Some(link_id) = link_id {
            log::trace!("  ✓ Found in origin.links with ID {:?}", link_id);
            // Check if it's in the same crate (fast path - no external loading)
            if let Some(item) = origin.get(link_id) {
                log::trace!(
                    "  → Same-crate item: path='{}', kind={:?}",
                    self.get_item_full_path(item),
                    item.kind()
                );
                return Some(LinkTarget::Resolved(item));
            }

            log::trace!("  → Not in same crate index, checking external paths");
            // It's in an external crate - extract path from item_summary without loading
            if let Some(item_summary) = origin.crate_docs().path_summary(link_id) {
                log::trace!(
                    "  ✓ Found in paths map: {:?}, kind: {:?}",
                    item_summary.path,
                    item_summary.kind
                );
                let full_path = item_summary.path.join("::");
                return Some(LinkTarget::Path {
                    path: Cow::Owned(full_path),
                    url: None,
                });
            }
        }

        // Fallback: try to resolve path relative to current crate
        // Handle "crate::", "self::", and absolute paths
        log::trace!("  ✗ Not found in links map, using fallback for '{}'", path);
        let qualified_path = if let Some(rest) = path.strip_prefix("crate::") {
            format!("{}::{}", origin.crate_docs().name(), rest)
        } else if let Some(rest) = path.strip_prefix("self::") {
            format!("{}::{}", origin.crate_docs().name(), rest)
        } else if path.contains("::") {
            path.to_string()
        } else {
            format!("{}::{}", origin.crate_docs().name(), path)
        };

        log::trace!("  → Qualified path: '{}'", qualified_path);
        Some(LinkTarget::Path {
            path: Cow::Owned(qualified_path),
            url: None,
        })
    }

    /// Resolve a hand-written relative link into rustdoc's HTML output —
    /// `../attr.main.html`, `task/index.html`, `index.html#anchor` — against the page
    /// `origin` is itself rendered on.
    ///
    /// Such links are relative to the *directory* holding the origin's page, and
    /// [`generate_docsrs_url`] is exactly what names that page: `…/tokio/runtime/index.html`
    /// for a module, `…/tokio/runtime/struct.Runtime.html#method.block_on` for one of its
    /// methods. Both sit in `…/tokio/runtime/`. Joining the link onto that directory
    /// gives an absolute URL, which [`DocsRsLink::parse`] already knows how to read —
    /// sigils, `index.html`, and item-naming fragments included.
    ///
    /// A relative link can only address the origin's own crate, so one that walks out of
    /// its documentation tree is broken and left alone. Being same-crate, the resulting
    /// path needs no version qualifier, exactly like the intra-doc links below.
    fn resolve_relative_html(
        &self,
        origin: DocRef<'a, Item>,
        relative: &str,
    ) -> Option<LinkTarget<'a>> {
        let (page_part, _) = relative.split_once('#').unwrap_or((relative, ""));
        if !page_part.ends_with(".html") {
            return None;
        }

        let absolute = resolve_relative(&generate_docsrs_url(origin), relative)?;

        // Confine the link to this crate's own tree: `..` can walk up into a sibling
        // crate's directory, where the path we'd derive would name the wrong crate.
        let docs = origin.crate_docs();
        let lib_name = docs.lib_name();
        let within_crate = absolute
            .strip_prefix(&crate_base_url(docs))
            .and_then(|rest| rest.strip_prefix('/'))
            .is_some_and(|rest| {
                rest == lib_name
                    || rest
                        .strip_prefix(lib_name)
                        .is_some_and(|r| r.starts_with('/'))
            });
        if !within_crate {
            log::trace!("  → relative link '{relative}' escapes {lib_name}, keeping as-is");
            return None;
        }

        let path = {
            let mut link = DocsRsLink::parse(&absolute)?;
            link.version = None;
            link.to_string()
        };

        log::trace!("  → Resolved '{relative}' to '{path}' ({absolute})");
        Some(LinkTarget::Path {
            path: Cow::Owned(path),
            url: Some(Cow::Owned(absolute)),
        })
    }

    /// Get the full path of an item (e.g., "std::vec::Vec")
    fn get_item_full_path(&self, item: DocRef<'_, Item>) -> String {
        if let Some(path) = item.path() {
            path.to_string()
        } else if let Some(name) = item.name() {
            format!("{}::{}", item.crate_docs().name(), name)
        } else {
            item.crate_docs().name().to_string()
        }
    }

    /// Get documentation to show for an item
    ///
    /// Returns None if no docs should be shown, Some(docs) if docs should be displayed.
    /// Docs are wrapped in a TruncatedBlock with appropriate level hint.
    pub(crate) fn docs_to_show(
        &self,
        item: DocRef<'a, Item>,
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
