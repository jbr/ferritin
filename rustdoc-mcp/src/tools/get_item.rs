use crate::filter::Filter;
use crate::format_context::FormatContext;
use crate::request::Request;
use crate::state::RustdocTools;
use crate::traits::WriteFmt;
use crate::verbosity::Verbosity;
use anyhow::Result;
use clap::{ArgAction, Args};
use mcplease::{
    traits::{Tool, WithExamples},
    types::Example,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use strum::VariantArray;

/// Get detailed information about a specific item or list items in a module/crate
///
/// ## Usage Patterns
/// - **Single item**: `name: "crate::MyStruct"` - Shows details for a specific type/function
/// - **Module listing**: `name: "crate::module"` - Lists items in a module  
/// - **Recursive exploration**: `name: "crate", recursive: true` - Shows all items in module tree
/// - **Filtered search**: `name: "crate", filter: ["struct", "enum"]` - Shows only specified item types
///
/// ## Parameter Combinations
/// - `recursive` works with `filter` to recursively show only filtered item types
/// - `include_source` adds source code snippets to any item that has them
/// - `verbosity` controls detail level for all output (minimal/brief/full)
#[derive(Debug, Default, Serialize, Deserialize, JsonSchema, Args)]
#[serde(rename = "get_item")]
pub struct GetItem {
    /// The name of the item to show (e.g., "crate::MyStruct", "serde_json::Value", "std::vec::Vec")
    pub name: String,

    /// Whether to include source code snippets (default: false)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[arg(long, action = ArgAction::SetTrue)]
    pub include_source: Option<bool>,

    /// Show recursive listing of all items in module and submodules (default: false)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[arg(long, action = ArgAction::SetTrue)]
    pub recursive: Option<bool>,

    /// Filter items in listing. Supports: struct, enum, trait, function, constant, static, module, union, macro, type
    /// Examples: `["struct"]`, `["struct", "enum"]`, `["function"]`
    /// Default: all item types
    #[serde(skip_serializing_if = "Option::is_none")]
    #[arg(long, value_enum)]
    pub filter: Option<Vec<Filter>>,

    /// Control documentation verbosity: minimal (structure only), brief (truncated with hints), full (complete)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[arg(long, value_enum)]
    pub verbosity: Option<Verbosity>,
}

impl GetItem {
    /// Get include_source with default
    pub(crate) fn include_source(&self) -> bool {
        self.include_source.unwrap_or(false)
    }

    /// Get recursive with default
    pub(crate) fn recursive(&self) -> bool {
        self.recursive.unwrap_or(false)
    }

    /// Get verbosity with default
    pub(crate) fn verbosity(&self) -> Verbosity {
        self.verbosity.unwrap_or_default()
    }

    /// Get filters with default
    pub(crate) fn filters(&self) -> &[Filter] {
        self.filter.as_deref().unwrap_or(Filter::VARIANTS)
    }
}

impl WithExamples for GetItem {
    fn examples() -> Vec<Example<Self>> {
        vec![
            Example {
                description: "Get information about a type in the current crate",
                item: Self {
                    name: "crate::MyStruct".to_string(),
                    include_source: Some(true),
                    ..Default::default()
                },
            },
            Example {
                description: "Get information about a type from a dependency",
                item: Self {
                    name: "serde_json::Value".to_string(),
                    include_source: Some(false),
                    ..Default::default()
                },
            },
            Example {
                description: "List all items in a module",
                item: Self {
                    name: "crate::tools".to_string(),
                    ..Default::default()
                },
            },
            Example {
                description: "Show root module of a crate",
                item: Self {
                    name: "serde_json".to_string(),
                    ..Default::default()
                },
            },
            Example {
                description: "Show recursive listing of all items in a module",
                item: Self {
                    name: "crate::tools".to_string(),
                    recursive: Some(true),
                    ..Default::default()
                },
            },
            Example {
                description: "Filter to show only structs",
                item: Self {
                    name: "crate".to_string(),
                    filter: Some(vec![Filter::Struct]),
                    ..Default::default()
                },
            },
            Example {
                description: "Filter to show functions (including methods)",
                item: Self {
                    name: "crate".to_string(),
                    recursive: Some(true),
                    filter: Some(vec![Filter::Function]),
                    ..Default::default()
                },
            },
            Example {
                description: "Show module structure without documentation",
                item: Self {
                    name: "serde_json".to_string(),
                    verbosity: Some(Verbosity::Minimal),
                    ..Default::default()
                },
            },
            Example {
                description: "Show complete documentation without truncation",
                item: Self {
                    name: "serde_json::Value".to_string(),
                    verbosity: Some(Verbosity::Full),
                    ..Default::default()
                },
            },
        ]
    }
}

impl Tool<RustdocTools> for GetItem {
    fn execute(self, tools: &mut RustdocTools) -> Result<String> {
        let manifest_path = tools.resolve_path("Cargo.toml", None)?;
        let request = Request::new(manifest_path);

        let mut suggestions = vec![];

        if let Some(item) = request.resolve_path(&self.name, &mut suggestions) {
            let context = FormatContext::from_get_item(&self);

            Ok(request.format_item(item, &context))
        } else {
            let mut result = format!("`{}` not found. Did you mean one of these?\n\n", self.name);
            suggestions.sort_by(|a, b| b.score().total_cmp(&a.score()));
            for suggestion in suggestions.into_iter().take(5).filter(|s| s.score() > 0.8) {
                result.write_fmt(format_args!("• `{}` ", suggestion.path()));

                if let Some(item) = suggestion.item() {
                    result.write_fmt(format_args!("({:?})\n", item.kind()));
                } else {
                    result.push_str("(Crate)\n");
                }
            }
            Ok(result)
        }
    }
}
