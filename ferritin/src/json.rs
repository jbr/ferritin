//! Serialize the semantic [`ItemDoc`] model to JSON for the `--format json`
//! output.
//!
//! This consumes the domain IR directly — not the lowered presentation
//! `Document` — so a structural item like a struct serializes as
//! `{ name, fields: [...], methods: [...] }` rather than a flat code block. That
//! is the property that lets a client render at a higher level than "turn JSON
//! into HTML": grouping, filtering, and collapsing are pure client concerns
//! because the server shipped structure, not presentation.
//!
//! Leaf references survive as `JsonSpan { text, style, url }` — the `url` is the
//! resolved navigation target (an intra-doc link), which is the hypermedia
//! pointer a client follows. Kinds not yet modeled fall back to a generic
//! serialization of their lowered presentation nodes.

use crate::format::{ItemBody, ItemDoc, PlainField, StructDoc, StructShape, TupleField};
use crate::styled_string::{
    Document, DocumentNode, HeadingLevel, ListItem, MetadataField, ShowWhen, Span, SpanStyle,
    TableCell, TruncationLevel,
};
use serde::Serialize;
use std::borrow::Cow;

/// Serialize an item model to a JSON string.
pub(crate) fn to_string(
    item: &ItemDoc<'_>,
    canonical_url: Option<String>,
) -> sonic_rs::Result<String> {
    sonic_rs::to_string(&JsonItem::new(item, canonical_url))
}

/// Serialize a presentation [`Document`] to JSON. This is the generic,
/// presentation-level representation used by commands that don't have a
/// structural domain model yet (`search`, `list`) — faithful JSON of the
/// rendered nodes, not a domain model. `get` uses the structural [`to_string`]
/// path instead. A structural search-results model is a future work unit.
pub(crate) fn document_to_string(document: &Document<'_>) -> sonic_rs::Result<String> {
    sonic_rs::to_string(&JsonDocument::new(document))
}

/// Pretty-printed variant of [`to_string`], for readable snapshot tests. Same
/// DTOs, so the field order matches the compact production output.
#[cfg(test)]
pub(crate) fn to_pretty_string(item: &ItemDoc<'_>, canonical_url: Option<String>) -> String {
    sonic_rs::to_string_pretty(&JsonItem::new(item, canonical_url)).unwrap()
}

/// Pretty-printed variant of [`document_to_string`], for snapshot tests.
#[cfg(test)]
pub(crate) fn document_to_pretty_string(document: &Document<'_>) -> String {
    sonic_rs::to_string_pretty(&JsonDocument::new(document)).unwrap()
}

/// Generic JSON wrapper for a presentation [`Document`] — just its nodes.
#[derive(Serialize)]
struct JsonDocument<'a> {
    nodes: Vec<JsonNode<'a>>,
}

impl<'a> JsonDocument<'a> {
    fn new(document: &Document<'a>) -> Self {
        Self {
            nodes: json_nodes(&document.nodes),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonItem<'a> {
    /// Canonical URL for this item (its docs.rs / std-docs page).
    #[serde(skip_serializing_if = "Option::is_none")]
    canonical_url: Option<String>,
    /// Metadata block + the item's own doc prose, as generic nodes.
    header: Vec<JsonNode<'a>>,
    body: JsonBody<'a>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    source: Vec<JsonNode<'a>>,
}

impl<'a> JsonItem<'a> {
    fn new(item: &ItemDoc<'a>, canonical_url: Option<String>) -> Self {
        Self {
            canonical_url,
            header: json_nodes(&item.header),
            body: JsonBody::new(&item.body),
            source: json_nodes(&item.source),
        }
    }
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum JsonBody<'a> {
    Struct(JsonStruct<'a>),
    /// A kind not yet modeled structurally: its lowered presentation nodes.
    Presentation { nodes: Vec<JsonNode<'a>> },
}

impl<'a> JsonBody<'a> {
    fn new(body: &ItemBody<'a>) -> Self {
        match body {
            ItemBody::Struct(model) => JsonBody::Struct(JsonStruct::new(model)),
            ItemBody::Presentation(nodes) => JsonBody::Presentation {
                nodes: json_nodes(nodes),
            },
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonStruct<'a> {
    name: &'a str,
    /// `"plain"`, `"tuple"`, or `"unit"`.
    shape: &'static str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    generics: Vec<JsonSpan<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    where_clause: Vec<JsonSpan<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    fields: Vec<JsonField<'a>>,
    #[serde(skip_serializing_if = "is_zero")]
    hidden_field_count: usize,
    /// Associated inherent methods — still rendered presentation nodes, until
    /// they get a structural model of their own (Unit 3).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    methods: Vec<JsonNode<'a>>,
}

impl<'a> JsonStruct<'a> {
    fn new(model: &StructDoc<'a>) -> Self {
        let (shape, fields, hidden_field_count) = match &model.shape {
            StructShape::Unit => ("unit", Vec::new(), 0),
            StructShape::Plain {
                fields,
                hidden_count,
                ..
            } => (
                "plain",
                fields.iter().map(JsonField::from_plain).collect(),
                *hidden_count,
            ),
            StructShape::Tuple {
                fields,
                hidden_count,
            } => (
                "tuple",
                fields.iter().map(JsonField::from_tuple).collect(),
                *hidden_count,
            ),
        };

        Self {
            name: model.name,
            shape,
            generics: json_spans(&model.generics),
            where_clause: json_spans(&model.where_clause),
            fields,
            hidden_field_count,
            methods: json_nodes(&model.methods),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonField<'a> {
    /// Field name, for plain-struct fields.
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<&'a str>,
    /// Positional index, for tuple-struct fields.
    #[serde(skip_serializing_if = "Option::is_none")]
    index: Option<usize>,
    #[serde(rename = "pub")]
    is_pub: bool,
    #[serde(rename = "type")]
    type_signature: Vec<JsonSpan<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    docs: Vec<JsonNode<'a>>,
}

impl<'a> JsonField<'a> {
    fn from_plain(field: &PlainField<'a>) -> Self {
        Self {
            name: field.name,
            index: None,
            is_pub: field.is_pub,
            type_signature: json_spans(&field.type_spans),
            docs: field.docs.as_deref().map(json_nodes).unwrap_or_default(),
        }
    }

    fn from_tuple(field: &TupleField<'a>) -> Self {
        Self {
            name: None,
            index: Some(field.index),
            is_pub: field.is_pub,
            type_signature: json_spans(&field.type_spans),
            docs: field.docs.as_deref().map(json_nodes).unwrap_or_default(),
        }
    }
}

/// JSON mirror of [`DocumentNode`] — a faithful serialization of the
/// presentation IR, used for the header, docs, source, opaque methods, and
/// not-yet-modeled bodies.
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum JsonNode<'a> {
    Paragraph {
        spans: Vec<JsonSpan<'a>>,
    },
    Metadata {
        fields: Vec<JsonMetadataField<'a>>,
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
    TruncatedBlock {
        nodes: Vec<JsonNode<'a>>,
        level: TruncationLevel,
    },
    Conditional {
        show_when: ShowWhen,
        nodes: Vec<JsonNode<'a>>,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonMetadataField<'a> {
    label: Cow<'a, str>,
    value: Vec<JsonSpan<'a>>,
}

#[derive(Serialize)]
struct JsonListItem<'a> {
    content: Vec<JsonNode<'a>>,
}

#[derive(Serialize)]
struct JsonTableCell<'a> {
    spans: Vec<JsonSpan<'a>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonSpan<'a> {
    text: Cow<'a, str>,
    style: SpanStyle,
    /// Resolved navigation target (intra-doc link), when the span points at
    /// another item — the hypermedia pointer a client follows.
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<Cow<'a, str>>,
}

fn is_zero(count: &usize) -> bool {
    *count == 0
}

fn json_nodes<'a>(nodes: &[DocumentNode<'a>]) -> Vec<JsonNode<'a>> {
    nodes.iter().map(json_node).collect()
}

fn json_spans<'a>(spans: &[Span<'a>]) -> Vec<JsonSpan<'a>> {
    spans.iter().map(json_span).collect()
}

fn json_span<'a>(span: &Span<'a>) -> JsonSpan<'a> {
    JsonSpan {
        text: span.text.clone(),
        style: span.style,
        url: span.url(),
    }
}

fn json_node<'a>(node: &DocumentNode<'a>) -> JsonNode<'a> {
    match node {
        DocumentNode::Paragraph { spans } => JsonNode::Paragraph {
            spans: json_spans(spans),
        },
        DocumentNode::Metadata { fields } => JsonNode::Metadata {
            fields: fields.iter().map(json_metadata_field).collect(),
        },
        DocumentNode::Heading { level, spans } => JsonNode::Heading {
            level: *level,
            spans: json_spans(spans),
        },
        DocumentNode::Section { title, nodes } => JsonNode::Section {
            title: title.as_deref().map(json_spans),
            nodes: json_nodes(nodes),
        },
        DocumentNode::List { items } => JsonNode::List {
            items: items.iter().map(json_list_item).collect(),
        },
        DocumentNode::CodeBlock { lang, code } => JsonNode::CodeBlock {
            lang: lang.clone(),
            code: code.clone(),
        },
        DocumentNode::GeneratedCode { spans } => JsonNode::GeneratedCode {
            spans: json_spans(spans),
        },
        DocumentNode::HorizontalRule => JsonNode::HorizontalRule,
        DocumentNode::BlockQuote { nodes } => JsonNode::BlockQuote {
            nodes: json_nodes(nodes),
        },
        DocumentNode::Table { header, rows } => JsonNode::Table {
            header: header
                .as_ref()
                .map(|cells| cells.iter().map(json_table_cell).collect()),
            rows: rows
                .iter()
                .map(|row| row.iter().map(json_table_cell).collect())
                .collect(),
        },
        DocumentNode::TruncatedBlock { nodes, level } => JsonNode::TruncatedBlock {
            nodes: json_nodes(nodes),
            level: *level,
        },
        DocumentNode::Conditional { show_when, nodes } => JsonNode::Conditional {
            show_when: *show_when,
            nodes: json_nodes(nodes),
        },
    }
}

fn json_metadata_field<'a>(field: &MetadataField<'a>) -> JsonMetadataField<'a> {
    JsonMetadataField {
        label: field.label.clone(),
        value: json_spans(&field.value),
    }
}

fn json_list_item<'a>(item: &ListItem<'a>) -> JsonListItem<'a> {
    JsonListItem {
        content: json_nodes(&item.content),
    }
}

fn json_table_cell<'a>(cell: &TableCell<'a>) -> JsonTableCell<'a> {
    JsonTableCell {
        spans: json_spans(&cell.spans),
    }
}
