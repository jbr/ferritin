use super::json_document::{JsonDocument, JsonListItem, JsonNode, JsonSpan, JsonTableCell};
use crate::document::{Document, DocumentNode, Span, TuiAction};
use crate::request::Request;
use ferritin_common::DocRef;
use rustdoc_types::Item;
use std::borrow::Cow;

impl Request {
    /// Transform a Document into JSON-serializable format
    /// Resolves all TuiActions to local URLs
    pub fn render_to_json<'a>(&'a self, document: Document<'a>) -> JsonDocument<'a> {
        JsonDocument::default().with_nodes(
            document
                .into_nodes()
                .into_iter()
                .map(|node| self.transform_node(node))
                .collect(),
        )
    }

    fn transform_node<'a>(&'a self, node: DocumentNode<'a>) -> JsonNode<'a> {
        match node {
            DocumentNode::Paragraph { spans } => JsonNode::Paragraph {
                spans: spans.into_iter().map(|s| self.transform_span(s)).collect(),
            },

            DocumentNode::Heading { level, spans } => JsonNode::Heading {
                level,
                spans: spans.into_iter().map(|s| self.transform_span(s)).collect(),
            },

            DocumentNode::Section { title, nodes } => JsonNode::Section {
                title: title
                    .map(|spans| spans.into_iter().map(|s| self.transform_span(s)).collect()),
                nodes: nodes.into_iter().map(|n| self.transform_node(n)).collect(),
            },

            DocumentNode::List { items } => JsonNode::List {
                items: items
                    .into_iter()
                    .map(|item| JsonListItem {
                        content: item
                            .content
                            .into_iter()
                            .map(|n| self.transform_node(n))
                            .collect(),
                    })
                    .collect(),
            },

            DocumentNode::CodeBlock { lang, code } => JsonNode::CodeBlock { lang, code },

            DocumentNode::GeneratedCode { spans } => JsonNode::GeneratedCode {
                spans: spans.into_iter().map(|s| self.transform_span(s)).collect(),
            },

            DocumentNode::HorizontalRule => JsonNode::HorizontalRule,

            DocumentNode::BlockQuote { nodes } => JsonNode::BlockQuote {
                nodes: nodes.into_iter().map(|n| self.transform_node(n)).collect(),
            },

            DocumentNode::Table { header, rows } => JsonNode::Table {
                header: header.map(|cells| {
                    cells
                        .into_iter()
                        .map(|cell| JsonTableCell {
                            spans: cell
                                .spans
                                .into_iter()
                                .map(|s| self.transform_span(s))
                                .collect(),
                        })
                        .collect()
                }),
                rows: rows
                    .into_iter()
                    .map(|row| {
                        row.into_iter()
                            .map(|cell| JsonTableCell {
                                spans: cell
                                    .spans
                                    .into_iter()
                                    .map(|s| self.transform_span(s))
                                    .collect(),
                            })
                            .collect()
                    })
                    .collect(),
            },

            // Apply truncation server-side to reduce transport cost
            DocumentNode::TruncatedBlock { nodes, level } => {
                use crate::document::TruncationLevel;

                let truncated_nodes = match level {
                    TruncationLevel::SingleLine => {
                        // Show first node only (heading or paragraph)
                        if let Some(first) = nodes.first() {
                            match first {
                                DocumentNode::Heading { spans, .. } => {
                                    // Just the heading text as a paragraph, no decoration
                                    vec![DocumentNode::Paragraph {
                                        spans: spans.clone(),
                                    }]
                                }
                                _ => vec![first.clone()],
                            }
                        } else {
                            vec![]
                        }
                    }
                    TruncationLevel::Brief => {
                        // Show first paragraph/node only, skip code blocks and lists
                        nodes
                            .iter()
                            .take(1)
                            .filter(|node| {
                                !matches!(
                                    node,
                                    DocumentNode::CodeBlock { .. }
                                        | DocumentNode::GeneratedCode { .. }
                                        | DocumentNode::List { .. }
                                )
                            })
                            .cloned()
                            .collect()
                    }
                    TruncationLevel::Full => nodes,
                };

                JsonNode::Section {
                    title: None,
                    nodes: truncated_nodes
                        .into_iter()
                        .map(|n| self.transform_node(n))
                        .collect(),
                }
            }

            // Flatten Conditional - web is always interactive
            DocumentNode::Conditional { nodes, show_when } => {
                use crate::document::ShowWhen;

                let should_show = match show_when {
                    ShowWhen::Always => true,
                    ShowWhen::Interactive => true,
                    ShowWhen::NonInteractive => false,
                };

                if should_show {
                    JsonNode::Section {
                        title: None,
                        nodes: nodes.into_iter().map(|n| self.transform_node(n)).collect(),
                    }
                } else {
                    // Return empty section for non-interactive content
                    JsonNode::Section {
                        title: None,
                        nodes: vec![],
                    }
                }
            }
        }
    }

    fn transform_span<'a>(&'a self, span: Span<'a>) -> JsonSpan<'a> {
        let url = span.action.as_ref().and_then(|action| match action {
            TuiAction::Navigate { doc_ref, .. } => Some(Cow::Owned(self.item_to_url(*doc_ref))),
            TuiAction::NavigateToPath { path, .. } => Some(Cow::Owned(self.path_to_url(path))),
            TuiAction::OpenUrl(url) => Some(url.clone()),
            TuiAction::ExpandBlock(_) | TuiAction::SelectTheme(_) => None,
        });

        JsonSpan {
            text: span.text,
            style: span.style,
            url,
        }
    }

    /// Convert a DocRef to a local URL like "/tokio@1.49.0::io::AsyncWrite"
    fn item_to_url<'a>(&'a self, item: DocRef<'a, Item>) -> String {
        let crate_docs = item.crate_docs();
        let crate_name = crate_docs.name();
        let version = crate_docs.version();

        let crate_part = match version {
            Some(v) => format!("{}@{}", crate_name, v),
            None => crate_name.to_string(),
        };

        if let Some(summary) = item.summary() {
            // Skip first element (crate name) since it's already in crate_part
            let path_parts: Vec<&str> = summary.path.iter().skip(1).map(|s| s.as_str()).collect();
            if path_parts.is_empty() {
                format!("/{}", crate_part)
            } else {
                format!("/{}::{}", crate_part, path_parts.join("::"))
            }
        } else {
            format!("/{}", crate_part)
        }
    }

    /// Convert a path string to a local URL
    /// Handles paths like "tokio::io::AsyncWrite" or "tokio@1.49::io::AsyncWrite"
    fn path_to_url(&self, path: &str) -> String {
        format!("/{}", path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{DocumentNode, HeadingLevel, ListItem, Span, SpanStyle, TruncationLevel};
    use crate::format_context::FormatContext;
    use ferritin_common::{Navigator, sources::StdSource};

    fn test_request() -> Request {
        let std_source = StdSource::from_rustup();
        let navigator = Navigator::default().with_std_source(std_source);
        let format_context = FormatContext::new();
        Request::new(navigator, format_context)
    }

    #[test]
    fn test_path_to_url_simple() {
        let request = test_request();
        assert_eq!(request.path_to_url("std::vec::Vec"), "/std::vec::Vec");
        assert_eq!(
            request.path_to_url("tokio::io::AsyncWrite"),
            "/tokio::io::AsyncWrite"
        );
    }

    #[test]
    fn test_path_to_url_with_version() {
        let request = test_request();
        assert_eq!(
            request.path_to_url("tokio@1.49::io::AsyncWrite"),
            "/tokio@1.49::io::AsyncWrite"
        );
        assert_eq!(
            request.path_to_url("serde@1.0::Serialize"),
            "/serde@1.0::Serialize"
        );
    }

    #[test]
    fn test_path_to_url_crate_only() {
        let request = test_request();
        assert_eq!(request.path_to_url("std"), "/std");
        assert_eq!(request.path_to_url("tokio@1.49"), "/tokio@1.49");
    }

    #[test]
    fn test_transform_paragraph() {
        let request = test_request();
        let doc = Document::from(vec![Span::plain("Hello "), Span::strong("world")]);

        let json_doc = request.render_to_json(doc);
        assert_eq!(json_doc.nodes().len(), 1);

        match &json_doc.nodes()[0] {
            JsonNode::Paragraph { spans } => {
                assert_eq!(spans.len(), 2);
                assert_eq!(spans[0].text, "Hello ");
                assert_eq!(spans[0].style, SpanStyle::Plain);
                assert_eq!(spans[1].text, "world");
                assert_eq!(spans[1].style, SpanStyle::Strong);
            }
            _ => panic!("Expected paragraph"),
        }
    }

    #[test]
    fn test_transform_heading() {
        let request = test_request();
        let doc = Document::from(DocumentNode::heading(
            HeadingLevel::Title,
            vec![Span::plain("API Documentation")],
        ));

        let json_doc = request.render_to_json(doc);

        match &json_doc.nodes()[0] {
            JsonNode::Heading { level, spans } => {
                assert_eq!(*level, HeadingLevel::Title);
                assert_eq!(spans[0].text, "API Documentation");
            }
            _ => panic!("Expected heading"),
        }
    }

    #[test]
    fn test_transform_code_block() {
        let request = test_request();
        let doc = Document::from(DocumentNode::code_block(Some("rust"), "fn main() {}"));

        let json_doc = request.render_to_json(doc);

        match &json_doc.nodes()[0] {
            JsonNode::CodeBlock { lang, code } => {
                assert_eq!(lang.as_deref(), Some("rust"));
                assert_eq!(code, "fn main() {}");
            }
            _ => panic!("Expected code block"),
        }
    }

    #[test]
    fn test_transform_list() {
        let request = test_request();
        let doc = Document::from(DocumentNode::list(vec![
            ListItem::new(vec![DocumentNode::paragraph(vec![Span::plain("Item 1")])]),
            ListItem::new(vec![DocumentNode::paragraph(vec![Span::plain("Item 2")])]),
        ]));
        let json_doc = request.render_to_json(doc);

        match &json_doc.nodes()[0] {
            JsonNode::List { items } => {
                assert_eq!(items.len(), 2);
            }
            _ => panic!("Expected list"),
        }
    }

    #[test]
    fn test_transform_truncated_block_flattens() {
        let request = test_request();
        let doc = Document::from(DocumentNode::truncated_block(
            vec![DocumentNode::paragraph(vec![Span::plain("Hidden content")])],
            TruncationLevel::Brief,
        ));

        let json_doc = request.render_to_json(doc);

        // Should be flattened to a Section
        match &json_doc.nodes()[0] {
            JsonNode::Section { title, nodes } => {
                assert!(title.is_none());
                assert_eq!(nodes.len(), 1);
            }
            _ => panic!("Expected section (flattened truncated block)"),
        }
    }

    #[test]
    fn test_transform_span_with_path_action() {
        let request = test_request();
        let doc = Document::from(Span::type_name("Vec").with_path("std::vec::Vec"));

        let json_doc = request.render_to_json(doc);

        match &json_doc.nodes()[0] {
            JsonNode::Paragraph { spans } => {
                assert_eq!(spans[0].text, "Vec");
                assert_eq!(spans[0].url.as_deref(), Some("/std::vec::Vec"));
            }
            _ => panic!("Expected paragraph"),
        }
    }
}
