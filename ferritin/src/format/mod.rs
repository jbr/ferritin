use crate::request::Request;
use crate::styled_string::{DocumentNode, MetadataField, Span as StyledSpan, TruncationLevel};
use ferritin_common::doc_ref::DocRef;
use rustdoc_types::{
    Abi, Constant, Enum, Function, FunctionPointer, GenericArg, GenericArgs, GenericBound,
    GenericParamDef, GenericParamDefKind, Generics, Id, Item, ItemEnum, ItemKind, ItemSummary,
    Path, Span, Static, Struct, StructKind, Term, Trait, Type, TypeAlias, Union, VariantKind,
    Visibility, WherePredicate,
};
use std::{collections::HashMap, fs};

/// Display name for a [`Path`], stripping the rustdoc-leaked `$crate::` prefix
/// that appears in derive-generated impls. Returns the trailing segment in that
/// case (e.g. `$crate::cmp::Eq` → `Eq`), matching what hand-written bounds and
/// rustdoc's HTML output show. Falls back to the original string otherwise.
pub(crate) fn display_path_name(path: &Path) -> &str {
    if let Some(rest) = path.path.strip_prefix("$crate::") {
        rest.rsplit("::").next().unwrap_or(rest)
    } else {
        &path.path
    }
}

mod documentation;
mod r#enum;
mod functions;
mod impls;
mod items;
mod r#module;
mod source;
mod r#struct;
mod r#trait;
mod types;

pub(crate) use r#struct::{PlainField, StructDoc, StructShape, TupleField};

/// Semantic model of a documented item: a kind-agnostic header (metadata block
/// + the item's own doc prose) and trailing source code, wrapped around a
/// kind-specific [`ItemBody`]. Built by [`Request::model_item`]; the terminal
/// renderers go through [`ItemDoc::lower`], while the JSON output serializes it
/// structurally.
pub(crate) struct ItemDoc<'a> {
    /// Metadata node + own-doc prose. Already structured enough (the metadata
    /// node carries labeled fields; docs are a markdown sub-IR), so only the
    /// body gets the semantic treatment for now.
    pub(crate) header: Vec<DocumentNode<'a>>,
    pub(crate) body: ItemBody<'a>,
    /// Source code block, when `--source` is requested; otherwise empty.
    pub(crate) source: Vec<DocumentNode<'a>>,
}

/// The kind-specific body of an item. Migrated kinds carry a structural model;
/// the rest fall back to already-lowered presentation nodes. This is the seam
/// that lets the domain-IR migration proceed one kind at a time.
pub(crate) enum ItemBody<'a> {
    Struct(r#struct::StructDoc<'a>),
    Presentation(Vec<DocumentNode<'a>>),
}

impl<'a> ItemBody<'a> {
    fn lower(self) -> Vec<DocumentNode<'a>> {
        match self {
            ItemBody::Struct(model) => r#struct::lower_struct(model),
            ItemBody::Presentation(nodes) => nodes,
        }
    }
}

impl<'a> ItemDoc<'a> {
    /// Lower to presentation nodes, reproducing the old `format_item` output
    /// (header, then body, then source).
    pub(crate) fn lower(self) -> Vec<DocumentNode<'a>> {
        let ItemDoc {
            mut header,
            body,
            source,
        } = self;
        header.extend(body.lower());
        header.extend(source);
        header
    }
}

impl<'a> Request<'a> {
    /// Whether `item` should be omitted because `--public` is active and
    /// the item is not `pub`.
    ///
    /// Uses [`DocRef::effective_visibility`], so a `pub use` of a non-`pub` item
    /// is correctly treated as public (and a non-`pub` `use` as hidden). Glob
    /// re-exports are best-effort — see `collect_use_children`.
    pub(super) fn hidden_by_visibility(&self, item: DocRef<'a, Item>) -> bool {
        if !self.format_context().public() {
            return false;
        }

        // Enum variants always carry `Visibility::Default` in rustdoc JSON but
        // are exactly as visible as their enum, so never hide them by their own
        // visibility — they only reach module listings via `pub use Enum::*`,
        // where hiding them would drop genuinely public API.
        if item.kind() == ItemKind::Variant {
            return false;
        }

        !matches!(item.effective_visibility(), Visibility::Public)
    }

    /// Format an item to presentation nodes by building its [`ItemDoc`] model
    /// and lowering it. The model flows through the same seam the JSON output
    /// uses, so terminal output stays byte-identical (snapshots are the guard).
    pub(crate) fn format_item(&mut self, item: DocRef<'a, Item>) -> Vec<DocumentNode<'a>> {
        self.model_item(item).lower()
    }

    /// Resolve an item into its semantic [`ItemDoc`] model.
    pub(crate) fn model_item(&mut self, item: DocRef<'a, Item>) -> ItemDoc<'a> {
        // Item metadata (name, kind, visibility, location, crate).
        let mut header = self.format_item_metadata(item);

        // The item's own documentation, at the level set by `--docs` (omitted
        // entirely when `--docs none`).
        if let Some(truncation) = self.format_context().doc_truncation()
            && let Some(docs) = self.docs_to_show(item, truncation)
        {
            header.extend(docs);
        }

        let body = self.model_item_body(item);

        let source = if self.format_context().include_source()
            && let Some(span) = &item.span
        {
            source::format_source_code(self, span)
        } else {
            vec![]
        };

        ItemDoc {
            header,
            body,
            source,
        }
    }

    /// Build the kind-specific [`ItemBody`]. Only `struct` is modeled
    /// structurally so far; every other kind lowers eagerly into
    /// [`ItemBody::Presentation`].
    fn model_item_body(&mut self, item: DocRef<'a, Item>) -> ItemBody<'a> {
        match item.inner() {
            ItemEnum::Struct(struct_data) => {
                ItemBody::Struct(self.model_struct(item, item.build_ref(struct_data)))
            }
            ItemEnum::Module(_) => ItemBody::Presentation(self.format_module(item)),
            ItemEnum::Enum(enum_data) => {
                ItemBody::Presentation(self.format_enum(item, item.build_ref(enum_data)))
            }
            ItemEnum::Trait(trait_data) => {
                ItemBody::Presentation(self.format_trait(item, item.build_ref(trait_data)))
            }
            ItemEnum::Function(function_data) => {
                ItemBody::Presentation(self.format_function(item, item.build_ref(function_data)))
            }
            ItemEnum::TypeAlias(type_alias_data) => {
                ItemBody::Presentation(self.format_type_alias(item, item.build_ref(type_alias_data)))
            }
            ItemEnum::Union(union_data) => {
                ItemBody::Presentation(self.format_union(item, item.build_ref(union_data)))
            }
            ItemEnum::Constant { type_, const_ } => {
                ItemBody::Presentation(self.format_constant(item, type_, const_))
            }
            ItemEnum::Static(static_data) => {
                ItemBody::Presentation(self.format_static(item, static_data))
            }
            ItemEnum::AssocType {
                generics,
                bounds,
                type_,
            } => {
                let name = item.name().unwrap_or("<unnamed>");
                let sig = self.format_trait_assoc_type_signature(
                    item,
                    generics,
                    bounds,
                    type_.as_ref(),
                    name,
                );
                ItemBody::Presentation(vec![DocumentNode::generated_code(sig)])
            }
            ItemEnum::AssocConst { type_, value } => {
                let name = item.name().unwrap_or("<unnamed>");
                let sig = self.format_trait_assoc_const_signature(item, type_, value, name);
                ItemBody::Presentation(vec![DocumentNode::generated_code(sig)])
            }
            ItemEnum::Macro(macro_def) => ItemBody::Presentation(vec![
                DocumentNode::paragraph(vec![StyledSpan::plain("Macro definition:")]),
                DocumentNode::code_block(Some("rust"), macro_def),
            ]),
            _ => {
                // For any other item, just print its name and kind.
                ItemBody::Presentation(vec![DocumentNode::paragraph(vec![
                    StyledSpan::plain(format!("{:?}", item.kind())),
                    StyledSpan::plain(" "),
                    StyledSpan::plain(item.name().unwrap_or("<unnamed>")),
                ])])
            }
        }
    }

    /// Format item metadata as a structured `Metadata` node so each renderer
    /// can pick its own layout. Plain/TTY/Interactive show labeled lines;
    /// the AI renderer collapses to a single-line summary.
    fn format_item_metadata(&mut self, item: DocRef<'a, Item>) -> Vec<DocumentNode<'a>> {
        let mut fields: Vec<MetadataField<'a>> = vec![];

        // Item
        fields.push(MetadataField::new(
            "Item",
            vec![StyledSpan::plain(item.name().unwrap_or("unnamed"))],
        ));

        // Kind
        fields.push(MetadataField::new(
            "Kind",
            vec![StyledSpan::plain(format!("{:?}", item.kind()))],
        ));

        // Visibility
        let mut vis_spans = vec![];
        match &item.item().visibility {
            Visibility::Public => vis_spans.push(StyledSpan::plain("Public")),
            Visibility::Default => vis_spans.push(StyledSpan::plain("Private")),
            Visibility::Crate => vis_spans.push(StyledSpan::plain("Crate")),
            Visibility::Restricted { parent, path } => {
                vis_spans.push(StyledSpan::plain("Restricted to "));
                if let Some(parent_summary) = item.get(parent).and_then(|item| item.summary()) {
                    let mut action_item = None;
                    let nav = self.navigator();
                    for (i, segment) in parent_summary.path.iter().enumerate() {
                        if i == 0 {
                            action_item = item
                                .crate_docs()
                                .traverse_to_crate_by_id(nav, parent_summary.crate_id)
                                .map(|x| x.root_item(nav));
                        } else {
                            vis_spans.push(StyledSpan::punctuation("::"));
                            if let Some(ai) = action_item {
                                action_item = self.find_child(ai, segment);
                            }
                        }
                        vis_spans.push(StyledSpan::type_name(segment).with_target(action_item));
                    }
                } else {
                    vis_spans.push(StyledSpan::plain(path));
                }
            }
        }
        fields.push(MetadataField::new("Visibility", vis_spans));

        if let Some(item_summary) = item.summary() {
            // Defined at
            let mut path_spans = vec![];
            let nav = self.navigator();
            let mut action_item = None;
            for (i, segment) in item_summary.path.iter().enumerate() {
                if i == 0 {
                    action_item = item
                        .crate_docs()
                        .traverse_to_crate_by_id(nav, item_summary.crate_id)
                        .map(|x| x.root_item(nav));
                } else {
                    path_spans.push(StyledSpan::punctuation("::"));
                    if let Some(ai) = action_item {
                        action_item = self.find_child(ai, segment);
                    }
                }
                path_spans.push(StyledSpan::type_name(segment).with_target(action_item));
            }
            fields.push(MetadataField::new("Defined at", path_spans));

            // In crate
            let mut crate_spans = vec![];
            let item_crate = item.crate_docs();
            crate_spans.push(StyledSpan::plain(item_crate.name()));
            if let Some(version) = item_crate.crate_version() {
                crate_spans.push(StyledSpan::plain(" ("));
                let version_normalized = version.replace('\t', " ");
                crate_spans.push(StyledSpan::plain(version_normalized));
                crate_spans.push(StyledSpan::plain(")"));
            }
            fields.push(MetadataField::new("In crate", crate_spans));
        }

        vec![DocumentNode::metadata(fields)]
    }

    /// Returns (defined_at_nodes, crate_info_nodes) with label prefixes
    fn format_item_summary(
        &mut self,
        item: DocRef<'a, Item>,
        item_summary: &'a ItemSummary,
    ) -> (Vec<DocumentNode<'a>>, Vec<DocumentNode<'a>>) {
        let mut defined_at_spans = vec![StyledSpan::strong("Defined at:"), StyledSpan::plain(" ")];
        let mut action_item = None;
        let mut source_crate = None;
        let item_crate = item.crate_docs();

        // Build "Defined at" path
        let nav = self.navigator();
        for (i, segment) in item_summary.path.iter().enumerate() {
            if i == 0 {
                action_item = item
                    .crate_docs()
                    .traverse_to_crate_by_id(nav, item_summary.crate_id)
                    .map(|x| x.root_item(nav));
                source_crate = action_item.map(|i| i.crate_docs());
            } else {
                defined_at_spans.push(StyledSpan::punctuation("::"));
                if let Some(ai) = action_item {
                    action_item = self.find_child(ai, segment);
                }
            }

            defined_at_spans.push(StyledSpan::type_name(segment).with_target(action_item));
        }

        // Add version if re-exported from different crate
        if let Some(source_crate) = source_crate
            && !std::ptr::eq(source_crate, item_crate)
            && let Some(version) = source_crate.version()
        {
            defined_at_spans.push(StyledSpan::plain(" ("));
            defined_at_spans.push(StyledSpan::plain(version.to_string()));
            defined_at_spans.push(StyledSpan::plain(" )"));
        }

        // Build "In crate" info
        let mut crate_info_spans = vec![
            StyledSpan::strong("In crate:"),
            StyledSpan::plain(" "),
            StyledSpan::plain(item_crate.name()),
        ];
        if let Some(version) = item_crate.crate_version() {
            crate_info_spans.push(StyledSpan::plain(" ("));
            // Replace tabs with spaces for consistent rendering across output modes
            let version_normalized = version.replace('\t', " ");
            crate_info_spans.push(StyledSpan::plain(version_normalized));
            crate_info_spans.push(StyledSpan::plain(")"));
        }

        (
            vec![DocumentNode::paragraph(defined_at_spans)],
            vec![DocumentNode::paragraph(crate_info_spans)],
        )
    }

    /// Format visibility value with label
    fn format_visibility_value(&mut self, item: DocRef<'a, Item>) -> Vec<DocumentNode<'a>> {
        let mut spans = vec![StyledSpan::strong("Visibility:"), StyledSpan::plain(" ")];

        match &item.item().visibility {
            Visibility::Public => spans.push(StyledSpan::plain("Public")),
            Visibility::Default => spans.push(StyledSpan::plain("Private")),
            Visibility::Crate => spans.push(StyledSpan::plain("Crate")),
            Visibility::Restricted { parent, path } => {
                spans.push(StyledSpan::plain("Restricted to "));
                if let Some(parent_summary) = item.get(parent).and_then(|item| item.summary()) {
                    let mut action_item = None;
                    let nav = self.navigator();
                    for (i, segment) in parent_summary.path.iter().enumerate() {
                        if i == 0 {
                            action_item = item
                                .crate_docs()
                                .traverse_to_crate_by_id(nav, parent_summary.crate_id)
                                .map(|x| x.root_item(nav));
                        } else {
                            spans.push(StyledSpan::punctuation("::"));
                            if let Some(ai) = action_item {
                                action_item = self.find_child(ai, segment);
                            }
                        }

                        spans.push(StyledSpan::type_name(segment).with_target(action_item));
                    }
                } else {
                    spans.push(StyledSpan::plain(path));
                }
            }
        }

        vec![DocumentNode::paragraph(spans)]
    }
}
