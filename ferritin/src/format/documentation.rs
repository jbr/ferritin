use std::borrow::Cow;

use super::*;
use crate::markdown::MarkdownRenderer;
use crate::styled_string::{DocumentNode, LinkTarget, TruncationLevel};
use rustdoc_types::ItemKind;

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
        &'a self,
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
    fn extract_link_target<'a>(
        &'a self,
        origin: DocRef<'a, Item>,
        url: &str,
    ) -> Option<LinkTarget<'a>> {
        // Handle fragment-only links
        if url.starts_with('#') {
            return None; // Keep as-is
        }

        // Handle external URLs
        if url.starts_with("http://") || url.starts_with("https://") {
            return None; // Keep as-is
        }

        // Split off any fragment/anchor
        let (path, _fragment) = url.split_once('#').unwrap_or((url, ""));

        // Check if this is a relative HTML URL (e.g., "task/index.html", "attr.main.html")
        // These are hand-written markdown links in the source that point to HTML docs
        if path.ends_with(".html") || path.contains("/") {
            log::trace!("extract_link_target: parsing relative URL '{}'", path);

            // Try to extract the item path from the HTML filename for navigation
            if let Some(item_path) = self.parse_html_path_to_item_path(origin, path) {
                log::trace!("  → Extracted item path: '{}'", item_path);
                return Some(LinkTarget::Path(Cow::Owned(item_path)));
            } else {
                // Can't parse to an item path - return None to keep as external URL
                log::trace!("  → Could not extract item path, keeping as external URL");
                return None;
            }
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
            if let Some(item_summary) = origin.crate_docs().paths.get(link_id) {
                log::trace!(
                    "  ✓ Found in paths map: {:?}, kind: {:?}",
                    item_summary.path,
                    item_summary.kind
                );
                let full_path = item_summary.path.join("::");
                return Some(LinkTarget::Path(Cow::Owned(full_path)));
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
        Some(LinkTarget::Path(Cow::Owned(qualified_path)))
    }

    /// Parse a relative HTML path to an item path for navigation
    ///
    /// Examples:
    /// - `./macro.trace.html` → `log::trace`
    /// - `macro.trace.html` → `log::trace`
    /// - `task/index.html` → `tokio::task`
    /// - `task/spawn/index.html` → `tokio::task::spawn`
    /// - `struct.TcpStream.html` → `tokio::TcpStream`
    fn parse_html_path_to_item_path(
        &self,
        origin: DocRef<'_, Item>,
        html_path: &str,
    ) -> Option<String> {
        let crate_name = origin.crate_docs().name();

        // Strip leading ./
        let path = html_path.strip_prefix("./").unwrap_or(html_path);

        // Must end with .html
        if !path.ends_with(".html") {
            return None;
        }

        // Handle module paths: "task/index.html" or "task/spawn/index.html"
        if path.ends_with("/index.html") {
            let module_path = path.strip_suffix("/index.html")?;
            let module_parts: Vec<&str> = module_path.split('/').collect();
            return Some(format!("{}::{}", crate_name, module_parts.join("::")));
        }

        // Handle item-specific HTML files: "struct.Name.html", "macro.trace.html", etc.
        // Format: "{kind}.{name}.html"
        let without_html = path.strip_suffix(".html")?;

        // Check for kind.name pattern
        if let Some((_kind, name)) = without_html.split_once('.') {
            // The kind prefix (struct, enum, macro, etc.) tells us the type,
            // but we just need the name for the path
            return Some(format!("{}::{}", crate_name, name));
        }

        // Fallback: if no dot separator, treat whole thing as name
        // (e.g., "something.html" -> "crate::something")
        Some(format!("{}::{}", crate_name, without_html))
    }

    /// Convert a relative HTML URL to an absolute docs.rs URL
    ///
    /// Hand-written markdown in documentation often contains relative HTML links
    /// like `task/index.html` or `../other_crate/index.html`. We convert these
    /// to absolute URLs based on the current crate's documentation location.
    fn make_relative_url_absolute(&self, origin: DocRef<'_, Item>, relative_url: &str) -> String {
        let crate_docs = origin.crate_docs();
        let crate_name = crate_docs.name();
        let version = crate_docs
            .version()
            .map(|v| v.to_string())
            .unwrap_or_else(|| "latest".to_string());

        let is_std = crate_docs.provenance().is_std();

        let base = if is_std {
            format!("https://doc.rust-lang.org/nightly/{}", crate_name)
        } else {
            format!("https://docs.rs/{}/{}/{}", crate_name, version, crate_name)
        };

        // Join the relative URL with the base
        // Remove leading "./" if present
        let relative = relative_url.strip_prefix("./").unwrap_or(relative_url);

        // If it starts with "../", we can't easily resolve it, just use crate root
        if relative.starts_with("../") {
            return format!("{}/{}", base, relative.trim_start_matches("../"));
        }

        format!("{}/{}", base, relative)
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

    /// Generate a docs.rs URL from a path and ItemKind
    fn generate_url_from_path_and_kind(&self, path: &str, kind: rustdoc_types::ItemKind) -> String {
        let parts: Vec<&str> = path.split("::").collect();
        if parts.is_empty() {
            return String::new();
        }

        let crate_name = parts[0];
        let is_std = matches!(crate_name, "std" | "core" | "alloc" | "proc_macro");

        let base = if is_std {
            "https://doc.rust-lang.org/nightly".to_string()
        } else {
            format!("https://docs.rs/{}/latest", crate_name)
        };

        if parts.len() == 1 {
            // Just the crate name
            return format!("{}/{}/index.html", base, crate_name);
        }

        // Assume the last part is the item name, everything before is the module path
        let module_parts = &parts[1..parts.len() - 1];
        let item_name = parts[parts.len() - 1];

        let module_path = if module_parts.is_empty() {
            crate_name.to_string()
        } else {
            format!("{}/{}", crate_name, module_parts.join("/"))
        };

        // Generate URL based on the actual item kind
        match kind {
            ItemKind::Module => {
                // Modules use the full path with index.html
                let full_module_path = format!("{}/{}", module_path, item_name);
                format!("{}/{}/index.html", base, full_module_path)
            }
            ItemKind::Struct => format!("{}/{}/struct.{}.html", base, module_path, item_name),
            ItemKind::Enum => format!("{}/{}/enum.{}.html", base, module_path, item_name),
            ItemKind::Trait => format!("{}/{}/trait.{}.html", base, module_path, item_name),
            ItemKind::Function => format!("{}/{}/fn.{}.html", base, module_path, item_name),
            ItemKind::TypeAlias => format!("{}/{}/type.{}.html", base, module_path, item_name),
            ItemKind::Constant => format!("{}/{}/constant.{}.html", base, module_path, item_name),
            ItemKind::Static => format!("{}/{}/static.{}.html", base, module_path, item_name),
            ItemKind::Union => format!("{}/{}/union.{}.html", base, module_path, item_name),
            ItemKind::Macro | ItemKind::ProcAttribute | ItemKind::ProcDerive => {
                format!("{}/{}/macro.{}.html", base, module_path, item_name)
            }
            ItemKind::Primitive => format!("{}/{}/primitive.{}.html", base, crate_name, item_name),
            _ => {
                // Fallback for unknown kinds - default to struct guess
                format!("{}/{}/struct.{}.html", base, module_path, item_name)
            }
        }
    }

    /// Generate a heuristic docs.rs URL from a path like "std::vec::Vec" when we don't know the kind
    fn generate_heuristic_url(&self, path: &str) -> String {
        // Default to struct as a reasonable guess
        self.generate_url_from_path_and_kind(path, rustdoc_types::ItemKind::Struct)
    }

    /// Generate a search URL for a path when we can't determine the item kind
    ///
    /// Example: "tokio::something::UnknownType" becomes
    /// "https://docs.rs/tokio/latest/tokio/index.html?search=tokio::something::UnknownType"
    fn generate_search_url(&self, path: &str) -> String {
        let parts: Vec<&str> = path.split("::").collect();
        if parts.is_empty() {
            return String::new();
        }

        let crate_name = parts[0];
        let is_std = matches!(crate_name, "std" | "core" | "alloc" | "proc_macro");

        let base = if is_std {
            "https://doc.rust-lang.org/nightly".to_string()
        } else {
            format!("https://docs.rs/{}/latest", crate_name)
        };

        // Link to the deepest module we can infer, with a search query for the full path
        let module_path = if parts.len() > 2 {
            // Use parent module path
            parts[1..parts.len() - 1].join("/")
        } else if parts.len() == 2 {
            // Just one level deep - link to crate root
            String::new()
        } else {
            String::new()
        };

        let index_path = if module_path.is_empty() {
            format!("{}/{}/index.html", base, crate_name)
        } else {
            format!("{}/{}/{}/index.html", base, crate_name, module_path)
        };

        // Add search query for the full path
        use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
        format!(
            "{}?search={}",
            index_path,
            utf8_percent_encode(path, NON_ALPHANUMERIC)
        )
    }

    /// Generate a documentation URL from html_root_url and item path
    ///
    /// The html_root_url points to the crate root (e.g., "https://docs.rs/tokio/1.49.0")
    /// We need to append the correct path based on item kind and location
    fn generate_url_from_html_root(
        &self,
        html_root: &str,
        path: &[String],
        kind: rustdoc_types::ItemKind,
    ) -> String {
        // Strip trailing slash from html_root to avoid double slashes
        let html_root = html_root.trim_end_matches('/');

        if path.is_empty() {
            return html_root.to_string();
        }

        // path[0] is the crate name, skip it since html_root already includes it
        let crate_name = &path[0];
        let remaining_parts = &path[1..];

        if remaining_parts.is_empty() {
            // Just the crate root
            return format!("{}/{}/index.html", html_root, crate_name);
        }

        let item_name = &remaining_parts[remaining_parts.len() - 1];
        let module_parts = &remaining_parts[..remaining_parts.len() - 1];

        let module_path = if module_parts.is_empty() {
            crate_name.to_string()
        } else {
            format!("{}/{}", crate_name, module_parts.join("/"))
        };

        // Generate URL based on the actual item kind
        use rustdoc_types::ItemKind;
        match kind {
            ItemKind::Module => {
                let full_module_path = format!("{}/{}", module_path, item_name);
                format!("{}/{}/index.html", html_root, full_module_path)
            }
            ItemKind::Struct => format!("{}/{}/struct.{}.html", html_root, module_path, item_name),
            ItemKind::Enum => format!("{}/{}/enum.{}.html", html_root, module_path, item_name),
            ItemKind::Trait => format!("{}/{}/trait.{}.html", html_root, module_path, item_name),
            ItemKind::Function => format!("{}/{}/fn.{}.html", html_root, module_path, item_name),
            ItemKind::TypeAlias => format!("{}/{}/type.{}.html", html_root, module_path, item_name),
            ItemKind::Constant => {
                format!("{}/{}/constant.{}.html", html_root, module_path, item_name)
            }
            ItemKind::Static => format!("{}/{}/static.{}.html", html_root, module_path, item_name),
            ItemKind::Union => format!("{}/{}/union.{}.html", html_root, module_path, item_name),
            ItemKind::Macro | ItemKind::ProcAttribute | ItemKind::ProcDerive => {
                format!("{}/{}/macro.{}.html", html_root, module_path, item_name)
            }
            ItemKind::Primitive => {
                format!("{}/{}/primitive.{}.html", html_root, crate_name, item_name)
            }
            _ => {
                // Fallback for unknown kinds
                format!("{}/{}/struct.{}.html", html_root, module_path, item_name)
            }
        }
    }

    /// Get documentation to show for an item
    ///
    /// Returns None if no docs should be shown, Some(docs) if docs should be displayed.
    /// Docs are wrapped in a TruncatedBlock with appropriate level hint.
    pub(crate) fn docs_to_show<'a>(
        &'a self,
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
