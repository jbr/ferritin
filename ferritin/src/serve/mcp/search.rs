//! The `search` tool: items within a crate, or — without a crate — which crate
//! to use.

use super::{McpState, SEARCH_LIMIT};
use crate::commands::Commands;
use anyhow::Result;
use mcplease::{
    traits::{Tool, ToolMeta},
    types::{Example, RequestContext, ToolAnnotations},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Search for items within a crate's documentation, or — without a crate —
/// discover which crate to use.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename = "search")]
pub struct Search {
    /// Crate to search within, optionally with an `@`-suffixed semver
    /// requirement (not an exact version): `serde`, `tokio@1` (newest 1.x), or
    /// `tokio@=1.40` to pin an exact release.
    ///
    /// **Omit only to discover which crate to use.** With `crate`, the search
    /// runs over that crate's full documentation — item names and prose.
    /// Without it, it searches a different, much shallower dataset: crates.io
    /// crate names, descriptions, and declared keywords. It returns crates,
    /// not items, and cannot see any crate's actual documentation. If you
    /// already know the crate name, always pass it.
    #[serde(rename = "crate", default, skip_serializing_if = "Option::is_none")]
    pub crate_: Option<String>,
    /// Search query — one or more complete words. With `crate`, matched
    /// against item names and documentation prose; without it, against crate
    /// names, descriptions, and declared keywords.
    pub query: String,
}

impl ToolMeta for Search {
    fn examples() -> Vec<Example<Self>> {
        vec![
            Example {
                description: "Find deserialization items in serde",
                item: Self {
                    crate_: Some("serde".into()),
                    query: "deserialize".into(),
                },
            },
            Example {
                description: "Discover which crate to use for MQTT",
                item: Self {
                    crate_: None,
                    query: "mqtt client".into(),
                },
            },
        ]
    }

    fn annotations() -> Option<ToolAnnotations> {
        Some(super::READ_ONLY_LOOKUP)
    }
}

impl Tool<McpState> for Search {
    /// Markdown, for the reason given on [`Get::execute`](super::Get).
    type Output = String;

    fn execute(self, state: &mut McpState, _context: &RequestContext) -> Result<Self::Output> {
        match self.crate_ {
            Some(crate_) => {
                let command = Commands::search(self.query)
                    .in_crate(crate_)
                    .with_limit(SEARCH_LIMIT)
                    .complete_words();
                Ok(state.render(command))
            }
            None => Ok(state.search_crates(&self.query)),
        }
    }
}
