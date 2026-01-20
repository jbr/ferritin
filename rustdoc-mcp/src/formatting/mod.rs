use crate::format_context::FormatContext;
use crate::indent::Indent;
use crate::request::Request;
use crate::traits::WriteFmt;
use crate::verbosity::Verbosity;
use ferretin_common::doc_ref::DocRef;
use rustdoc_types::{
    Abi, Constant, Enum, Function, FunctionPointer, GenericArg, GenericArgs, GenericBound,
    GenericParamDef, GenericParamDefKind, Generics, Id, Item, ItemEnum, Path, Span, Static, Struct,
    StructKind, Term, Trait, Type, TypeAlias, Union, VariantKind, Visibility, WherePredicate,
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
    pub(crate) fn format_item(&self, item: DocRef<'_, Item>, context: &FormatContext) -> String {
        let mut result = String::new();

        // Basic item information
        result.write_fmt(format_args!("Item: {}\n", item.name().unwrap_or("unnamed")));
        result.write_fmt(format_args!("Kind: {:?}\n", item.kind()));
        result.write_fmt(format_args!("Visibility: {:?}\n", item.visibility));

        if let Some(path) = item.path() {
            result.write_fmt(format_args!("Defined at: {path}\n"));
        }

        // Add documentation if available
        if let Some(docs) = self.docs_to_show(item, false, context) {
            result.write_fmt(format_args!("\n{docs}\n\n"));
        };

        // Handle different item types
        let str = match &item.inner {
            ItemEnum::Module(_) => self.format_module(item, context),
            ItemEnum::Struct(struct_data) => {
                self.format_struct(item, item.build_ref(struct_data), context)
            }
            ItemEnum::Enum(enum_data) => self.format_enum(item, item.build_ref(enum_data), context),
            ItemEnum::Trait(trait_data) => {
                self.format_trait(item, item.build_ref(trait_data), context)
            }
            ItemEnum::Function(function_data) => {
                result.push('\n');
                self.format_function(item, item.build_ref(function_data), context)
            }
            ItemEnum::TypeAlias(type_alias_data) => {
                self.format_type_alias(item, item.build_ref(type_alias_data), context)
            }
            ItemEnum::Union(union_data) => {
                self.format_union(item, item.build_ref(union_data), context)
            }
            ItemEnum::Constant { type_, const_ } => {
                self.format_constant(item, type_, const_, context)
            }
            ItemEnum::Static(static_data) => self.format_static(item, static_data, context),

            ItemEnum::Macro(macro_def) => {
                format!("Macro definition:\n\n```rust\n{macro_def}\n```")
            }
            _ => {
                // For any other item, just print its name and kind
                format!(
                    "\n{:?} {}\n",
                    item.kind(),
                    item.name().unwrap_or("<unnamed>")
                )
            }
        };

        result.push_str(&str);

        // Add source code if requested
        if context.include_source()
            && let Some(span) = &item.span
        {
            result.push_str(&source::format_source_code(self, span));
        }

        result
    }
}
