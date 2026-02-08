use crate::styled_string::{
    DocumentNode, HeadingLevel, LinkTarget, ListItem, Span, SpanStyle, TuiAction,
};
use pulldown_cmark::{BrokenLink, CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

/// Stack item for building the document tree
/// We need this because Lists contain ListItems (not DocumentNodes directly)
enum StackItem<'a> {
    Node(DocumentNode<'a>),
    Item(ListItem<'a>),
}

pub struct MarkdownRenderer;

impl MarkdownRenderer {
    /// Render markdown with an optional link resolver
    ///
    /// The link_resolver returns a LinkTarget for intra-doc links, which can be
    /// either a resolved DocRef or an unresolved path. URL generation is deferred
    /// to the renderer that needs it.
    pub fn render_with_resolver<'a, F>(markdown: &str, link_resolver: F) -> Vec<DocumentNode<'a>>
    where
        F: Fn(&str) -> Option<LinkTarget<'a>>,
    {
        let callback = |broken_link: BrokenLink| {
            Some((
                broken_link.reference.trim_matches('`').to_string().into(),
                broken_link.reference.clone().into_static(),
            ))
        };

        let mut options = Options::empty();
        options.insert(Options::ENABLE_TABLES);
        let parser = Parser::new_with_broken_link_callback(markdown, options, Some(&callback));

        let mut root: Vec<DocumentNode<'a>> = Vec::new();
        let mut stack: Vec<StackItem<'a>> = Vec::new();
        let mut current_spans: Vec<Span<'a>> = Vec::new();

        // Inline style state (doesn't nest structurally)
        let mut in_code_block = false;
        let mut code_block_lang: Option<String> = None;
        let mut code_block_content = String::new();
        let mut in_strong = false;
        let mut in_emphasis = false;
        let mut in_strikethrough = false;
        let mut in_heading = false;
        let mut heading_level: Option<HeadingLevel> = None;
        let mut current_link_action: Option<TuiAction<'a>> = None;

        // Table state
        let mut in_table_head = false;
        let mut table_header: Option<Vec<crate::styled_string::TableCell<'a>>> = None;
        let mut table_rows: Vec<Vec<crate::styled_string::TableCell<'a>>> = Vec::new();
        let mut current_row: Vec<crate::styled_string::TableCell<'a>> = Vec::new();

        for event in parser {
            match event {
                Event::Start(tag) => match tag {
                    Tag::CodeBlock(kind) => {
                        in_code_block = true;
                        code_block_lang = Some(match kind {
                            CodeBlockKind::Fenced(lang) => {
                                match lang.split(',').next().unwrap_or(&*lang) {
                                    "no_run" | "should_panic" | "ignore" | "compile_fail"
                                    | "edition2015" | "edition2018" | "edition2021"
                                    | "edition2024" | "rust" | "" => "rust".to_string(),
                                    other => other.to_string(),
                                }
                            }
                            CodeBlockKind::Indented => "rust".to_string(),
                        });
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
                        // Resolve the link and determine the action
                        let action = if let Some(target) = link_resolver(dest_url.as_ref()) {
                            match target {
                                LinkTarget::Resolved(doc_ref) => TuiAction::Navigate {
                                    doc_ref,
                                    url: None, // URL generation deferred to renderer
                                },
                                LinkTarget::Path(path) => TuiAction::NavigateToPath {
                                    path,
                                    url: None, // URL generation deferred to renderer
                                },
                            }
                        } else {
                            TuiAction::OpenUrl(dest_url.to_string().into())
                        };
                        current_link_action = Some(action);
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
                        // Flush any accumulated spans before starting a list
                        if !current_spans.is_empty() {
                            let para = DocumentNode::Paragraph {
                                spans: std::mem::take(&mut current_spans),
                            };
                            Self::push_to_parent(&mut stack, &mut root, StackItem::Node(para));
                        }
                        stack.push(StackItem::Node(DocumentNode::List { items: vec![] }));
                    }
                    Tag::Item => {
                        stack.push(StackItem::Item(ListItem::new(vec![])));
                    }
                    Tag::BlockQuote(_) => {
                        // Flush any accumulated spans before starting a blockquote
                        if !current_spans.is_empty() {
                            let para = DocumentNode::Paragraph {
                                spans: std::mem::take(&mut current_spans),
                            };
                            Self::push_to_parent(&mut stack, &mut root, StackItem::Node(para));
                        }
                        stack.push(StackItem::Node(DocumentNode::BlockQuote { nodes: vec![] }));
                    }
                    Tag::Table(_) => {
                        table_header = None;
                        table_rows.clear();
                    }
                    Tag::TableHead => {
                        in_table_head = true;
                        current_row.clear();
                    }
                    Tag::TableRow => {
                        current_row.clear();
                    }
                    Tag::TableCell => {
                        // Cell content will be in current_spans
                    }
                    Tag::Paragraph => {
                        // Paragraphs will be created when we hit TagEnd::Paragraph
                    }
                    _ => {}
                },
                Event::End(tag_end) => match tag_end {
                    TagEnd::Paragraph => {
                        // Create a paragraph node from collected spans
                        let paragraph_spans = std::mem::take(&mut current_spans);
                        if !paragraph_spans.is_empty() {
                            let para = DocumentNode::Paragraph {
                                spans: paragraph_spans,
                            };
                            Self::push_to_parent(&mut stack, &mut root, StackItem::Node(para));
                        }
                    }
                    TagEnd::Heading(_level) => {
                        if in_heading {
                            if let Some(level) = heading_level {
                                let heading = DocumentNode::Heading {
                                    level,
                                    spans: std::mem::take(&mut current_spans),
                                };
                                Self::push_to_parent(
                                    &mut stack,
                                    &mut root,
                                    StackItem::Node(heading),
                                );
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

                            let code_block = DocumentNode::code_block(code_block_lang.take(), code);
                            Self::push_to_parent(
                                &mut stack,
                                &mut root,
                                StackItem::Node(code_block),
                            );
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
                        // Just clear the link action - spans have already been created with it
                        current_link_action = None;
                    }
                    TagEnd::BlockQuote(_) => {
                        // Flush any remaining spans as a paragraph before closing the blockquote
                        if !current_spans.is_empty() {
                            let para = DocumentNode::Paragraph {
                                spans: std::mem::take(&mut current_spans),
                            };
                            Self::push_to_parent(&mut stack, &mut root, StackItem::Node(para));
                        }

                        // Pop the blockquote and push to parent
                        if let Some(StackItem::Node(blockquote)) = stack.pop() {
                            Self::push_to_parent(
                                &mut stack,
                                &mut root,
                                StackItem::Node(blockquote),
                            );
                        }
                    }
                    TagEnd::List(_) => {
                        // Pop the list and push to parent
                        if let Some(StackItem::Node(list)) = stack.pop() {
                            Self::push_to_parent(&mut stack, &mut root, StackItem::Node(list));
                        }
                    }
                    TagEnd::Item => {
                        // Flush any remaining spans as a paragraph before closing the item
                        if !current_spans.is_empty() {
                            let para = DocumentNode::Paragraph {
                                spans: std::mem::take(&mut current_spans),
                            };
                            Self::push_to_parent(&mut stack, &mut root, StackItem::Node(para));
                        }

                        // Pop the item and push to parent list
                        if let Some(StackItem::Item(item)) = stack.pop() {
                            Self::push_to_parent(&mut stack, &mut root, StackItem::Item(item));
                        }
                    }
                    TagEnd::TableCell => {
                        // Create a table cell from collected spans
                        let cell = crate::styled_string::TableCell::new(std::mem::take(
                            &mut current_spans,
                        ));
                        current_row.push(cell);
                    }
                    TagEnd::TableHead => {
                        table_header = Some(std::mem::take(&mut current_row));
                        in_table_head = false;
                    }
                    TagEnd::TableRow => {
                        if !in_table_head {
                            table_rows.push(std::mem::take(&mut current_row));
                        }
                    }
                    TagEnd::Table => {
                        let table = DocumentNode::Table {
                            header: table_header.take(),
                            rows: std::mem::take(&mut table_rows),
                        };
                        Self::push_to_parent(&mut stack, &mut root, StackItem::Node(table));
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
                            action: current_link_action.clone(),
                        };
                        current_spans.push(span);
                    }
                }
                Event::Code(code) => {
                    let mut span = Span::inline_code(code.to_string());
                    span.action = current_link_action.clone();
                    current_spans.push(span);
                }
                Event::SoftBreak => {
                    let mut span = Span::plain(" ");
                    span.action = current_link_action.clone();
                    current_spans.push(span);
                }
                Event::HardBreak => {
                    let mut span = Span::plain("\n");
                    span.action = current_link_action.clone();
                    current_spans.push(span);
                }
                Event::Rule => {
                    Self::push_to_parent(
                        &mut stack,
                        &mut root,
                        StackItem::Node(DocumentNode::HorizontalRule),
                    );
                }
                _ => {}
            }
        }

        // Flush any remaining spans as a paragraph
        if !current_spans.is_empty() {
            root.push(DocumentNode::paragraph(std::mem::take(&mut current_spans)));
        }

        root
    }

    /// Push a completed StackItem to its parent container
    fn push_to_parent<'a>(
        stack: &mut Vec<StackItem<'a>>,
        root: &mut Vec<DocumentNode<'a>>,
        item: StackItem<'a>,
    ) {
        match stack.last_mut() {
            Some(StackItem::Item(list_item)) => {
                // Push DocumentNode to ListItem's content
                match item {
                    StackItem::Node(node) => list_item.content.push(node),
                    StackItem::Item(_) => {
                        panic!(
                            "Cannot nest ListItem directly in ListItem - lists should be nested via DocumentNode::List"
                        )
                    }
                }
            }
            Some(StackItem::Node(DocumentNode::List { items })) => {
                // Push ListItem to List's items
                match item {
                    StackItem::Item(list_item) => items.push(list_item),
                    StackItem::Node(_) => {
                        panic!(
                            "Cannot push DocumentNode directly to List - must be wrapped in ListItem"
                        )
                    }
                }
            }
            Some(StackItem::Node(DocumentNode::BlockQuote { nodes })) => {
                // Push DocumentNode to BlockQuote's nodes
                match item {
                    StackItem::Node(node) => nodes.push(node),
                    StackItem::Item(_) => {
                        panic!(
                            "Cannot push ListItem directly to BlockQuote - lists should be nested via DocumentNode::List"
                        )
                    }
                }
            }
            None => {
                // Push to root
                match item {
                    StackItem::Node(node) => root.push(node),
                    StackItem::Item(_) => {
                        panic!("Cannot push ListItem to root - must be inside a List")
                    }
                }
            }
            _ => {
                panic!("Unexpected parent type on stack")
            }
        }
    }

    /// Strip hidden lines from Rust code examples
    /// Lines starting with `# ` (hash followed by space) are hidden from display
    /// but included in doctests for completeness
    /// Skip lines that start with "# " (hash followed by space)
    /// Also skip lines that are just "#"
    /// But keep lines like "#[derive(...)]" or "#![feature(...)]"
    fn strip_hidden_lines(code: &str) -> String {
        code.lines()
            .filter(|line| {
                let trimmed_start = line.trim_start();
                trimmed_start != "#" && !trimmed_start.starts_with("# ")
            })
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
        // Should contain a Paragraph with a Span that has an action (link)
        let has_link_span = nodes.iter().any(|n| {
            if let DocumentNode::Paragraph { spans } = n {
                spans.iter().any(|s| s.action.is_some())
            } else {
                false
            }
        });
        assert!(
            has_link_span,
            "Should contain a paragraph with a span containing a link action"
        );
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

            // First item should contain a paragraph with a span containing a link
            let first_has_link = items[0].content.iter().any(|n| {
                if let DocumentNode::Paragraph { spans } = n {
                    spans.iter().any(|s| s.action.is_some())
                } else {
                    false
                }
            });
            assert!(first_has_link, "First list item should contain a link");

            // Second item should contain a paragraph with a span containing a link
            let second_has_link = items[1].content.iter().any(|n| {
                if let DocumentNode::Paragraph { spans } = n {
                    spans.iter().any(|s| s.action.is_some())
                } else {
                    false
                }
            });
            assert!(second_has_link, "Second list item should contain a link");
        } else {
            panic!("Expected a List node");
        }
    }
}
