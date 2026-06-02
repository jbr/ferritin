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

    /// Format an item with automatic recursion tracking
    pub(crate) fn format_item(&mut self, item: DocRef<'a, Item>) -> Vec<DocumentNode<'a>> {
        let mut doc_nodes = vec![];

        // Item metadata (name, kind, visibility, location, crate)
        doc_nodes.extend(self.format_item_metadata(item));

        // Add documentation if available
        if let Some(docs) = self.docs_to_show(item, TruncationLevel::Full) {
            doc_nodes.extend(docs);
        };

        // Handle different item types
        match item.inner() {
            ItemEnum::Module(_) => {
                doc_nodes.extend(self.format_module(item));
            }
            ItemEnum::Struct(struct_data) => {
                doc_nodes.extend(self.format_struct(item, item.build_ref(struct_data)));
            }
            ItemEnum::Enum(enum_data) => {
                doc_nodes.extend(self.format_enum(item, item.build_ref(enum_data)));
            }
            ItemEnum::Trait(trait_data) => {
                doc_nodes.extend(self.format_trait(item, item.build_ref(trait_data)));
            }
            ItemEnum::Function(function_data) => {
                doc_nodes.extend(self.format_function(item, item.build_ref(function_data)));
            }
            ItemEnum::TypeAlias(type_alias_data) => {
                doc_nodes.extend(self.format_type_alias(item, item.build_ref(type_alias_data)));
            }
            ItemEnum::Union(union_data) => {
                doc_nodes.extend(self.format_union(item, item.build_ref(union_data)));
            }
            ItemEnum::Constant { type_, const_ } => {
                doc_nodes.extend(self.format_constant(item, type_, const_));
            }
            ItemEnum::Static(static_data) => {
                doc_nodes.extend(self.format_static(item, static_data));
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
                doc_nodes.push(DocumentNode::generated_code(sig));
            }
            ItemEnum::AssocConst { type_, value } => {
                let name = item.name().unwrap_or("<unnamed>");
                let sig = self.format_trait_assoc_const_signature(item, type_, value, name);
                doc_nodes.push(DocumentNode::generated_code(sig));
            }
            ItemEnum::Macro(macro_def) => {
                doc_nodes.push(DocumentNode::paragraph(vec![StyledSpan::plain(
                    "Macro definition:",
                )]));
                doc_nodes.push(DocumentNode::code_block(Some("rust"), macro_def));
            }
            _ => {
                // For any other item, just print its name and kind
                doc_nodes.push(DocumentNode::paragraph(vec![
                    StyledSpan::plain(format!("{:?}", item.kind())),
                    StyledSpan::plain(" "),
                    StyledSpan::plain(item.name().unwrap_or("<unnamed>")),
                ]));
            }
        }

        // Add source code if requested
        if self.format_context().include_source()
            && let Some(span) = &item.span
        {
            doc_nodes.extend(source::format_source_code(self, span));
        }

        doc_nodes
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
            if let Some(version) = item_crate.crate_version.as_deref() {
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
            && source_crate != item_crate
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
        if let Some(version) = item_crate.crate_version.as_deref() {
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
