//! The `get` tool: documentation for a Rust item by path.

use super::McpState;
use crate::commands::Commands;
use anyhow::Result;
use mcplease::{
    traits::{Tool, ToolMeta},
    types::{Example, RequestContext, ToolAnnotations},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Show documentation for a Rust item by path.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename = "get")]
pub struct Get {
    /// Item path, e.g. `serde::Deserialize` or `std::vec::Vec`. A crate segment
    /// may carry an `@`-suffixed semver requirement (not an exact version):
    /// `tokio@1::runtime::Runtime` serves the newest 1.x, while `tokio@=1.40`
    /// pins an exact release.
    pub path: String,
}

impl ToolMeta for Get {
    fn examples() -> Vec<Example<Self>> {
        vec![Example {
            description: "Look up a trait",
            item: Self {
                path: "serde::Deserialize".into(),
            },
        }]
    }

    fn annotations() -> Option<ToolAnnotations> {
        Some(super::READ_ONLY_LOOKUP)
    }
}

impl Tool<McpState> for Get {
    // Markdown, not a structured value: the point of this server is a rendering
    // calibrated for a model to read, and the spec's structured shape would
    // duplicate the whole document as JSON to say the same thing.
    type Output = String;

    fn execute(self, state: &mut McpState, _context: &RequestContext) -> Result<Self::Output> {
        Ok(state.render(Commands::get(self.path)))
    }
}
