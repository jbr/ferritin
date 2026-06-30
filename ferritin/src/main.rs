#![allow(dead_code)]

use clap::Parser;

// Include the generated themes module
mod themes {
    include!(concat!(env!("OUT_DIR"), "/themes.rs"));
}
use ferritin_common::{
    Navigator,
    sources::{DocsRsSource, FeatureSelection, LocalSource, StdSource},
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
mod json;
mod kind;
mod logging;
mod markdown;
mod render_context;
mod renderer;
mod request;
mod styled_string;
#[cfg(test)]
mod tests;
mod traits;
mod verbosity;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

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

    /// Use local workspace (implies --manifest-path cwd if not set). By default, docs.rs is used.
    #[arg(short = 'l', long, global = true)]
    local: bool,

    /// Force rebuilding rustdoc for the queried local crate, ignoring the cache.
    /// Useful when cached docs are stale (e.g. after switching branches).
    #[arg(long, global = true)]
    rebuild: bool,

    /// Hide non-public items (private fields, methods, and module items) when
    /// documenting local crates.
    #[arg(long, global = true)]
    public: bool,

    /// Build local docs with these cargo features (comma-separated or repeated).
    /// Requires --local; ignored for docs.rs. The selection sticks: later bare
    /// invocations reuse it until you pass different features or --rebuild.
    #[arg(long, global = true, value_delimiter = ',')]
    features: Vec<String>,

    /// Build local docs with all cargo features enabled. Requires --local.
    #[arg(long, global = true)]
    all_features: bool,

    /// Build local docs without the default cargo features. Requires --local.
    #[arg(long, global = true)]
    no_default_features: bool,

    /// Output format. Defaults to autodetection: agent format under coding
    /// agents (CLAUDECODE/GEMINI_CLI/CODEX_SANDBOX), ANSI on a TTY, plain when
    /// piped. `json` emits the structured item model and is only valid for `get`.
    #[arg(long, global = true, value_enum)]
    format: Option<Format>,

    #[command(subcommand)]
    command: Option<Commands>,
}

/// Explicit `--format` override. `Agent`/`Plain`/`Tty` select a renderer; `Json`
/// takes a separate path that serializes the semantic item model directly,
/// bypassing the `Document` render pipeline.
#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
enum Format {
    /// ANSI terminal output (colors, hyperlinks)
    Tty,
    /// Plain text, no decoration
    Plain,
    /// Token-efficient output for coding agents and other LLM readers
    Agent,
    /// Structured JSON of the item model (only valid for `get`)
    Json,
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
    let cli = Cli::parse();

    let use_local = cli.local || cli.manifest_path.is_some();
    let path = cli
        .manifest_path
        .unwrap_or_else(|| std::env::current_dir().unwrap());

    // A feature selection is requested only when the user passed at least one
    // feature flag; otherwise `None` lets the cached (sticky) selection stand.
    let requested_features = (!cli.features.is_empty()
        || cli.all_features
        || cli.no_default_features)
        .then(|| FeatureSelection {
            no_default: cli.no_default_features,
            all: cli.all_features,
            list: cli.features.clone(),
        });

    if requested_features.is_some() && !use_local {
        eprintln!(
            "--features, --all-features, and --no-default-features require --local: \
             docs.rs builds are not under ferritin's control."
        );
        return ExitCode::FAILURE;
    }

    // `--format json` is incompatible with the interactive TUI.
    if cli.format == Some(Format::Json) && cli.interactive {
        eprintln!("--format json cannot be combined with --interactive");
        return ExitCode::FAILURE;
    }

    let mut output_mode = OutputMode::detect();
    match cli.format {
        Some(Format::Tty) => output_mode = OutputMode::Tty,
        Some(Format::Plain) => output_mode = OutputMode::Plain,
        Some(Format::Agent) => output_mode = OutputMode::Agent,
        // JSON bypasses the renderer entirely; handled in the non-interactive
        // path below. Output mode is irrelevant there.
        Some(Format::Json) | None => {}
    }

    let mut render_context = RenderContext::new()
        .with_output_mode(output_mode)
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

    if cli.interactive {
        // Interactive mode with scrolling and navigation
        // Install custom log backend that captures logs for status bar
        let (log_backend, log_reader) = logging::StatusLogBackend::new(10_000);
        if let Err(e) = log_backend.install() {
            eprintln!("Failed to install log backend: {}", e);
            return ExitCode::FAILURE;
        }

        if let Err(e) = renderer::render_interactive(
            path,
            use_local,
            render_context,
            cli.command,
            log_reader,
            cli.public,
            requested_features,
        ) {
            eprintln!("Interactive mode error: {}", e);
            return ExitCode::FAILURE;
        }
        return ExitCode::SUCCESS;
    }

    // Non-interactive mode: build sources eagerly and handle errors upfront
    let std_source = StdSource::from_rustup();
    let navigator = if use_local {
        let local_source = LocalSource::load(&path);
        if let Err(error) = &local_source {
            eprintln!("could not load rust project at {}", path.display());
            log::error!("{error:?}");
            return ExitCode::FAILURE;
        }
        Navigator::default()
            .with_std_source(std_source)
            .with_local_source(local_source.ok().map(|ls| {
                ls.with_force_rebuild(cli.rebuild)
                    .with_features(requested_features)
            }))
    } else {
        Navigator::default()
            .with_std_source(std_source)
            .with_docsrs_source(DocsRsSource::from_default_cache())
    };

    let format_context = FormatContext::new().with_public(cli.public);
    let mut request = Request::new(&navigator, format_context);

    // Use env_logger for CLI mode
    env_logger::init();

    // `--format json` takes a separate path: it serializes the semantic item
    // model directly instead of going through the `Document` render pipeline.
    if cli.format == Some(Format::Json) {
        return run_json(&mut request, cli.command);
    }

    // One-shot mode: execute command and render to stdout
    let (document, is_error, _initial_entry) = cli
        .command
        .unwrap_or_else(Commands::list)
        .execute(&mut request);

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

/// Handle `--format json`, bypassing the `Document` render pipeline. `get`
/// serializes its structural [`crate::format::ItemDoc`] model; every other
/// command serializes its presentation `Document` generically (see
/// [`crate::json::document_to_string`]).
fn run_json(request: &mut Request<'_>, command: Option<Commands>) -> ExitCode {
    let (json, is_error) = match command.unwrap_or_else(Commands::list) {
        Commands::Get {
            path,
            source,
            recursive,
            kind,
            docs,
        } => {
            request
                .format_context()
                .set_filter(crate::kind::predicate(&kind))
                .set_doc_level(docs);
            match commands::get::model(request, &path, source, recursive) {
                commands::get::JsonOutcome::Found {
                    model,
                    canonical_url,
                } => (json::to_string(&model, Some(canonical_url)), false),
                // Not found: serialize the structural suggestions result and
                // signal failure.
                commands::get::JsonOutcome::NotFound(not_found) => {
                    (json::not_found_to_string(&not_found), true)
                }
            }
        }
        Commands::List => (
            json::list_to_string(&commands::list::json_model(request)),
            false,
        ),
        Commands::Search {
            limit,
            kind,
            target,
        } => {
            request
                .format_context()
                .set_filter(crate::kind::predicate(&kind));
            let (crate_, query) = commands::search::parse_target(target);
            let model = commands::search::model(request, &query, limit, crate_.as_deref());
            (json::search_to_string(&model), model.is_error())
        }
    };

    match json {
        Ok(json) => {
            println!("{json}");
            if is_error {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(error) => {
            eprintln!("failed to serialize JSON: {error}");
            ExitCode::FAILURE
        }
    }
}
