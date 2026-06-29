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

use crate::format::{
    AssocKind, ConstantDoc, EnumDoc, FunctionDoc, ItemBody, ItemDoc, ItemMeta, MacroDoc,
    MetaVisibility, MethodDoc, MethodVisibility, ModuleDoc, ModuleItem, PlainField, StaticDoc,
    StructDoc, StructShape, TraitDoc, TraitMember, TupleField, TypeAliasDoc, VariantDoc,
    VariantShape,
};
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
    /// Structured metadata (name, kind, visibility, path, crate).
    meta: JsonMeta,
    /// The item's own doc prose, as generic nodes.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    docs: Vec<JsonNode<'a>>,
    body: JsonBody<'a>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    source: Vec<JsonNode<'a>>,
}

impl<'a> JsonItem<'a> {
    fn new(item: &ItemDoc<'a>, canonical_url: Option<String>) -> Self {
        Self {
            canonical_url,
            meta: JsonMeta::new(&item.meta),
            docs: json_nodes(&item.docs),
            body: JsonBody::new(&item.body),
            source: json_nodes(&item.source),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonMeta {
    name: String,
    /// Lowercased item kind (`"struct"`, `"enum"`, …).
    kind: String,
    /// `"public"`, `"private"`, `"crate"`, or `"restricted"`.
    visibility: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    defined_at: Option<String>,
    crate_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    crate_version: Option<String>,
}

impl JsonMeta {
    fn new(meta: &ItemMeta<'_>) -> Self {
        Self {
            name: meta.name.to_string(),
            kind: meta.kind.clone(),
            visibility: match meta.visibility {
                MetaVisibility::Public => "public",
                MetaVisibility::Private => "private",
                MetaVisibility::Crate => "crate",
                MetaVisibility::Restricted => "restricted",
            },
            defined_at: meta.defined_at.clone(),
            crate_name: meta.crate_name.clone(),
            crate_version: meta.crate_version.clone(),
        }
    }
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum JsonBody<'a> {
    Struct(JsonStruct<'a>),
    Enum(JsonEnum<'a>),
    Trait(JsonTrait<'a>),
    Module(JsonModule<'a>),
    Function(JsonFunction<'a>),
    TypeAlias(JsonTypeAlias<'a>),
    Constant(JsonConstant<'a>),
    Static(JsonStatic<'a>),
    Macro(JsonMacro<'a>),
    /// A kind not yet modeled structurally: its lowered presentation nodes.
    Presentation { nodes: Vec<JsonNode<'a>> },
}

impl<'a> JsonBody<'a> {
    fn new(body: &ItemBody<'a>) -> Self {
        match body {
            ItemBody::Struct(model) => JsonBody::Struct(JsonStruct::new(model)),
            ItemBody::Enum(model) => JsonBody::Enum(JsonEnum::new(model)),
            ItemBody::Trait(model) => JsonBody::Trait(JsonTrait::new(model)),
            ItemBody::Module(model) => JsonBody::Module(JsonModule::new(model)),
            ItemBody::Function(model) => JsonBody::Function(JsonFunction::new(model)),
            ItemBody::TypeAlias(model) => JsonBody::TypeAlias(JsonTypeAlias::new(model)),
            ItemBody::Constant(model) => JsonBody::Constant(JsonConstant::new(model)),
            ItemBody::Static(model) => JsonBody::Static(JsonStatic::new(model)),
            ItemBody::Macro(model) => JsonBody::Macro(JsonMacro::new(model)),
            ItemBody::Presentation(nodes) => JsonBody::Presentation {
                nodes: json_nodes(nodes),
            },
        }
    }
}

/// A type-alias body: the aliased type (RHS of `=`) as a span sequence.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonTypeAlias<'a> {
    name: &'a str,
    aliased: Vec<JsonSpan<'a>>,
}

impl<'a> JsonTypeAlias<'a> {
    fn new(model: &TypeAliasDoc<'a>) -> Self {
        Self {
            name: model.name,
            aliased: json_spans(&model.aliased),
        }
    }
}

/// A constant body: name, type, and optional value expression.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonConstant<'a> {
    name: &'a str,
    #[serde(rename = "type")]
    type_signature: Vec<JsonSpan<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<&'a str>,
}

impl<'a> JsonConstant<'a> {
    fn new(model: &ConstantDoc<'a>) -> Self {
        Self {
            name: model.name,
            type_signature: json_spans(&model.type_signature),
            value: model.value,
        }
    }
}

/// A static body: name, type, and value expression.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonStatic<'a> {
    name: &'a str,
    #[serde(rename = "type")]
    type_signature: Vec<JsonSpan<'a>>,
    value: &'a str,
}

impl<'a> JsonStatic<'a> {
    fn new(model: &StaticDoc<'a>) -> Self {
        Self {
            name: model.name,
            type_signature: json_spans(&model.type_signature),
            value: model.value,
        }
    }
}

/// A macro body: its definition source verbatim.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonMacro<'a> {
    definition: &'a str,
}

impl<'a> JsonMacro<'a> {
    fn new(model: &MacroDoc<'a>) -> Self {
        Self {
            definition: model.definition,
        }
    }
}

/// A free function body. Mirrors [`JsonMethod`] minus the assoc-item
/// `kind`/`visibility` and per-item `docs` — params live inside `signature`, so
/// a function and a method serialize the same way.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonFunction<'a> {
    name: &'a str,
    #[serde(skip_serializing_if = "is_false")]
    is_async: bool,
    #[serde(skip_serializing_if = "is_false")]
    is_const: bool,
    #[serde(skip_serializing_if = "is_false")]
    is_unsafe: bool,
    /// Return-type spans (functions with an explicit output).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    returns: Vec<JsonSpan<'a>>,
    /// Full display signature, the same spans the terminal renders.
    signature: Vec<JsonSpan<'a>>,
}

impl<'a> JsonFunction<'a> {
    fn new(model: &FunctionDoc<'a>) -> Self {
        Self {
            name: model.name,
            is_async: model.is_async,
            is_const: model.is_const,
            is_unsafe: model.is_unsafe,
            returns: json_spans(&model.returns),
            signature: json_spans(&model.signature),
        }
    }
}

/// A module body: its child items as a flat list. Grouping is left to the
/// client — each item carries the `kind` it would group under.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonModule<'a> {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    items: Vec<JsonModuleItem<'a>>,
}

impl<'a> JsonModule<'a> {
    fn new(model: &ModuleDoc<'a>) -> Self {
        Self {
            items: model.items.iter().map(JsonModuleItem::new).collect(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonModuleItem<'a> {
    /// Path as listed (bare name, or `a::b::c` when reached recursively).
    path: String,
    /// Lowercased item kind (`"struct"`, `"enum"`, `"function"`, …).
    kind: String,
    /// Navigation target — the resolved docs.rs / std-docs URL for the child.
    url: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    docs: Vec<JsonNode<'a>>,
}

impl<'a> JsonModuleItem<'a> {
    fn new(item: &ModuleItem<'a>) -> Self {
        Self {
            path: item.path.clone(),
            kind: format!("{:?}", item.kind).to_lowercase(),
            url: crate::generate_docsrs_url::generate_docsrs_url(item.target),
            docs: item.docs.as_deref().map(json_nodes).unwrap_or_default(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonTrait<'a> {
    name: &'a str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    generics: Vec<JsonSpan<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    supertraits: Vec<JsonSpan<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    where_clause: Vec<JsonSpan<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    members: Vec<JsonTraitMember<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    implementors: Vec<JsonNode<'a>>,
}

impl<'a> JsonTrait<'a> {
    fn new(model: &TraitDoc<'a>) -> Self {
        Self {
            name: model.name,
            generics: json_spans(&model.generics),
            supertraits: json_spans(&model.supertraits),
            where_clause: json_spans(&model.where_clause),
            members: model.members.iter().map(JsonTraitMember::new).collect(),
            implementors: json_nodes(&model.implementors),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonTraitMember<'a> {
    name: &'a str,
    /// `"method"`, `"const"`, or `"type"`.
    kind: &'static str,
    /// For methods: `true` if a default body is provided.
    #[serde(skip_serializing_if = "is_false")]
    has_default: bool,
    signature: Vec<JsonSpan<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    docs: Vec<JsonNode<'a>>,
}

impl<'a> JsonTraitMember<'a> {
    fn new(member: &TraitMember<'a>) -> Self {
        Self {
            name: member.name,
            kind: assoc_kind_str(member.kind),
            has_default: member.has_default,
            signature: json_spans(&member.signature),
            docs: member.docs.as_deref().map(json_nodes).unwrap_or_default(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonEnum<'a> {
    name: &'a str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    generics: Vec<JsonSpan<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    where_clause: Vec<JsonSpan<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    variants: Vec<JsonVariant<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    methods: Vec<JsonMethod<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    trait_impls: Vec<JsonNode<'a>>,
}

impl<'a> JsonEnum<'a> {
    fn new(model: &EnumDoc<'a>) -> Self {
        Self {
            name: model.name,
            generics: json_spans(&model.generics),
            where_clause: json_spans(&model.where_clause),
            variants: model.variants.iter().map(JsonVariant::new).collect(),
            methods: model.methods.iter().map(JsonMethod::new).collect(),
            trait_impls: json_nodes(&model.trait_impls),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonVariant<'a> {
    name: &'a str,
    /// `"plain"`, `"tuple"`, or `"struct"`.
    shape: &'static str,
    /// Tuple-variant field types, each a span sequence.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tuple_fields: Vec<Vec<JsonSpan<'a>>>,
    /// Struct-variant named fields.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    fields: Vec<JsonVariantField<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    docs: Vec<JsonNode<'a>>,
}

impl<'a> JsonVariant<'a> {
    fn new(variant: &VariantDoc<'a>) -> Self {
        let (shape, tuple_fields, fields) = match &variant.shape {
            VariantShape::Plain => ("plain", Vec::new(), Vec::new()),
            VariantShape::Tuple { fields } => (
                "tuple",
                fields.iter().map(|spans| json_spans(spans)).collect(),
                Vec::new(),
            ),
            VariantShape::Struct { fields } => (
                "struct",
                Vec::new(),
                fields
                    .iter()
                    .map(|field| JsonVariantField {
                        name: field.name,
                        type_signature: json_spans(&field.type_spans),
                    })
                    .collect(),
            ),
        };

        Self {
            name: variant.name,
            shape,
            tuple_fields,
            fields,
            docs: variant.docs.as_deref().map(json_nodes).unwrap_or_default(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonVariantField<'a> {
    name: &'a str,
    #[serde(rename = "type")]
    type_signature: Vec<JsonSpan<'a>>,
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
    /// Inherent associated items, structurally modeled.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    methods: Vec<JsonMethod<'a>>,
    /// Trait implementations — still a faithful serialization of presentation
    /// nodes, until they get a structural model.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    trait_impls: Vec<JsonNode<'a>>,
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
            methods: model.methods.iter().map(JsonMethod::new).collect(),
            trait_impls: json_nodes(&model.trait_impls),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonMethod<'a> {
    name: &'a str,
    /// `"method"`, `"const"`, or `"type"`.
    kind: &'static str,
    /// `"public"`, `"crate"`, `"restricted"`, or `"default"`.
    visibility: &'static str,
    #[serde(skip_serializing_if = "is_false")]
    is_async: bool,
    #[serde(skip_serializing_if = "is_false")]
    is_const: bool,
    #[serde(skip_serializing_if = "is_false")]
    is_unsafe: bool,
    /// Return-type spans (functions with an explicit output).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    returns: Vec<JsonSpan<'a>>,
    /// Full display signature, the same spans the terminal renders.
    signature: Vec<JsonSpan<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    docs: Vec<JsonNode<'a>>,
}

impl<'a> JsonMethod<'a> {
    fn new(method: &MethodDoc<'a>) -> Self {
        Self {
            name: method.name,
            kind: assoc_kind_str(method.kind),
            visibility: match method.visibility {
                MethodVisibility::Public => "public",
                MethodVisibility::Crate => "crate",
                MethodVisibility::Restricted => "restricted",
                MethodVisibility::Default => "default",
            },
            is_async: method.is_async,
            is_const: method.is_const,
            is_unsafe: method.is_unsafe,
            returns: json_spans(&method.returns),
            signature: json_spans(&method.signature),
            docs: method.docs.as_deref().map(json_nodes).unwrap_or_default(),
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

fn is_false(value: &bool) -> bool {
    !*value
}

fn assoc_kind_str(kind: AssocKind) -> &'static str {
    match kind {
        AssocKind::Method => "method",
        AssocKind::Const => "const",
        AssocKind::Type => "type",
    }
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
