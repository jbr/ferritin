mod filter;
mod format_context;
mod formatting;
mod indent;
mod request;
mod state;
mod tools;
mod traits;
mod verbosity;

use anyhow::Result;
use mcplease::{ServerConfig, server_info};
use state::RustdocTools;
use std::{env, path::PathBuf};
use tools::Tools;

const INSTRUCTIONS: &str = "Rustdoc documentation explorer for Rust projects.

Use set_working_directory to set the project directory first, then use get_item to explore types, \
                            functions, and other items with their source code.";

fn main() -> Result<()> {
    let storage_path = env::var("MCP_SHARED_SESSION_PATH")
        .map(|path| PathBuf::from(&*shellexpand::tilde(&path)))
        .ok();

    let mut state = RustdocTools::new(storage_path)?;

    // The tool list is fixed at compile time, so mcplease's hour-long default
    // `tools/list` ttl is right as-is.
    let config = ServerConfig::new(server_info!()).with_instructions(INSTRUCTIONS);

    mcplease::run::<Tools, _>(&mut state, config)
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod workspace_tests;
