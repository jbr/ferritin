use crate::{
    commands::Commands,
    format_context::FormatContext,
    render_context::RenderContext,
    renderer::{OutputMode, render},
    request::Request,
};
use ferritin_common::{
    Navigator, Store,
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

/// Build a navigator rooted at a given path. The Navigator must outlive the
/// `Request` that borrows it; tests typically stash it in a local then pass
/// `&navigator` to `Request::new`.
fn build_test_navigator(path: &std::path::Path) -> Navigator {
    Navigator::new(std::sync::Arc::new(
        Store::default()
            .with_local_source(LocalSource::load(path).ok())
            .with_std_source(StdSource::from_rustup()),
    ))
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
    render_with_context(command, output_mode, project_root, FormatContext::new())
}

fn render_with_context(
    command: Commands,
    output_mode: OutputMode,
    project_root: &std::path::Path,
    format_context: FormatContext,
) -> String {
    let navigator = build_test_navigator(project_root);
    let mut request = Request::new(&navigator, format_context);
    let (document, _, _) = command.execute(&mut request);
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

/// Render a command to pretty-printed `--format json` output, mirroring
/// `run_json`: `get` serializes its structural item model, every other command
/// serializes its presentation `Document` generically.
fn render_json_for_tests_rooted(command: Commands, project_root: &std::path::Path) -> String {
    let navigator = build_test_navigator(project_root);
    let mut request = Request::new(&navigator, FormatContext::new());

    let json = match command {
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
            match crate::commands::get::model(&mut request, &path, source, recursive) {
                crate::commands::get::JsonOutcome::Found {
                    model,
                    canonical_url,
                } => crate::json::to_pretty_string(&model, Some(canonical_url)),
                crate::commands::get::JsonOutcome::NotFound(not_found) => {
                    crate::json::not_found_to_pretty_string(&not_found)
                }
            }
        }
        Commands::List => {
            crate::json::list_to_pretty_string(&crate::commands::list::json_model(&request))
        }
        Commands::Search {
            limit,
            kind,
            target,
        } => {
            request
                .format_context()
                .set_filter(crate::kind::predicate(&kind));
            let (crate_, query) = crate::commands::search::parse_target(target);
            let model =
                crate::commands::search::model(&mut request, &query, limit, crate_.as_deref());
            crate::json::search_to_pretty_string(&model)
        }
        // Only `Serve`/`Schema` remain, and only when their features are on.
        #[cfg(feature = "serve")]
        other => panic!("{other:?} has no JSON test rendering"),
    };

    // Same normalization the text snapshots use (the crate path and nightly
    // version string both appear inside JSON string values).
    let project_root_str = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf())
        .to_string_lossy()
        .to_string();
    let json = json.replace(&project_root_str, "/TEST_CRATE_ROOT");
    let re =
        regex::Regex::new(r"\d+\.\d+\.\d+-[a-z]+\s+\([a-f0-9]+\s+\d{4}-\d{2}-\d{2}\)").unwrap();
    re.replace_all(&json, "RUST_VERSION").to_string()
}

fn render_interactive_for_tests_rooted(
    command: Commands,
    project_root: &std::path::Path,
) -> TestBackend {
    use crate::renderer::render_to_test_backend;

    let navigator = build_test_navigator(project_root);
    let mut request = Request::new(&navigator, FormatContext::new());
    let (document, _, _) = command.execute(&mut request);
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
            fn [<$name _agent_mode>]() {
                insta::assert_snapshot!(render_for_tests_rooted($cmd, OutputMode::Agent, &$path_fn()));
            }

            #[test]
            fn [<$name _json>]() {
                insta::assert_snapshot!(render_json_for_tests_rooted($cmd, &$path_fn()));
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

/// `--public` on the crate root: truly-private items (`private_function`,
/// the private `private_detail` module, private `use` imports) drop out, while
/// public re-exports and glob-re-exported enum variants are preserved. Filtering
/// is output-mode-independent, so TestMode alone locks the behavior.
#[test]
fn get_crate_root_public_test_mode() {
    insta::assert_snapshot!(render_with_context(
        Commands::get("crate"),
        OutputMode::TestMode,
        &get_fixture_crate_path(),
        FormatContext::new().with_public(true),
    ));
}

/// `--public` on a struct: non-`pub` fields fold into the
/// "private field hidden" count rather than being listed.
#[test]
fn get_struct_public_test_mode() {
    insta::assert_snapshot!(render_with_context(
        Commands::get("crate::TestStruct"),
        OutputMode::TestMode,
        &get_fixture_crate_path(),
        FormatContext::new().with_public(true),
    ));
}

// Using macro to test across all modes
test_all_modes!(get_struct_details, Commands::get("crate::TestStruct"));

test_all_modes!(
    get_struct_with_source,
    Commands::get("crate::TestStruct").with_source()
);

test_all_modes!(get_union, Commands::get("crate::TestUnion"));

test_all_modes!(get_negative_impls, Commands::get("crate::NotThreadSafe"));

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

// Resolving and rendering individual trait-declared associated items
// (method, assoc type, assoc const) directly by path.
test_all_modes!(
    get_trait_method,
    Commands::get("crate::TestTrait::test_method")
);

// A marker trait with two bound-free implementors — exercises the compact
// (comma-separated) implementors list.
test_all_modes!(get_trait_marker, Commands::get("crate::Marker"));

test_all_modes!(get_trait_assoc_type, Commands::get("crate::TestTrait::T"));

test_all_modes!(
    get_trait_assoc_const,
    Commands::get("crate::TestTrait::ASSOCIATED_CONSTANT")
);

test_all_modes!(
    fuzzy_matching_trait_member,
    Commands::get("crate::TestTrait::test_methid")
); // typo: should suggest "test_method" and other trait members

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
// re-exports from crate-a via every prefix shape (`self::`, `super::`,
// `crate::`); each `use.id` points foreign (into crate-a). `follow_use`
// resolves them through the id/`paths` map, so they appear as resolved structs
// rather than being dropped. These snapshots guard that cross-crate resolution.
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

/// Regression for cross-crate re-exports through a *renamed* crate. crate-b
/// re-exports `crate_a::CrateAStruct` via a source-level alias
/// (`use crate_a as aliased_a`), so rustdoc records the re-export's
/// `Use::source` as `aliased_a::CrateAStruct` — a path whose leading segment is
/// not a real crate. Resolution must follow the re-export's `use.id` (which the
/// `paths` map points into the real `crate-a`) rather than trying to load a
/// crate literally named `aliased_a`. This is the same shape as quinn's `proto`
/// (= quinn_proto) / `udp` (= quinn_udp) re-exports that originally surfaced the
/// bug; pre-fix the item was silently dropped.
#[test]
fn cross_crate_aliased_reexport_resolves() {
    let output = render_for_tests_rooted(
        Commands::get("crate_b::AliasedCrateAStruct"),
        OutputMode::Plain,
        &get_test_workspace_path(),
    );
    assert!(
        output.contains("crate_a::CrateAStruct"),
        "aliased re-export should resolve into crate-a, not load a crate named \
         `aliased_a`; got:\n{output}"
    );
}
