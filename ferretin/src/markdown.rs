use crate::styled_string::{DocumentNode, HeadingLevel, Span, SpanStyle};
use ferretin_common::DocRef;
use pulldown_cmark::{BrokenLink, CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use rustdoc_types::Item;

pub struct MarkdownRenderer;

impl MarkdownRenderer {
    pub fn render_with_resolver<'a, F>(markdown: &str, link_resolver: F) -> Vec<DocumentNode<'a>>
    where
        F: Fn(&str) -> Option<(String, DocRef<'a, Item>)>,
    {
        let callback = |broken_link: BrokenLink| {
            Some((
                broken_link.reference.trim_matches('`').to_string().into(),
                broken_link.reference.clone().into_static(),
            ))
        };

        let parser =
            Parser::new_with_broken_link_callback(markdown, Options::empty(), Some(&callback));

        let mut nodes: Vec<DocumentNode<'a>> = Vec::new();
        let mut current_spans: Vec<Span<'a>> = Vec::new();

        // State tracking
        let mut in_code_block = false;
        let mut code_block_lang: Option<String> = None;
        let mut code_block_content = String::new();
        let mut in_strong = false;
        let mut in_emphasis = false;
        let mut in_strikethrough = false;
        let mut in_heading = false;
        let mut heading_level: Option<HeadingLevel> = None;
        let mut current_link_url: Option<(String, Option<DocRef<'a, Item>>)> = None;
        let mut link_spans: Vec<Span<'a>> = Vec::new();
        let mut in_list = false;
        let mut list_items: Vec<crate::styled_string::ListItem<'a>> = Vec::new();
        let mut in_item = false;
        let mut item_nodes: Vec<DocumentNode<'a>> = Vec::new();

        for event in parser {
            match event {
                Event::Start(tag) => match tag {
                    Tag::CodeBlock(kind) => {
                        in_code_block = true;
                        code_block_lang = match kind {
                            CodeBlockKind::Fenced(lang) => {
                                if lang.is_empty() {
                                    None
                                } else {
                                    Some(lang.to_string())
                                }
                            }
                            CodeBlockKind::Indented => None,
                        };
                        code_block_content.clear();
                    }
                    Tag::Emphasis => {
                        in_emphasis = true;
                    }
                    Tag::Strong => {
                        in_strong = true;
                    }
                    Tag::Strikethrough => {
                        in_strikethrough = true;
                    }
                    Tag::Link { dest_url, .. } => {
                        let resolved_url = link_resolver(dest_url.as_ref())
                            .map(|(link, item)| (link, Some(item)))
                            .unwrap_or_else(|| (dest_url.to_string(), None));
                        current_link_url = Some(resolved_url);
                    }
                    Tag::Heading { level, .. } => {
                        in_heading = true;
                        // Map pulldown_cmark HeadingLevel to our HeadingLevel
                        heading_level = Some(match level {
                            pulldown_cmark::HeadingLevel::H1 => HeadingLevel::Title,
                            _ => HeadingLevel::Section,
                        });
                    }
                    Tag::List(_) => {
                        in_list = true;
                        list_items.clear();
                    }
                    Tag::Item => {
                        in_item = true;
                        item_nodes.clear();
                    }
                    Tag::Paragraph | Tag::BlockQuote(_) => {
                        // These will be handled in TagEnd
                    }
                    _ => {}
                },
                Event::End(tag_end) => match tag_end {
                    TagEnd::Paragraph => {
                        // Skip paragraph handling inside list items (handled by Item end)
                        if !in_item {
                            // Flush current spans and add paragraph break
                            for span in current_spans.drain(..) {
                                nodes.push(DocumentNode::Span(span));
                            }
                            nodes.push(DocumentNode::Span(Span::plain("\n\n")));
                        }
                    }
                    TagEnd::Heading(_level) => {
                        if in_heading {
                            if let Some(level) = heading_level {
                                nodes.push(DocumentNode::Heading {
                                    level,
                                    spans: std::mem::take(&mut current_spans),
                                });
                            }
                            in_heading = false;
                            heading_level = None;
                        }
                    }
                    TagEnd::CodeBlock => {
                        if in_code_block {
                            // Strip hidden lines for Rust code
                            let code = if matches!(code_block_lang.as_deref(), Some("rust") | None)
                            {
                                Self::strip_hidden_lines(&code_block_content)
                            } else {
                                code_block_content.clone()
                            };

                            nodes.push(DocumentNode::code_block(code_block_lang.take(), code));
                            in_code_block = false;
                        }
                    }
                    TagEnd::Emphasis => {
                        in_emphasis = false;
                    }
                    TagEnd::Strong => {
                        in_strong = false;
                    }
                    TagEnd::Strikethrough => {
                        in_strikethrough = false;
                    }
                    TagEnd::Link => {
                        if let Some((url, item)) = current_link_url.take() {
                            let link_text = std::mem::take(&mut link_spans);
                            let link_node = DocumentNode::Link {
                                url,
                                text: link_text,
                                item,
                            };

                            if in_item {
                                // Add link to the current item's content
                                item_nodes.push(link_node);
                            } else {
                                // Flush current_spans to preserve order
                                for span in current_spans.drain(..) {
                                    nodes.push(DocumentNode::Span(span));
                                }
                                nodes.push(link_node);
                            }
                        }
                    }
                    TagEnd::BlockQuote(_) => {
                        // Simplified: just treat as plain text for now
                        for span in current_spans.drain(..) {
                            nodes.push(DocumentNode::Span(span));
                        }
                    }
                    TagEnd::List(_) => {
                        if in_list {
                            // Flush current spans before list
                            for span in current_spans.drain(..) {
                                nodes.push(DocumentNode::Span(span));
                            }

                            // Create the list node
                            let items = std::mem::take(&mut list_items);
                            nodes.push(DocumentNode::List { items });
                            in_list = false;
                        }
                    }
                    TagEnd::Item => {
                        if in_item {
                            // Create list item from collected nodes
                            let nodes = std::mem::take(&mut item_nodes);
                            list_items.push(crate::styled_string::ListItem::new(nodes));
                            in_item = false;
                        }
                    }
                    _ => {}
                },
                Event::Text(text) => {
                    if in_code_block {
                        code_block_content.push_str(&text);
                    } else {
                        let style = if in_strong {
                            SpanStyle::Strong
                        } else if in_emphasis {
                            SpanStyle::Emphasis
                        } else if in_strikethrough {
                            SpanStyle::Strikethrough
                        } else {
                            SpanStyle::Plain
                        };

                        let span = Span {
                            text: text.to_string().into(),
                            style,
                            action: None,
                        };

                        if current_link_url.is_some() {
                            link_spans.push(span);
                        } else if in_item {
                            item_nodes.push(DocumentNode::Span(span));
                        } else {
                            current_spans.push(span);
                        }
                    }
                }
                Event::Code(code) => {
                    let span = Span::inline_code(code.to_string());
                    if current_link_url.is_some() {
                        link_spans.push(span);
                    } else if in_item {
                        item_nodes.push(DocumentNode::Span(span));
                    } else {
                        current_spans.push(span);
                    }
                }
                Event::SoftBreak => {
                    let span = Span::plain(" ");
                    if current_link_url.is_some() {
                        link_spans.push(span);
                    } else if in_item {
                        item_nodes.push(DocumentNode::Span(span));
                    } else {
                        current_spans.push(span);
                    }
                }
                Event::HardBreak => {
                    let span = Span::plain("\n");
                    if current_link_url.is_some() {
                        link_spans.push(span);
                    } else if in_item {
                        item_nodes.push(DocumentNode::Span(span));
                    } else {
                        current_spans.push(span);
                    }
                }
                Event::Rule => {
                    nodes.push(DocumentNode::HorizontalRule);
                }
                _ => {}
            }
        }

        // Flush any remaining spans
        for span in current_spans {
            nodes.push(DocumentNode::Span(span));
        }

        nodes
    }

    /// Strip hidden lines from Rust code examples
    /// Lines starting with `# ` (hash followed by space) are hidden from display
    /// but included in doctests for completeness
    /// Skip lines that start with "# " (hash followed by space)
    /// But keep lines like "#[derive(...)]" or "#![feature(...)]"
    fn strip_hidden_lines(code: &str) -> String {
        code.lines()
            .filter(|line| !line.starts_with("# "))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_markdown() {
        let input = "This is **bold** and this is *italic*.";
        let nodes = MarkdownRenderer::render_with_resolver(input, |_| None);
        assert!(!nodes.is_empty());
        // Should contain spans with Strong and Emphasis styles
    }

    #[test]
    fn test_code_block() {
        let input = "```rust\nfn main() {\n    println!(\"Hello\");\n}\n```";
        let nodes = MarkdownRenderer::render_with_resolver(input, |_| None);
        assert!(!nodes.is_empty());
        // Should contain a CodeBlock node
        assert!(
            nodes
                .iter()
                .any(|n| matches!(n, DocumentNode::CodeBlock { .. }))
        );
    }

    #[test]
    fn test_link() {
        let input = "See [this link](https://example.com) for more.";
        let nodes = MarkdownRenderer::render_with_resolver(input, |_| None);
        assert!(!nodes.is_empty());
        // Should contain a Link node
        assert!(nodes.iter().any(|n| matches!(n, DocumentNode::Link { .. })));
    }

    #[test]
    fn test_heading() {
        let input = "# Main Title\n\n## Subsection";
        let nodes = MarkdownRenderer::render_with_resolver(input, |_| None);
        assert!(!nodes.is_empty());
        // Should contain Heading nodes
        let headings: Vec<_> = nodes
            .iter()
            .filter(|n| matches!(n, DocumentNode::Heading { .. }))
            .collect();
        assert_eq!(headings.len(), 2, "Expected 2 heading nodes");

        // Check first is Title level
        if let DocumentNode::Heading { level, .. } = &headings[0] {
            assert!(matches!(level, HeadingLevel::Title));
        }

        // Check second is Section level
        if let DocumentNode::Heading { level, .. } = &headings[1] {
            assert!(matches!(level, HeadingLevel::Section));
        }
    }

    #[test]
    fn test_links_in_list_items() {
        let input = "- Item with [link](https://example.com) inline\n- Another [link](https://other.com) here";
        let nodes = MarkdownRenderer::render_with_resolver(input, |_| None);

        // Should have exactly one list
        let lists: Vec<_> = nodes
            .iter()
            .filter(|n| matches!(n, DocumentNode::List { .. }))
            .collect();
        assert_eq!(lists.len(), 1, "Expected exactly 1 list node");

        // Check that the list has 2 items with links properly nested
        if let DocumentNode::List { items } = lists[0] {
            assert_eq!(items.len(), 2, "Expected 2 list items");

            // First item should contain a link
            let first_has_link = items[0]
                .content
                .iter()
                .any(|n| matches!(n, DocumentNode::Link { .. }));
            assert!(first_has_link, "First list item should contain a link");

            // Second item should contain a link
            let second_has_link = items[1]
                .content
                .iter()
                .any(|n| matches!(n, DocumentNode::Link { .. }));
            assert!(second_has_link, "Second list item should contain a link");
        } else {
            panic!("Expected a List node");
        }
    }
}
