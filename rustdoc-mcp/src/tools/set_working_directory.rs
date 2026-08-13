use crate::state::RustdocTools;
use anyhow::Result;
use clap::Args;
use mcplease::{
    traits::{Tool, ToolMeta},
    types::{Example, RequestContext, ToolAnnotations},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Set the working context path for a session
#[derive(Debug, Serialize, Deserialize, JsonSchema, Args)]
#[serde(rename = "set_working_directory")]
pub struct SetWorkingDirectory {
    /// Set the manifest directory for this session
    pub path: String,
}

impl ToolMeta for SetWorkingDirectory {
    fn examples() -> Vec<Example<Self>> {
        vec![
            Example {
                description: "Set working directory to a Rust project",
                item: Self {
                    path: "/path/to/rust/project".to_string(),
                },
            },
            Example {
                description: "Set working directory using tilde expansion",
                item: Self {
                    path: "~/code/my-rust-project".to_string(),
                },
            },
        ]
    }

    /// The only tool here that writes anything: it rebinds the session's
    /// working directory. Not destructive — it replaces a pointer, losing no
    /// data — and setting the same path twice is the same as setting it once.
    fn annotations() -> Option<ToolAnnotations> {
        Some(ToolAnnotations {
            read_only_hint: Some(false),
            destructive_hint: Some(false),
            idempotent_hint: Some(true),
            open_world_hint: Some(false),
            ..ToolAnnotations::default()
        })
    }
}

impl Tool<RustdocTools> for SetWorkingDirectory {
    type Output = String;

    fn execute(self, state: &mut RustdocTools, _context: &RequestContext) -> Result<Self::Output> {
        let new_context_path = state.resolve_path(&self.path, None)?;
        let response = format!("Set context to {}", new_context_path.display());
        state.set_working_directory(new_context_path, None)?;
        Ok(response)
    }
}
