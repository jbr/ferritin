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
use clap::Parser;
use mcplease::{Cli, ServerConfig, server_info, traits::Dispatch, types::RequestContext};
use smol::{Executor, Unblock};
use state::RustdocTools;
use std::path::PathBuf;
use tools::Tools;

const INSTRUCTIONS: &str = "Rustdoc documentation explorer for Rust projects.

Every tool looks in the Rust project this server was started in, unless you pass `project` (a \
                            directory with a Cargo.toml). Use get_item to explore types, \
                            functions, and other items with their source code.";

#[derive(Parser)]
#[command(version, about)]
struct Args {
    /// The directory `project` resolves against, and the default project
    /// [default: the current directory]
    #[arg(long, global = true)]
    root: Option<PathBuf>,

    #[command(subcommand)]
    command: Cli<Tools>,
}

fn main() -> Result<()> {
    // stderr is the one place a stdio server may log.
    env_logger::init();
    let Args { root, command } = Args::parse();
    let root = match root {
        Some(root) => root,
        None => std::env::current_dir()?,
    };
    let state = RustdocTools::new(root);

    match command {
        Cli::Tool(tool) => {
            let output = smol::block_on(tool.call_to_text(state, RequestContext::default()))?;
            println!("{output}");
            Ok(())
        }

        Cli::Serve => {
            // The tool list is fixed at compile time, so mcplease's hour-long
            // default `tools/list` ttl is right as-is.
            let config = ServerConfig::new(server_info!()).with_instructions(INSTRUCTIONS);
            let executor = Executor::new();
            smol::block_on(executor.run(async {
                let stdin = Unblock::new(std::io::stdin());
                let stdout = Unblock::new(std::io::stdout());
                let mut calls = mcplease::calls::<Tools, _>(stdin, stdout, state, &config);
                while let Some(call) = calls.next().await {
                    executor.spawn(call?).detach();
                }
                Ok(())
            }))
        }
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod workspace_tests;
