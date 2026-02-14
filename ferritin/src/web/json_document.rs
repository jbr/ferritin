use crate::document::{HeadingLevel, SpanStyle};
use fieldwork::Fieldwork;
use serde::Serialize;
use std::borrow::Cow;

/// A JSON-serializable document with hypermedia links
#[derive(Serialize, Debug, Clone, PartialEq, Fieldwork, Default)]
#[cfg_attr(test, derive(serde::Deserialize))]
#[serde(rename_all = "camelCase")]
#[fieldwork(get, set, with)]
pub struct JsonDocument<'a> {
    /// Canonical URL for this document (e.g., "/tokio/1.49.0/io/AsyncWrite")
    #[serde(skip_serializing_if = "Option::is_none")]
    canonical_url: Option<String>,

    nodes: Vec<JsonNode<'a>>,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
#[cfg_attr(test, derive(serde::Deserialize))]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum JsonNode<'a> {
    Paragraph {
        spans: Vec<JsonSpan<'a>>,
    },
    Heading {
        level: HeadingLevel,
        spans: Vec<JsonSpan<'a>>,
    },
    Section {
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<Vec<JsonSpan<'a>>>,
        nodes: Vec<JsonNode<'a>>,
    },
    List {
        items: Vec<JsonListItem<'a>>,
    },
    CodeBlock {
        #[serde(skip_serializing_if = "Option::is_none")]
        lang: Option<Cow<'a, str>>,
        code: Cow<'a, str>,
    },
    GeneratedCode {
        spans: Vec<JsonSpan<'a>>,
    },
    HorizontalRule,
    BlockQuote {
        nodes: Vec<JsonNode<'a>>,
    },
    Table {
        #[serde(skip_serializing_if = "Option::is_none")]
        header: Option<Vec<JsonTableCell<'a>>>,
        rows: Vec<Vec<JsonTableCell<'a>>>,
    },
}

#[derive(Serialize, Debug, Clone, PartialEq)]
#[cfg_attr(test, derive(serde::Deserialize))]
#[serde(rename_all = "camelCase")]
pub struct JsonSpan<'a> {
    pub text: Cow<'a, str>,
    pub style: SpanStyle,

    /// Local URL for navigation (e.g., "/tokio/1.49.0/io/AsyncWrite")
    /// Resolved from TuiAction during transformation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<Cow<'a, str>>,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub struct JsonListItem<'a> {
    pub content: Vec<JsonNode<'a>>,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub struct JsonTableCell<'a> {
    pub spans: Vec<JsonSpan<'a>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_span_serialization() {
        let span = JsonSpan {
            text: Cow::Borrowed("hello"),
            style: SpanStyle::Plain,
            url: None,
        };

        let json = sonic_rs::to_string(&span).unwrap();
        assert!(json.contains("hello"));
        assert!(json.contains(r#""style":"Plain"#));
    }

    #[test]
    fn test_json_span_with_url() {
        let span = JsonSpan {
            text: Cow::Borrowed("Vec"),
            style: SpanStyle::TypeName,
            url: Some(Cow::Borrowed("/std/1.0.0/vec/Vec")),
        };

        let json = sonic_rs::to_string(&span).unwrap();
        assert!(json.contains("Vec"));
        assert!(json.contains(r#""style":"TypeName"#));
        assert!(json.contains(r#""url":"/std/1.0.0/vec/Vec"#));
    }

    #[test]
    fn test_paragraph_node() {
        let node = JsonNode::Paragraph {
            spans: vec![
                JsonSpan {
                    text: Cow::Borrowed("Hello "),
                    style: SpanStyle::Plain,
                    url: None,
                },
                JsonSpan {
                    text: Cow::Borrowed("world"),
                    style: SpanStyle::Strong,
                    url: None,
                },
            ],
        };

        let json = sonic_rs::to_string(&node).unwrap();
        assert!(json.contains(r#""type":"paragraph"#));
        assert!(json.contains("Hello "));
        assert!(json.contains("world"));
    }

    #[test]
    fn test_heading_node() {
        let node = JsonNode::Heading {
            level: HeadingLevel::Title,
            spans: vec![JsonSpan {
                text: Cow::Borrowed("API Documentation"),
                style: SpanStyle::Plain,
                url: None,
            }],
        };

        let json = sonic_rs::to_string(&node).unwrap();
        assert!(json.contains(r#""type":"heading"#));
        assert!(json.contains(r#""level":"Title"#));
        assert!(json.contains("API Documentation"));
    }

    #[test]
    fn test_code_block_node() {
        let node = JsonNode::CodeBlock {
            lang: Some(Cow::Borrowed("rust")),
            code: Cow::Borrowed("fn main() {}"),
        };

        let json = sonic_rs::to_string(&node).unwrap();
        assert!(json.contains(r#""type":"codeBlock"#));
        assert!(json.contains(r#""lang":"rust"#));
        assert!(json.contains("fn main() {}"));
    }

    #[test]
    fn test_document_with_canonical_url() {
        let doc = JsonDocument {
            canonical_url: Some("/tokio/1.49.0/io/AsyncWrite".into()),
            nodes: vec![JsonNode::Paragraph {
                spans: vec![JsonSpan {
                    text: Cow::Borrowed("Test"),
                    style: SpanStyle::Plain,
                    url: None,
                }],
            }],
        };

        let json = sonic_rs::to_string(&doc).unwrap();
        assert!(json.contains(r#""canonicalUrl":"/tokio/1.49.0/io/AsyncWrite"#));
        assert!(json.contains(r#""type":"paragraph"#));
    }

    #[test]
    fn test_document_without_canonical_url() {
        let doc = JsonDocument {
            canonical_url: None,
            nodes: vec![JsonNode::HorizontalRule],
        };

        let json = sonic_rs::to_string(&doc).unwrap();
        // canonicalUrl should be omitted when None
        assert!(!json.contains("canonicalUrl"));
        assert!(json.contains(r#""type":"horizontalRule"#));
    }

    #[test]
    fn test_section_with_title() {
        let node = JsonNode::Section {
            title: Some(vec![JsonSpan {
                text: Cow::Borrowed("Methods"),
                style: SpanStyle::Plain,
                url: None,
            }]),
            nodes: vec![JsonNode::HorizontalRule],
        };

        let json = sonic_rs::to_string(&node).unwrap();
        assert!(json.contains(r#""type":"section"#));
        assert!(json.contains("Methods"));
    }

    #[test]
    fn test_list_items() {
        let node = JsonNode::List {
            items: vec![
                JsonListItem {
                    content: vec![JsonNode::Paragraph {
                        spans: vec![JsonSpan {
                            text: Cow::Borrowed("Item 1"),
                            style: SpanStyle::Plain,
                            url: None,
                        }],
                    }],
                },
                JsonListItem {
                    content: vec![JsonNode::Paragraph {
                        spans: vec![JsonSpan {
                            text: Cow::Borrowed("Item 2"),
                            style: SpanStyle::Plain,
                            url: None,
                        }],
                    }],
                },
            ],
        };

        let json = sonic_rs::to_string(&node).unwrap();
        assert!(json.contains(r#""type":"list"#));
        assert!(json.contains("Item 1"));
        assert!(json.contains("Item 2"));
    }
}
