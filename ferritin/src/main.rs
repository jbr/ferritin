#![allow(dead_code)]

use clap::Parser;

// Include the generated themes module
mod themes {
    include!(concat!(env!("OUT_DIR"), "/themes.rs"));
}
use ferritin_common::{
    Navigator,
    sources::{DocsRsSource, LocalSource, StdSource},
};
use std::{path::PathBuf, process::ExitCode};
use terminal_size::{Width, terminal_size};

use crate::{
    commands::Commands, format_context::FormatContext, render_context::RenderContext,
    renderer::OutputMode, request::Request,
};

mod color_scheme;
mod commands;
mod format;
mod format_context;
mod generate_docsrs_url;
mod indent;
mod markdown;
mod render_context;
mod renderer;
mod request;
mod styled_string;
#[cfg(test)]
mod tests;
mod traits;
mod verbosity;

/// A friendly CLI for browsing Rust documentation
#[derive(Parser, Debug)]
#[command(name = "ferritin")]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Path to Cargo.toml (defaults to current directory)
    #[arg(short, long, global = true)]
    manifest_path: Option<PathBuf>,

    /// Syntax highlighting theme (theme name or path to .tmTheme file)
    #[arg(
        long,
        short,
        global = true,
        default_value = "Catppuccin Frappe",
        env = "FERRITIN_THEME",
        long_help = build_theme_help()
    )]
    theme: String,

    /// Enable interactive mode with scrolling and navigation
    #[arg(short, long, global = true)]
    interactive: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

fn build_theme_help() -> &'static str {
    use std::sync::OnceLock;
    static HELP: OnceLock<String> = OnceLock::new();

    HELP.get_or_init(|| {
        let mut help = String::from("Syntax highlighting theme\n\n");
        help.push_str("Can be either:\n");
        help.push_str("  - A theme name from the list below\n");
        help.push_str("  - A path to a .tmTheme file\n\n");
        help.push_str("Available themes:\n");

        for name in themes::THEME_NAMES {
            help.push_str(&format!("  - {}\n", name));
        }

        help
    })
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

    let local_source = LocalSource::load(&path);

    if !cli.interactive
        && let Err(error) = local_source
    {
        eprintln!("could not load rust project at {}", path.display());
        log::error!("{error:?}");
        return ExitCode::FAILURE;
    }

    let navigator = Navigator::default()
        .with_std_source(StdSource::from_rustup())
        .with_local_source(LocalSource::load(&path).ok())
        .with_docsrs_source(DocsRsSource::from_default_cache());

    let format_context = FormatContext::new();

    let mut render_context = RenderContext::new()
        .with_output_mode(OutputMode::detect())
        .with_terminal_width(
            terminal_size()
                .map(|(Width(w), _)| w as usize)
                .unwrap_or(80),
        )
        .with_interactive(cli.interactive);

    if let Err(e) = render_context.set_theme_name(&cli.theme) {
        eprintln!("{e}");
        return ExitCode::FAILURE;
    };

    let request = Request::new(navigator, format_context);

    if cli.interactive {
        // Interactive mode with scrolling and navigation
        if let Err(e) = renderer::render_interactive(&request, render_context, cli.command) {
            eprintln!("Interactive mode error: {}", e);
            return ExitCode::FAILURE;
        }
        return ExitCode::SUCCESS;
    }

    // One-shot mode: execute command and render to stdout
    let (document, is_error, _initial_entry) =
        cli.command.unwrap_or_else(Commands::list).execute(&request);

    // Render to stdout and exit
    if renderer::render(
        &document,
        &render_context,
        &mut IoFmtWriter(std::io::stdout()),
    )
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
