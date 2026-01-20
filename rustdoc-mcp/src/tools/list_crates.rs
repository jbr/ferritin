use crate::{state::RustdocTools, traits::WriteFmt};
use anyhow::Result;
use clap::Args;
use mcplease::traits::{Tool, WithExamples};
use mcplease::types::Example;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Default, Debug, Serialize, Deserialize, Args, JsonSchema)]
#[serde(rename = "list_crates")]
/// List available crates in the workspace, including dependencies
pub struct ListCrates {
    /// Optional workspace member to scope dependencies to
    #[arg(long)]
    pub workspace_member: Option<String>,
    #[serde(skip)]
    pub for_schemars: (),
}

impl WithExamples for ListCrates {
    fn examples() -> Vec<Example<Self>> {
        vec![Example {
            description: "listing crates",
            item: Self::default(),
        }]
    }
}

impl Tool<RustdocTools> for ListCrates {
    fn execute(self, state: &mut RustdocTools) -> Result<String> {
        let project = state.project_context(None)?;

        // Determine if we're showing a member-scoped view (either via parameter or working directory)
        let is_scoped_view =
            self.workspace_member.is_some() || project.detect_subcrate_context().is_some();

        let mut result = String::new();
        for crate_info in project.crate_info(self.workspace_member.as_deref()) {
            let crate_name = crate_info.name();

            let note = if crate_info.is_default_crate() {
                " (workspace-local, aliased as \"crate\")".to_string()
            } else if crate_info.crate_type().is_workspace() {
                " (workspace-local)".to_string()
            } else if let Some(version) = crate_info.version() {
                let dev_dep_note = if crate_info.is_dev_dep() {
                    " (dev-dep)"
                } else {
                    ""
                };

                // Add workspace member usage info when showing full workspace view
                let usage_info = if !is_scoped_view && !crate_info.used_by().is_empty() {
                    let members: Vec<String> = crate_info
                        .used_by()
                        .iter()
                        .map(|member| {
                            if crate_info.is_dev_dep() {
                                format!("{member} dev")
                            } else {
                                member.clone()
                            }
                        })
                        .collect();
                    format!(" ({})", members.join(", "))
                } else {
                    String::new()
                };

                format!(" {version}{dev_dep_note}{usage_info}")
            } else {
                String::new()
            };
            result.write_fmt(format_args!("• {crate_name}{note}\n"));
            if let Some(description) = crate_info.description() {
                let description = description.replace('\n', " ");
                result.write_fmt(format_args!("    {description}\n"));
            }
        }

        Ok(result)
    }
}
