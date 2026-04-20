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
fn get_fixture_crate_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixture-crate")
}

/// Get the path to the multi-crate workspace (crate-a + crate-b, where crate-b
/// depends on crate-a). Used for cross-crate fixture tests.
fn get_test_workspace_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/test-workspace")
}

/// Create a test state rooted at a given path (expected to be a Cargo workspace root).
fn create_test_state_at(path: &std::path::Path) -> Request {
    let navigator = Navigator::default()
        .with_local_source(LocalSource::load(path).ok())
        .with_std_source(StdSource::from_rustup());
    Request::new(navigator, FormatContext::new())
}

/// Create a test state with isolated session
fn create_test_state() -> Request {
    create_test_state_at(&get_fixture_crate_path())
}

/// Convert OSC8 hyperlinks to markdown-style [text](url) before stripping ANSI
fn convert_osc8_to_markdown(text: &str) -> String {
    use regex::Regex;

    // OSC8 format: ESC]8;;URL ESC\TEXT ESC]8;; ESC\
    let re = Regex::new("\x1B\\]8;;([^\x1B]*)\x1B\\\\(.*?)\x1B\\]8;;\x1B\\\\").unwrap();

    re.replace_all(text, "[$2]($1)").to_string()
}

fn render_for_tests_rooted(
    command: Commands,
    output_mode: OutputMode,
    project_root: &std::path::Path,
) -> String {
    let request = create_test_state_at(project_root);
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
    let project_root_str = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf())
        .to_string_lossy()
        .to_string();
    let output = output.replace(&project_root_str, "/TEST_CRATE_ROOT");

    // Normalize Rust version info to avoid daily breakage with nightly updates
    // Matches patterns like: 1.95.0-nightly	(f889772d6	2026-02-05)
    let re =
        regex::Regex::new(r"\d+\.\d+\.\d+-[a-z]+\s+\([a-f0-9]+\s+\d{4}-\d{2}-\d{2}\)").unwrap();
    re.replace_all(&output, "RUST_VERSION").to_string()
}

fn render_for_tests(command: Commands, output_mode: OutputMode) -> String {
    render_for_tests_rooted(command, output_mode, &get_fixture_crate_path())
}

fn render_interactive_for_tests_rooted(
    command: Commands,
    project_root: &std::path::Path,
) -> TestBackend {
    use crate::renderer::render_to_test_backend;

    let request = create_test_state_at(project_root);
    let (document, _, _) = command.execute(&request);
    let render_context = RenderContext::new();

    render_to_test_backend(document, render_context)
}

fn render_interactive_for_tests(command: Commands) -> TestBackend {
    render_interactive_for_tests_rooted(command, &get_fixture_crate_path())
}

/// Macro to run the same test across all output modes, rooted at a given path.
macro_rules! test_all_modes_rooted {
    ($name:ident, $cmd:expr, $path_fn:ident) => {
        paste::paste! {
            #[test]
            fn [<$name _test_mode>]() {
                insta::assert_snapshot!(render_for_tests_rooted($cmd, OutputMode::TestMode, &$path_fn()));
            }

            #[test]
            fn [<$name _tty_mode>]() {
                insta::assert_snapshot!(render_for_tests_rooted($cmd, OutputMode::Tty, &$path_fn()));
            }

            #[test]
            fn [<$name _plain_mode>]() {
                insta::assert_snapshot!(render_for_tests_rooted($cmd, OutputMode::Plain, &$path_fn()));
            }

            #[test]
            fn [<$name _ai_mode>]() {
                insta::assert_snapshot!(render_for_tests_rooted($cmd, OutputMode::Ai, &$path_fn()));
            }

            #[test]
            fn [<$name _interactive_mode>]() {
                let project_root = $path_fn();
                let project_root_str = project_root
                    .canonicalize()
                    .unwrap_or(project_root.clone())
                    .to_string_lossy()
                    .to_string();

                let mut settings = insta::Settings::clone_current();
                settings.add_filter(&project_root_str, "/TEST_CRATE_ROOT");
                // Strip trailing whitespace from lines containing the replaced path
                // to avoid snapshot differences due to fixed-width TUI padding
                settings.add_filter(r#"(?m)(.*TEST_CRATE_ROOT[^"]+?)\s+"$"#, r#"$1""#);
                // Normalize Rust version info to avoid daily breakage with nightly updates
                // Matches patterns like: 1.95.0-nightly	(f889772d6	2026-02-05)
                settings.add_filter(r"\d+\.\d+\.\d+-[a-z]+\s+\([a-f0-9]+\s+\d{4}-\d{2}-\d{2}\)", "RUST_VERSION");
                settings.bind(|| {
                    insta::assert_snapshot!(render_interactive_for_tests_rooted($cmd, &$path_fn()));
                });
            }
        }
    };
}

/// Macro to run the same test across all output modes, rooted at the fixture crate.
macro_rules! test_all_modes {
    ($name:ident, $cmd:expr) => {
        test_all_modes_rooted!($name, $cmd, get_fixture_crate_path);
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
    Commands::get("fixture-crate::TestStruct")
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
    Commands::get("fixture-crate::markdown_test")
);

test_all_modes!(get_trait_simple, Commands::get("crate::TestTrait"));

test_all_modes!(get_trait_complex, Commands::get("crate::ComplexTrait"));

// Prefix-resolution fixtures: every `pub use` inside `prefix_tests` has a
// `use.id` that is present in the local index (same-crate), so the iterator
// resolves via the id fast-path and never reaches the prefix-rewriting
// fallback. These snapshots lock in the currently-working same-crate behavior
// so cross-crate fixes don't accidentally regress it.
test_all_modes!(get_prefix_tests, Commands::get("crate::prefix_tests"));
test_all_modes!(
    get_prefix_tests_deep,
    Commands::get("crate::prefix_tests::deep")
);
test_all_modes!(
    get_prefix_tests_doc_deep,
    Commands::get("crate::prefix_tests::deep::DocDeep")
);
test_all_modes!(
    get_prefix_tests_glob_reexport,
    Commands::get("crate::prefix_tests::deep::glob_reexport")
);

// Cross-crate prefix fixtures live in the multi-crate workspace. crate-b
// re-exports from crate-a via every prefix shape; the `use.id` always points
// foreign, forcing the Navigator to resolve via source. These snapshots
// capture today's (partly broken) behavior: self:: and super:: prefix re-
// exports are silently dropped. When the prefix rewriter lands, these
// snapshots will gain the missing re-exports.
test_all_modes_rooted!(
    get_crate_b_root,
    Commands::get("crate_b"),
    get_test_workspace_path
);
test_all_modes_rooted!(
    get_crate_b_prefix_inner,
    Commands::get("crate_b::prefix_inner"),
    get_test_workspace_path
);
