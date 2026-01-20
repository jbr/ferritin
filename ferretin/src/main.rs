#![allow(dead_code)]

use clap::Parser;
use ferretin_common::RustdocProject;
use std::{path::PathBuf, process::ExitCode};
use terminal_size::{Width, terminal_size};

use crate::{
    commands::Commands, format_context::FormatContext, renderer::OutputMode, request::Request,
};

mod color_scheme;
mod commands;
mod format;
mod format_context;
mod generate_docsrs_url;
mod indent;
mod markdown;
mod renderer;
mod request;
mod styled_string;
#[cfg(test)]
mod tests;
mod traits;
mod verbosity;

/// A friendly CLI for browsing Rust documentation
#[derive(Parser, Debug)]
#[command(name = "ferretin")]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Path to Cargo.toml (defaults to current directory)
    #[arg(short, long, global = true)]
    manifest_path: Option<PathBuf>,

    /// Syntax highlighting theme
    #[arg(
        long,
        global = true,
        default_value = "Solarized (dark)",
        env = "FERRETIN_THEME",
        value_parser = ["InspiredGitHub", "Solarized (dark)", "Solarized (light)", "base16-eighties.dark", "base16-mocha.dark", "base16-ocean.dark", "base16-ocean.light"]
    )]
    theme: String,

    #[command(subcommand)]
    command: Commands,
}

struct IoFmtWriter<T>(T);
impl<T> std::fmt::Write for IoFmtWriter<T>
where
    T: std::io::Write,
{
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        self.0.write_all(s.as_bytes()).map_err(|_| std::fmt::Error)
    }
}

fn main() -> ExitCode {
    env_logger::init();
    let cli = Cli::parse();

    let path = cli
        .manifest_path
        .unwrap_or_else(|| std::env::current_dir().unwrap());

    let Ok(project) = RustdocProject::load(path.clone()) else {
        eprintln!("could not load rust project at {}", path.display());
        return ExitCode::FAILURE;
    };

    let format_context = FormatContext::new()
        .with_output_mode(OutputMode::detect())
        .with_terminal_width(
            terminal_size()
                .map(|(Width(w), _)| w as usize)
                .unwrap_or(80),
        )
        .with_theme(cli.theme.clone());

    let request = Request::new(project, format_context);

    let (document, is_error) = cli.command.execute(&request);

    if request
        .render(&document, &mut IoFmtWriter(std::io::stdout()))
        .is_err()
    {
        return ExitCode::FAILURE;
    }

    if is_error {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
