use crate::request::Request;
use crate::styled_string::{DocumentNode, Span as StyledSpan, TruncationLevel};
use ferritin_common::doc_ref::DocRef;
use rustdoc_types::{
    Abi, Constant, Enum, Function, FunctionPointer, GenericArg, GenericArgs, GenericBound,
    GenericParamDef, GenericParamDefKind, Generics, Id, Item, ItemEnum, ItemSummary, Path, Span,
    Static, Struct, StructKind, Term, Trait, Type, TypeAlias, Union, VariantKind, Visibility,
    WherePredicate,
};
use std::{collections::HashMap, fs};

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

impl Request {
    /// Format an item with automatic recursion tracking
    pub(crate) fn format_item<'a>(&'a self, item: DocRef<'a, Item>) -> Vec<DocumentNode<'a>> {
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

    /// Format item metadata as a compact paragraph (Item, Kind, Visibility, Location, Crate)
    fn format_item_metadata<'a>(&'a self, item: DocRef<'a, Item>) -> Vec<DocumentNode<'a>> {
        let mut spans = vec![];

        // Item name
        spans.push(StyledSpan::strong("Item:"));
        spans.push(StyledSpan::plain(" "));
        spans.push(StyledSpan::plain(item.name().unwrap_or("unnamed")));
        spans.push(StyledSpan::plain("\n"));

        // Kind
        spans.push(StyledSpan::strong("Kind:"));
        spans.push(StyledSpan::plain(" "));
        spans.push(StyledSpan::plain(format!("{:?}", item.kind())));
        spans.push(StyledSpan::plain("\n"));

        // Visibility
        spans.push(StyledSpan::strong("Visibility:"));
        spans.push(StyledSpan::plain(" "));
        match &item.item().visibility {
            Visibility::Public => spans.push(StyledSpan::plain("Public")),
            Visibility::Default => spans.push(StyledSpan::plain("Private")),
            Visibility::Crate => spans.push(StyledSpan::plain("Crate")),
            Visibility::Restricted { parent, path } => {
                spans.push(StyledSpan::plain("Restricted to "));
                if let Some(parent_summary) = item.get(parent).and_then(|item| item.summary()) {
                    let mut action_item = None;
                    for (i, segment) in parent_summary.path.iter().enumerate() {
                        if i == 0 {
                            action_item = item
                                .crate_docs()
                                .traverse_to_crate_by_id(self, parent_summary.crate_id)
                                .map(|x| x.root_item(self));
                        } else {
                            spans.push(StyledSpan::punctuation("::"));
                            if let Some(ai) = action_item {
                                action_item = ai.find_child(segment);
                            }
                        }
                        spans.push(StyledSpan::type_name(segment).with_target(action_item));
                    }
                } else {
                    spans.push(StyledSpan::plain(path));
                }
            }
        }
        spans.push(StyledSpan::plain("\n"));

        // Location and Crate (from item_summary if available)
        if let Some(item_summary) = item.summary() {
            // Defined at
            spans.push(StyledSpan::strong("Defined at:"));
            spans.push(StyledSpan::plain(" "));

            let mut action_item = None;
            for (i, segment) in item_summary.path.iter().enumerate() {
                if i == 0 {
                    action_item = item
                        .crate_docs()
                        .traverse_to_crate_by_id(self, item_summary.crate_id)
                        .map(|x| x.root_item(self));
                } else {
                    spans.push(StyledSpan::punctuation("::"));
                    if let Some(ai) = action_item {
                        action_item = ai.find_child(segment);
                    }
                }
                spans.push(StyledSpan::type_name(segment).with_target(action_item));
            }
            spans.push(StyledSpan::plain("\n"));

            // In crate
            spans.push(StyledSpan::strong("In crate:"));
            spans.push(StyledSpan::plain(" "));

            let item_crate = item.crate_docs();
            spans.push(StyledSpan::plain(item_crate.name()));
            if let Some(version) = item_crate.crate_version.as_deref() {
                spans.push(StyledSpan::plain(" ("));
                spans.push(StyledSpan::plain(version));
                spans.push(StyledSpan::plain(")"));
            }
        }

        vec![DocumentNode::paragraph(spans)]
    }

    /// Returns (defined_at_nodes, crate_info_nodes) with label prefixes
    fn format_item_summary<'a>(
        &'a self,
        item: DocRef<'a, Item>,
        item_summary: &'a ItemSummary,
    ) -> (Vec<DocumentNode<'a>>, Vec<DocumentNode<'a>>) {
        let mut defined_at_spans = vec![StyledSpan::strong("Defined at:"), StyledSpan::plain(" ")];
        let mut action_item = None;
        let mut source_crate = None;
        let item_crate = item.crate_docs();

        // Build "Defined at" path
        for (i, segment) in item_summary.path.iter().enumerate() {
            if i == 0 {
                action_item = item
                    .crate_docs()
                    .traverse_to_crate_by_id(self, item_summary.crate_id)
                    .map(|x| x.root_item(self));
                source_crate = action_item.map(|i| i.crate_docs());
            } else {
                defined_at_spans.push(StyledSpan::punctuation("::"));
                if let Some(ai) = action_item {
                    action_item = ai.find_child(segment);
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
            crate_info_spans.push(StyledSpan::plain(version));
            crate_info_spans.push(StyledSpan::plain(")"));
        }

        (
            vec![DocumentNode::paragraph(defined_at_spans)],
            vec![DocumentNode::paragraph(crate_info_spans)],
        )
    }

    /// Format visibility value with label
    fn format_visibility_value<'a>(&'a self, item: DocRef<'a, Item>) -> Vec<DocumentNode<'a>> {
        let mut spans = vec![StyledSpan::strong("Visibility:"), StyledSpan::plain(" ")];

        match &item.item().visibility {
            Visibility::Public => spans.push(StyledSpan::plain("Public")),
            Visibility::Default => spans.push(StyledSpan::plain("Private")),
            Visibility::Crate => spans.push(StyledSpan::plain("Crate")),
            Visibility::Restricted { parent, path } => {
                spans.push(StyledSpan::plain("Restricted to "));
                if let Some(parent_summary) = item.get(parent).and_then(|item| item.summary()) {
                    let mut action_item = None;
                    for (i, segment) in parent_summary.path.iter().enumerate() {
                        if i == 0 {
                            action_item = item
                                .crate_docs()
                                .traverse_to_crate_by_id(self, parent_summary.crate_id)
                                .map(|x| x.root_item(self));
                        } else {
                            spans.push(StyledSpan::punctuation("::"));
                            if let Some(ai) = action_item {
                                action_item = ai.find_child(segment);
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
