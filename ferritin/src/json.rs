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
//! Leaf references survive as `JsonSpan { text, style, path }` — the `path` is the
//! resolved item path (`trillium::Conn`), which is the pointer a client follows,
//! back into this same API. A span carries a `url` *instead* only when it names no
//! item (an external hyperlink in the prose); the viewed item's own upstream page
//! is served once, as `canonicalUrl`. Kinds not yet modeled fall back to a generic
//! serialization of their lowered presentation nodes.

use crate::{
    commands::{
        get::NotFoundDoc,
        list::ListDoc,
        search::{SearchDoc, SearchResult},
    },
    format::{
        AssocKind, ConstantDoc, EnumDoc, FunctionDoc, ImplAssocType, ImplementorDoc, ItemBody,
        ItemDoc, ItemMeta, MacroDoc, MetaVisibility, MethodDoc, MethodVisibility, ModuleDoc,
        ModuleItem, PlainField, StaticDoc, StructDoc, StructShape, TraitDoc, TraitImplDoc,
        TraitMember, TupleField, TypeAliasDoc, UnionDoc, VariantDoc, VariantShape,
    },
    styled_string::{
        Document, DocumentNode, HeadingLevel, ListItem, MetadataField, ShowWhen, Span, SpanStyle,
        TableCell, TruncationLevel,
    },
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

/// Serialize a not-found result to JSON (`{ error, query, suggestions }`).
pub(crate) fn not_found_to_string(not_found: &NotFoundDoc<'_>) -> sonic_rs::Result<String> {
    sonic_rs::to_string(&JsonNotFound::new(not_found))
}

/// Serialize the crate list to JSON (`{ crates: [...] }`).
pub(crate) fn list_to_string(list: &ListDoc) -> sonic_rs::Result<String> {
    sonic_rs::to_string(&JsonList::new(list))
}

/// Pretty-printed variant of [`list_to_string`], for snapshot tests.
#[cfg(test)]
pub(crate) fn list_to_pretty_string(list: &ListDoc) -> String {
    sonic_rs::to_string_pretty(&JsonList::new(list)).unwrap()
}

/// Serialize a search outcome to JSON.
pub(crate) fn search_to_string(doc: &SearchDoc<'_>) -> sonic_rs::Result<String> {
    sonic_rs::to_string(&JsonSearch::new(doc))
}

/// Serialize crate-name typeahead results to JSON.
#[cfg(feature = "serve")]
pub(crate) fn typeahead_to_string(
    query: &str,
    results: crate::typeahead::TypeaheadResults,
) -> sonic_rs::Result<String> {
    sonic_rs::to_string(&JsonTypeahead::new(query, results))
}

/// Pretty-printed variant of [`search_to_string`], for snapshot tests.
#[cfg(test)]
pub(crate) fn search_to_pretty_string(doc: &SearchDoc<'_>) -> String {
    sonic_rs::to_string_pretty(&JsonSearch::new(doc)).unwrap()
}

/// Pretty-printed variant of [`to_string`], for readable snapshot tests. Same
/// DTOs, so the field order matches the compact production output.
#[cfg(test)]
pub(crate) fn to_pretty_string(item: &ItemDoc<'_>, canonical_url: Option<String>) -> String {
    sonic_rs::to_string_pretty(&JsonItem::new(item, canonical_url)).unwrap()
}

/// Pretty-printed variant of [`not_found_to_string`], for snapshot tests.
#[cfg(test)]
pub(crate) fn not_found_to_pretty_string(not_found: &NotFoundDoc<'_>) -> String {
    sonic_rs::to_string_pretty(&JsonNotFound::new(not_found)).unwrap()
}

/// Crate-name typeahead results: the highest-ranked crates whose names start
/// with the query prefix, in rank order.
#[cfg(feature = "serve")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct JsonTypeahead {
    query: String,
    /// Exact number of crates matching the prefix; `results.len() < total`
    /// means the list was truncated to the requested limit.
    total: usize,
    results: Vec<JsonTypeaheadEntry>,
}

#[cfg(feature = "serve")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonTypeaheadEntry {
    name: String,
    /// The crate's crates.io default version (typically the latest stable
    /// non-yanked release), up to ~a day stale.
    version: String,
}

#[cfg(feature = "serve")]
impl JsonTypeahead {
    pub(crate) fn new(query: &str, results: crate::typeahead::TypeaheadResults) -> Self {
        Self {
            query: query.to_owned(),
            total: results.total,
            results: results
                .entries
                .into_iter()
                .map(|entry| JsonTypeaheadEntry {
                    name: entry.name,
                    version: entry.version,
                })
                .collect(),
        }
    }
}

/// A search outcome. `error` is set only for the no-crates-loaded case; an empty
/// query and a query with no matches both serialize as `{ query, results: [] }`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct JsonSearch<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    query: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    results: Vec<JsonSearchResult<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    suggestions: Vec<JsonSuggestion>,
}

impl<'a> JsonSearch<'a> {
    pub(crate) fn new(doc: &SearchDoc<'a>) -> Self {
        match doc {
            SearchDoc::Results { query, results } => Self {
                error: None,
                query: Some(query.clone()),
                results: results.iter().map(JsonSearchResult::new).collect(),
                suggestions: vec![],
            },
            SearchDoc::NoResults { query } => Self {
                error: None,
                query: Some(query.clone()),
                results: vec![],
                suggestions: vec![],
            },
            SearchDoc::EmptyQuery => Self {
                error: None,
                query: Some(String::new()),
                results: vec![],
                suggestions: vec![],
            },
            SearchDoc::NoCrates { suggestions } => Self {
                error: Some("noCratesLoaded"),
                query: None,
                results: vec![],
                suggestions: suggestions
                    .iter()
                    .map(|s| JsonSuggestion {
                        path: s.path.clone(),
                        kind: s.item.map(|i| format!("{:?}", i.kind()).to_lowercase()),
                        url: s.item.map(crate::docsrs_url::generate_docsrs_url),
                    })
                    .collect(),
            },
        }
    }
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonSearchResult<'a> {
    path: String,
    /// Lowercased item kind.
    kind: String,
    url: String,
    /// Normalized relevance score (best result = 100).
    score: f32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    docs: Vec<JsonNode<'a>>,
}

impl<'a> JsonSearchResult<'a> {
    fn new(result: &SearchResult<'a>) -> Self {
        Self {
            path: result.path.clone(),
            kind: format!("{:?}", result.item.kind()).to_lowercase(),
            url: crate::docsrs_url::generate_docsrs_url(result.item),
            score: result.score,
            docs: result.docs.as_deref().map(json_nodes).unwrap_or_default(),
        }
    }
}

/// A not-found result: the query and "did you mean" candidates. A JSON client
/// distinguishes it from a found item by the `error` discriminant.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct JsonNotFound {
    /// `"notFound"` for a path that didn't resolve, or `"crateUnavailable"`
    /// when the crate exists on crates.io but its docs couldn't be loaded.
    error: &'static str,
    query: String,
    /// The canonical crate name, set only for the `"crateUnavailable"` case.
    #[serde(skip_serializing_if = "Option::is_none")]
    unavailable_crate: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    suggestions: Vec<JsonSuggestion>,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonSuggestion {
    path: String,
    /// Lowercased kind of the resolved candidate, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
}

/// The crate list. Minimal projection of [`ListDoc`] (a soon-to-be-reworked
/// command).
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct JsonList {
    crates: Vec<JsonCrate>,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonCrate {
    name: String,
    version: String,
    #[serde(skip_serializing_if = "is_false")]
    is_default: bool,
    #[serde(skip_serializing_if = "is_false")]
    is_workspace: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    used_by: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
}

impl JsonList {
    fn new(list: &ListDoc) -> Self {
        Self {
            crates: list
                .crates
                .iter()
                .map(|c| JsonCrate {
                    name: c.name.clone(),
                    version: c.version.clone(),
                    is_default: c.is_default,
                    is_workspace: c.is_workspace,
                    used_by: c.used_by.clone(),
                    description: c.description.clone(),
                })
                .collect(),
        }
    }
}

impl JsonNotFound {
    pub(crate) fn new(not_found: &NotFoundDoc<'_>) -> Self {
        let error = if not_found.unavailable_crate.is_some() {
            "crateUnavailable"
        } else {
            "notFound"
        };
        Self {
            error,
            query: not_found.query.clone(),
            unavailable_crate: not_found.unavailable_crate.clone(),
            suggestions: not_found
                .suggestions
                .iter()
                .map(|s| JsonSuggestion {
                    path: s.path.clone(),
                    kind: s.item.map(|i| format!("{:?}", i.kind()).to_lowercase()),
                    url: s.item.map(crate::docsrs_url::generate_docsrs_url),
                })
                .collect(),
        }
    }
}

/// Pretty-printed variant of [`document_to_string`], for snapshot tests.
#[cfg(test)]
pub(crate) fn document_to_pretty_string(document: &Document<'_>) -> String {
    sonic_rs::to_string_pretty(&JsonDocument::new(document)).unwrap()
}

/// Generic JSON wrapper for a presentation [`Document`] — just its nodes.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Serialize)]
pub(crate) struct JsonDocument<'a> {
    nodes: Vec<JsonNode<'a>>,
}

impl<'a> JsonDocument<'a> {
    fn new(document: &Document<'a>) -> Self {
        Self {
            nodes: json_nodes(&document.nodes),
        }
    }
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct JsonItem<'a> {
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
    pub(crate) fn new(item: &ItemDoc<'a>, canonical_url: Option<String>) -> Self {
        Self {
            canonical_url,
            meta: JsonMeta::new(&item.meta),
            docs: json_nodes(&item.docs),
            body: JsonBody::new(&item.body),
            source: json_nodes(&item.source),
        }
    }
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
    Union(JsonUnion<'a>),
    /// A directly-queried trait associated item (type or const).
    AssocItem(JsonAssocItem<'a>),
    /// A kind not yet modeled structurally: its lowered presentation nodes.
    Presentation {
        nodes: Vec<JsonNode<'a>>,
    },
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
            ItemBody::Union(model) => JsonBody::Union(JsonUnion::new(model)),
            ItemBody::AssocItem(member) => JsonBody::AssocItem(JsonAssocItem::new(member)),
            ItemBody::Presentation(nodes) => JsonBody::Presentation {
                nodes: json_nodes(nodes),
            },
        }
    }
}

/// A type-alias body: the aliased type (RHS of `=`) as a span sequence.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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

/// A union body. Mirrors [`JsonStruct`] minus the `shape` (a union is always
/// named fields).
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonUnion<'a> {
    name: &'a str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    generics: Vec<JsonSpan<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    where_clause: Vec<JsonSpan<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    fields: Vec<JsonField<'a>>,
    #[serde(skip_serializing_if = "is_zero")]
    hidden_field_count: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    methods: Vec<JsonMethod<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    trait_impls: Vec<JsonTraitImpl<'a>>,
}

impl<'a> JsonUnion<'a> {
    fn new(model: &UnionDoc<'a>) -> Self {
        Self {
            name: model.name,
            generics: json_spans(&model.generics),
            where_clause: json_spans(&model.where_clause),
            fields: model.fields.iter().map(JsonField::from_plain).collect(),
            hidden_field_count: model.hidden_count,
            methods: model.methods.iter().map(JsonMethod::new).collect(),
            trait_impls: model.trait_impls.iter().map(JsonTraitImpl::new).collect(),
        }
    }
}

/// A free function body. Mirrors [`JsonMethod`] minus the assoc-item
/// `kind`/`visibility` and per-item `docs` — params live inside `signature`, so
/// a function and a method serialize the same way.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
            url: crate::docsrs_url::generate_docsrs_url(item.target),
            docs: item.docs.as_deref().map(json_nodes).unwrap_or_default(),
        }
    }
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
    /// Every implementor in the crate, sorted by type name — not the terminal's
    /// capped preview, so a client can show the whole list. Each carries impl
    /// detail only where the impl block or its methods have their own docs.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    implementors: Vec<JsonImplementor<'a>>,
}

impl<'a> JsonTrait<'a> {
    fn new(model: &TraitDoc<'a>) -> Self {
        Self {
            name: model.name,
            generics: json_spans(&model.generics),
            supertraits: json_spans(&model.supertraits),
            where_clause: json_spans(&model.where_clause),
            members: model.members.iter().map(JsonTraitMember::new).collect(),
            implementors: model
                .implementors
                .iter()
                .map(JsonImplementor::new)
                .collect(),
        }
    }
}

/// A trait implementor: the implementing type plus the impl's structured
/// metadata (richer than the terminal, which shows the type and assoc types).
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonImplementor<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    type_name: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    type_url: Option<String>,
    /// The implementing type, bounds merged inline (`BufReader<R: Read>`).
    for_type: Vec<JsonSpan<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    assoc_types: Vec<JsonImplAssocType<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    methods: Vec<JsonMethod<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    provided_methods: Vec<&'a str>,
    #[serde(skip_serializing_if = "is_false")]
    is_unsafe: bool,
    #[serde(skip_serializing_if = "is_false")]
    is_synthetic: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    blanket: Vec<JsonSpan<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    docs: Vec<JsonNode<'a>>,
}

impl<'a> JsonImplementor<'a> {
    fn new(model: &ImplementorDoc<'a>) -> Self {
        Self {
            type_name: model.type_name,
            type_url: model.type_url.clone(),
            for_type: json_spans(&model.for_type),
            assoc_types: model
                .assoc_types
                .iter()
                .map(JsonImplAssocType::new)
                .collect(),
            methods: model.methods.iter().map(JsonMethod::new).collect(),
            provided_methods: model.provided_methods.clone(),
            is_unsafe: model.is_unsafe,
            is_synthetic: model.is_synthetic,
            blanket: model.blanket.as_deref().map(json_spans).unwrap_or_default(),
            docs: model.docs.as_deref().map(json_nodes).unwrap_or_default(),
        }
    }
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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

/// A directly-queried trait associated item (type or const). Its own kind is
/// serialized as `assocKind` to avoid colliding with the `JsonBody` `kind` tag.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonAssocItem<'a> {
    /// `"type"` or `"const"`.
    assoc_kind: &'static str,
    name: &'a str,
    /// For an assoc type: whether it has a default; for an assoc const: whether
    /// it has a value.
    #[serde(skip_serializing_if = "is_false")]
    has_default: bool,
    signature: Vec<JsonSpan<'a>>,
}

impl<'a> JsonAssocItem<'a> {
    fn new(member: &TraitMember<'a>) -> Self {
        Self {
            assoc_kind: assoc_kind_str(member.kind),
            name: member.name,
            has_default: member.has_default,
            signature: json_spans(&member.signature),
        }
    }
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
    trait_impls: Vec<JsonTraitImpl<'a>>,
}

impl<'a> JsonEnum<'a> {
    fn new(model: &EnumDoc<'a>) -> Self {
        Self {
            name: model.name,
            generics: json_spans(&model.generics),
            where_clause: json_spans(&model.where_clause),
            variants: model.variants.iter().map(JsonVariant::new).collect(),
            methods: model.methods.iter().map(JsonMethod::new).collect(),
            trait_impls: model.trait_impls.iter().map(JsonTraitImpl::new).collect(),
        }
    }
}

/// A trait implementation on a type. Carries the full structural impl —
/// including data the terminal drops (the impl's `methods`, `providedMethods`,
/// the negative/unsafe/synthetic flags, the blanket source type, and impl
/// `docs`). The compact/std bucketing is a terminal concern and not serialized.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonTraitImpl<'a> {
    trait_name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    trait_url: Option<String>,
    /// The trait's generic arguments (`<T>` in `From<T>`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    args: Vec<JsonSpan<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    assoc_types: Vec<JsonImplAssocType<'a>>,
    /// Methods / assoc consts the impl provides or overrides.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    methods: Vec<JsonMethod<'a>>,
    /// Names of trait-default methods inherited (not overridden).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    provided_methods: Vec<&'a str>,
    #[serde(skip_serializing_if = "is_false")]
    is_negative: bool,
    #[serde(skip_serializing_if = "is_false")]
    is_unsafe: bool,
    #[serde(skip_serializing_if = "is_false")]
    is_synthetic: bool,
    #[serde(skip_serializing_if = "is_false")]
    is_std: bool,
    /// Blanket source type, when this came from a blanket impl.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    blanket: Vec<JsonSpan<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    docs: Vec<JsonNode<'a>>,
}

impl<'a> JsonTraitImpl<'a> {
    fn new(model: &TraitImplDoc<'a>) -> Self {
        Self {
            trait_name: model.trait_name,
            trait_url: model.trait_url.clone(),
            args: json_spans(&model.trait_args),
            assoc_types: model
                .assoc_types
                .iter()
                .map(JsonImplAssocType::new)
                .collect(),
            methods: model.methods.iter().map(JsonMethod::new).collect(),
            provided_methods: model.provided_methods.clone(),
            is_negative: model.is_negative,
            is_unsafe: model.is_unsafe,
            is_synthetic: model.is_synthetic,
            is_std: model.is_std,
            blanket: model.blanket.as_deref().map(json_spans).unwrap_or_default(),
            docs: model.docs.as_deref().map(json_nodes).unwrap_or_default(),
        }
    }
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonImplAssocType<'a> {
    name: &'a str,
    #[serde(rename = "type")]
    type_signature: Vec<JsonSpan<'a>>,
}

impl<'a> JsonImplAssocType<'a> {
    fn new(assoc: &ImplAssocType<'a>) -> Self {
        Self {
            name: assoc.name,
            type_signature: json_spans(&assoc.type_spans),
        }
    }
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonVariantField<'a> {
    name: &'a str,
    #[serde(rename = "type")]
    type_signature: Vec<JsonSpan<'a>>,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
    /// Trait implementations, structurally modeled.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    trait_impls: Vec<JsonTraitImpl<'a>>,
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
            trait_impls: model.trait_impls.iter().map(JsonTraitImpl::new).collect(),
        }
    }
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
        /// Syntax-highlighted spans that tile the source: concatenating their
        /// `text` reconstructs the raw code (there is no separate `code` field).
        /// A span without a `class` is unstyled text (punctuation, plain
        /// identifiers, or a block whose language has no grammar).
        spans: Vec<JsonCodeSpan<'a>>,
        /// Doctest attributes worth surfacing to a reader — `should_panic` and
        /// `compile_fail`, positive assertions that the example is a
        /// counterexample. Omitted when empty.
        #[serde(skip_serializing_if = "Vec::is_empty")]
        attrs: Vec<Cow<'a, str>>,
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
        /// The retained nodes *after* applying `level` — a preview
        /// (`SingleLine`/`Brief`) keeps only the first block, `Full` keeps all.
        /// Truncation happens here rather than on the client so the wire payload
        /// carries the summary, not the whole essay it summarizes.
        nodes: Vec<JsonNode<'a>>,
        level: TruncationLevel,
        /// `true` when nodes were dropped — the client's cue that a fuller body
        /// exists behind the item's navigation target.
        #[serde(skip_serializing_if = "is_false")]
        truncated: bool,
    },
    Conditional {
        show_when: ShowWhen,
        nodes: Vec<JsonNode<'a>>,
    },
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonMetadataField<'a> {
    label: Cow<'a, str>,
    value: Vec<JsonSpan<'a>>,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Serialize)]
struct JsonListItem<'a> {
    content: Vec<JsonNode<'a>>,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Serialize)]
struct JsonTableCell<'a> {
    spans: Vec<JsonSpan<'a>>,
}

/// A span in a syntax-highlighted code block: a slice of source and its lexical
/// class (`keyword`, `type`, `string`, …). Distinct from [`JsonSpan`], whose
/// `style` is a semantic, navigable [`SpanStyle`]; a code-block class is a purely
/// lexical highlight the client colors, with no navigation.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonCodeSpan<'a> {
    text: Cow<'a, str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    class: Option<Cow<'a, str>>,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonSpan<'a> {
    text: Cow<'a, str>,
    style: SpanStyle,
    /// A link we cannot express as an item path: an external hyperlink written in
    /// the docs (a blog post, an RFC) or a link that resolved to no item. Mutually
    /// exclusive with `path` — where a path exists it *is* the navigation target,
    /// so no upstream URL is emitted beside it (see [`json_span`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<Cow<'a, str>>,
    /// In-app navigation target: a `::`-joined item path (e.g. `trillium::Conn`)
    /// the client routes to. Present whenever the target resolves to an item with
    /// its own page; absent for associated items, variants, and bare external URLs.
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<Cow<'a, str>>,
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
    let path = span.nav_path();
    JsonSpan {
        text: span.text.clone(),
        style: span.style,
        // A `path` already names the item, and the client routes on it — so an
        // upstream `url` beside it is redundant, and an expensive redundancy: the
        // two were ~36% of a large item's payload, nearly all of it absolute
        // docs.rs URLs the client never followed. Keep the `url` only where there
        // is no path to derive one from — genuine external hyperlinks in prose
        // (a blog post, an RFC) and links we could not resolve to an item. The
        // item's own upstream page is still served once, as `canonicalUrl`.
        url: if path.is_some() { None } else { span.url() },
        path,
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
        DocumentNode::CodeBlock { lang, code, attrs } => JsonNode::CodeBlock {
            lang: lang.clone(),
            spans: crate::highlight::highlight(lang.as_deref(), code.as_ref())
                .into_iter()
                .map(|span| JsonCodeSpan {
                    text: Cow::Owned(span.text.to_owned()),
                    class: span.class.map(Cow::Borrowed),
                })
                .collect(),
            attrs: attrs.clone(),
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
        DocumentNode::TruncatedBlock { nodes, level } => {
            // A preview level keeps only the first block; the terminal renderers
            // apply the same "first node" rule at render time (see
            // `renderer::plain`). `Full` is the item's own docs — kept whole.
            let retained: &[DocumentNode] = match level {
                TruncationLevel::Full => nodes,
                TruncationLevel::SingleLine | TruncationLevel::Brief => {
                    &nodes[..nodes.len().min(1)]
                }
            };
            JsonNode::TruncatedBlock {
                nodes: json_nodes(retained),
                level: *level,
                truncated: retained.len() < nodes.len(),
            }
        }
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
