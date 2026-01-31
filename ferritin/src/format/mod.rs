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
        doc_nodes.push(DocumentNode::Span(StyledSpan::plain("  ")));

        // Basic item information
        doc_nodes.push(DocumentNode::Span(StyledSpan::plain(format!(
            "Item: {}\n",
            item.name().unwrap_or("unnamed")
        ))));
        doc_nodes.push(DocumentNode::Span(StyledSpan::plain(format!(
            "Kind: {:?}\n",
            item.kind()
        ))));
        doc_nodes.extend(self.format_visibility(item));

        if let Some(item_summary) = item.summary() {
            doc_nodes.extend(self.format_item_summary(item, item_summary));
        }

        // Add documentation if available
        if let Some(docs) = self.docs_to_show(item, TruncationLevel::Full) {
            doc_nodes.push(DocumentNode::Span(StyledSpan::plain("\n")));
            doc_nodes.extend(docs);
            doc_nodes.push(DocumentNode::Span(StyledSpan::plain("\n")));
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
                doc_nodes.push(DocumentNode::Span(StyledSpan::plain("\n")));
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
                doc_nodes.push(DocumentNode::Span(StyledSpan::plain(
                    "Macro definition:\n\n",
                )));
                doc_nodes.push(DocumentNode::code_block(Some("rust"), macro_def));
            }
            _ => {
                // For any other item, just print its name and kind
                doc_nodes.push(DocumentNode::Span(StyledSpan::plain("\n")));
                doc_nodes.push(DocumentNode::Span(StyledSpan::plain(format!(
                    "{:?}",
                    item.kind()
                ))));
                doc_nodes.push(DocumentNode::Span(StyledSpan::plain(" ")));
                doc_nodes.push(DocumentNode::Span(StyledSpan::plain(
                    item.name().unwrap_or("<unnamed>"),
                )));
                doc_nodes.push(DocumentNode::Span(StyledSpan::plain("\n")));
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

    fn format_item_summary<'a>(
        &'a self,
        item: DocRef<'a, Item>,
        item_summary: &'a ItemSummary,
    ) -> Vec<DocumentNode<'a>> {
        let mut nodes = vec![DocumentNode::Span(StyledSpan::plain("Defined at: "))];
        let mut action_item = None;
        let mut source_crate = None;
        let item_crate = item.crate_docs();

        for (i, segment) in item_summary.path.iter().enumerate() {
            if i == 0 {
                action_item = item
                    .crate_docs()
                    .traverse_to_crate_by_id(self, item_summary.crate_id)
                    .map(|x| x.root_item(self));
                source_crate = action_item.map(|i| i.crate_docs());
            } else {
                nodes.push(DocumentNode::Span(StyledSpan::punctuation("::")));
                if let Some(ai) = action_item {
                    action_item = ai.find_child(segment);
                }
            }

            nodes.push(DocumentNode::Span(
                StyledSpan::type_name(segment).with_target(action_item),
            ));
        }

        if let Some(source_crate) = source_crate
            && source_crate != item_crate
            && let Some(version) = source_crate.version()
        {
            nodes.push(DocumentNode::Span(StyledSpan::plain(" (")));
            nodes.push(DocumentNode::Span(StyledSpan::plain(version.to_string())));
            nodes.push(DocumentNode::Span(StyledSpan::plain(" )")));
        }

        nodes.push(DocumentNode::Span(StyledSpan::plain("\n")));

        nodes.push(DocumentNode::Span(StyledSpan::plain("In crate: ")));
        nodes.push(DocumentNode::Span(StyledSpan::plain(item_crate.name())));
        if let Some(version) = item_crate.crate_version.as_deref() {
            nodes.push(DocumentNode::Span(StyledSpan::plain(" (")));
            nodes.push(DocumentNode::Span(StyledSpan::plain(version)));
            nodes.push(DocumentNode::Span(StyledSpan::plain(")")));
        }
        nodes.push(DocumentNode::Span(StyledSpan::plain("\n")));

        nodes
    }

    pub(crate) fn format_visibility<'a>(&'a self, item: DocRef<'a, Item>) -> Vec<DocumentNode<'a>> {
        let mut nodes = vec![DocumentNode::Span(StyledSpan::plain("Visibility: "))];

        match &item.item().visibility {
            Visibility::Public => nodes.push(DocumentNode::Span(StyledSpan::plain("Public"))),
            Visibility::Default => nodes.push(DocumentNode::Span(StyledSpan::plain("Private"))),
            Visibility::Crate => nodes.push(DocumentNode::Span(StyledSpan::plain("Crate"))),
            Visibility::Restricted { parent, path } => {
                nodes.push(DocumentNode::Span(StyledSpan::plain("Restricted to ")));
                if let Some(parent_summary) = item.get(parent).and_then(|item| item.summary()) {
                    let mut action_item = None;
                    for (i, segment) in parent_summary.path.iter().enumerate() {
                        if i == 0 {
                            action_item = item
                                .crate_docs()
                                .traverse_to_crate_by_id(self, parent_summary.crate_id)
                                .map(|x| x.root_item(self));
                        } else {
                            nodes.push(DocumentNode::Span(StyledSpan::punctuation("::")));
                            if let Some(ai) = action_item {
                                action_item = ai.find_child(segment);
                            }
                        }

                        nodes.push(DocumentNode::Span(
                            StyledSpan::type_name(segment).with_target(action_item),
                        ));
                    }
                } else {
                    nodes.push(DocumentNode::Span(StyledSpan::plain(path)));
                }
            }
        }

        nodes.push(DocumentNode::Span(StyledSpan::plain("\n")));

        nodes
    }
}
