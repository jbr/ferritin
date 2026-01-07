use fieldwork::Fieldwork;
use rustdoc_types::ItemKind;
use strum::VariantArray;

use crate::{filter::Filter, tools::GetItem, verbosity::Verbosity};

/// Context for formatting operations
#[derive(Debug, Clone, Fieldwork)]
#[fieldwork(get)]
pub(crate) struct FormatContext {
    /// Whether to include source code snippets
    include_source: bool,
    /// Whether to show recursive/nested content
    #[field = "is_recursive"]
    recursive: bool,
    /// Level of documentation detail to show
    #[field(copy)]
    verbosity: Verbosity,
    /// Filter items by type
    filters: Vec<Filter>,
}

impl Default for FormatContext {
    fn default() -> Self {
        Self {
            include_source: false,
            recursive: false,
            verbosity: Verbosity::Brief,
            filters: Filter::VARIANTS.into(),
        }
    }
}

impl FormatContext {
    /// Create context from GetItem tool arguments
    pub(crate) fn from_get_item(item: &GetItem) -> Self {
        Self {
            include_source: item.include_source(),
            recursive: item.recursive(),
            verbosity: item.verbosity(),
            filters: item.filters().to_vec(),
        }
    }

    pub(crate) fn filter_match_kind(&self, kind: ItemKind) -> bool {
        self.filters.iter().any(|filter| filter.matches_kind(kind))
    }
}
