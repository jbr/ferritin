mod filter;
mod format_context;
mod formatting;
mod indent;
mod request;
mod state;
mod tools;
mod traits;
mod verbosity;

use std::{env, path::PathBuf};

use anyhow::Result;
use mcplease::server_info;
use state::RustdocTools;
use tools::Tools;

const INSTRUCTIONS: &str = "Rustdoc documentation explorer for Rust projects.

Use set_working_directory to set the project directory first, then use get_item to explore types, functions, and other items with their source code.";

fn main() -> Result<()> {
    let storage_path = env::var("MCP_SHARED_SESSION_PATH")
        .map(|path| PathBuf::from(&*shellexpand::tilde(&path)))
        .ok();

    let mut state = RustdocTools::new(storage_path)?;

    mcplease::run::<Tools, _>(&mut state, server_info!(), Some(INSTRUCTIONS))
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod workspace_tests;
