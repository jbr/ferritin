use crate::request::Request;
use crate::{state::RustdocTools, traits::WriteFmt};
use anyhow::Result;
use clap::Args;
use mcplease::traits::{Tool, ToolMeta};
use mcplease::types::{Example, RequestContext, ToolAnnotations};
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

impl ToolMeta for ListCrates {
    fn examples() -> Vec<Example<Self>> {
        vec![Example {
            description: "listing crates",
            item: Self::default(),
        }]
    }

    fn annotations() -> Option<ToolAnnotations> {
        Some(crate::tools::READ_ONLY_LOCAL)
    }
}

impl Tool<RustdocTools> for ListCrates {
    type Output = String;

    fn execute(self, state: &mut RustdocTools, _context: &RequestContext) -> Result<Self::Output> {
        let manifest_path = state.working_directory(None)?;
        let navigator = crate::request::build_navigator(manifest_path);
        let request = Request::new(&navigator);

        let mut result = String::new();
        let root_crate = request.navigator().local_source().and_then(|ls| ls.root_crate());

        let mut available_crates = request
            .navigator()
            .list_available_crates()
            .filter(|c| {
                root_crate.is_none_or(|rc| {
                    !c.provenance().is_local_dependency() || c.used_by().iter().any(|u| **u == **rc)
                })
            })
            .collect::<Vec<_>>();

        available_crates.sort_by(|a, b| a.name().cmp(b.name()));

        for crate_info in available_crates {
            let crate_name = crate_info.name();

            let note = if crate_info.is_default_crate() {
                " (workspace-local, aliased as \"crate\")".to_string()
            } else if crate_info.provenance().is_workspace() {
                " (workspace-local)".to_string()
            } else {
                // Add workspace member usage info when showing full workspace view
                let usage_info = if !crate_info.used_by().is_empty() {
                    format!(" ({})", crate_info.used_by().join(", "))
                } else {
                    String::new()
                };

                format!(" {}{usage_info}", crate_info.version())
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
