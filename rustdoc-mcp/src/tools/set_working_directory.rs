use crate::state::RustdocTools;
use anyhow::Result;
use clap::Args;
use mcplease::{
    traits::{Tool, WithExamples},
    types::Example,
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

impl WithExamples for SetWorkingDirectory {
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
}

impl Tool<RustdocTools> for SetWorkingDirectory {
    fn execute(self, state: &mut RustdocTools) -> Result<String> {
        let new_context_path = state.resolve_path(&self.path, None)?;
        let response = format!("Set context to {}", new_context_path.display());
        state.set_working_directory(new_context_path, None)?;
        Ok(response)
    }
}
