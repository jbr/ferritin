use crate::{
    commands::Commands,
    format_context::FormatContext,
    render_context::RenderContext,
    renderer::{OutputMode, render},
    request::Request,
};
use ferritin_common::{
    Navigator,
    sources::{LocalSource, StdSource},
};
use ratatui::backend::TestBackend;
use std::path::PathBuf;

/// Get the path to our test crate (fast to build, minimal dependencies)
fn get_test_crate_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/test-crate")
}

/// Create a test state with isolated session
fn create_test_state() -> Request {
    let navigator = Navigator::default()
        .with_local_source(LocalSource::load(&get_test_crate_path()).ok())
        .with_std_source(StdSource::from_rustup());
    Request::new(navigator, FormatContext::new())
}

/// Convert OSC8 hyperlinks to markdown-style [text](url) before stripping ANSI
fn convert_osc8_to_markdown(text: &str) -> String {
    use regex::Regex;

    // OSC8 format: ESC]8;;URL ESC\TEXT ESC]8;; ESC\
    let re = Regex::new("\x1B\\]8;;([^\x1B]*)\x1B\\\\(.*?)\x1B\\]8;;\x1B\\\\").unwrap();

    re.replace_all(text, "[$2]($1)").to_string()
}

fn render_for_tests(command: Commands, output_mode: OutputMode) -> String {
    let request = create_test_state();
    let (document, _, _) = command.execute(&request);
    let mut output = String::new();
    let render_context = RenderContext::new().with_output_mode(output_mode);
    render(&document, &render_context, &mut output).unwrap();

    // For TTY mode: convert OSC8 links to markdown, then strip remaining ANSI codes
    let output = if matches!(output_mode, OutputMode::Tty) {
        let with_markdown_links = convert_osc8_to_markdown(&output);
        String::from_utf8(strip_ansi_escapes::strip(with_markdown_links.as_bytes()))
            .unwrap_or(with_markdown_links)
    } else {
        output
    };

    // Normalize the test crate path for consistent snapshots across environments
    let test_crate_path = get_test_crate_path();
    let test_crate_path_str = test_crate_path
        .canonicalize()
        .unwrap_or(test_crate_path)
        .to_string_lossy()
        .to_string();
    let output = output.replace(&test_crate_path_str, "/TEST_CRATE_ROOT");

    // Normalize Rust version info to avoid daily breakage with nightly updates
    // Matches patterns like: 1.95.0-nightly	(f889772d6	2026-02-05)
    let re =
        regex::Regex::new(r"\d+\.\d+\.\d+-[a-z]+\s+\([a-f0-9]+\s+\d{4}-\d{2}-\d{2}\)").unwrap();
    re.replace_all(&output, "RUST_VERSION").to_string()
}

fn render_interactive_for_tests(command: Commands) -> TestBackend {
    use crate::renderer::render_to_test_backend;

    let request = create_test_state();
    let (document, _, _) = command.execute(&request);
    let render_context = RenderContext::new();

    render_to_test_backend(document, render_context)
}

/// Macro to run the same test across all output modes
macro_rules! test_all_modes {
    ($name:ident, $cmd:expr) => {
        paste::paste! {
            #[test]
            fn [<$name _test_mode>]() {
                insta::assert_snapshot!(render_for_tests($cmd, OutputMode::TestMode));
            }

            #[test]
            fn [<$name _tty_mode>]() {
                insta::assert_snapshot!(render_for_tests($cmd, OutputMode::Tty));
            }

            #[test]
            fn [<$name _plain_mode>]() {
                insta::assert_snapshot!(render_for_tests($cmd, OutputMode::Plain));
            }

            #[test]
            fn [<$name _interactive_mode>]() {
                let test_crate_path = get_test_crate_path();
                let test_crate_path_str = test_crate_path
                    .canonicalize()
                    .unwrap_or(test_crate_path)
                    .to_string_lossy()
                    .to_string();

                let mut settings = insta::Settings::clone_current();
                settings.add_filter(&test_crate_path_str, "/TEST_CRATE_ROOT");
                // Strip trailing whitespace from lines containing the replaced path
                // to avoid snapshot differences due to fixed-width TUI padding
                settings.add_filter(r#"(?m)(.*TEST_CRATE_ROOT[^"]+?)\s+"$"#, r#"$1""#);
                // Normalize Rust version info to avoid daily breakage with nightly updates
                // Matches patterns like: 1.95.0-nightly	(f889772d6	2026-02-05)
                settings.add_filter(r"\d+\.\d+\.\d+-[a-z]+\s+\([a-f0-9]+\s+\d{4}-\d{2}-\d{2}\)", "RUST_VERSION");
                settings.bind(|| {
                    insta::assert_snapshot!(render_interactive_for_tests($cmd));
                });
            }
        }
    };
}

test_all_modes!(get_crate_root, Commands::get("crate"));

// Using macro to test across all modes
test_all_modes!(get_struct_details, Commands::get("crate::TestStruct"));

test_all_modes!(
    get_struct_with_source,
    Commands::get("crate::TestStruct").with_source()
);

test_all_modes!(get_submodule, Commands::get("crate::submodule"));

test_all_modes!(
    get_enum_details,
    Commands::get("crate::submodule::TestEnum")
);

test_all_modes!(get_generic_enum, Commands::get("crate::GenericEnum"));

test_all_modes!(nonexistent_item, Commands::get("crate::DoesNotExist"));

test_all_modes!(recursive_module_listing, Commands::get("crate").recursive());

test_all_modes!(
    recursive_submodule_listing,
    Commands::get("crate::submodule").recursive()
);

test_all_modes!(
    get_item_with_normalized_crate_name,
    Commands::get("test-crate::TestStruct")
);

test_all_modes!(list_crates, Commands::list());

test_all_modes!(search, Commands::search("trigger line-based truncation"));

test_all_modes!(search_2, Commands::search("generic struct"));

test_all_modes!(
    fuzzy_matching_typo,
    Commands::get("crate::TestStruct::test_metod")
); // typo: should suggest "test_method"

test_all_modes!(
    fuzzy_matching_trait_methods,
    Commands::get("crate::TestStruct::cute")
); // Should suggest "clone" from Clone trait

test_all_modes!(
    fuzzy_matching_suggestions,
    Commands::get("crate::TestStruct::incrementCount")
); // typo: should be increment_count

test_all_modes!(get_std, Commands::get("std"));

test_all_modes!(
    get_markdown_test,
    Commands::get("test-crate::markdown_test")
);
