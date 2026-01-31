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

fn render_for_tests(command: Commands, output_mode: OutputMode) -> String {
    let request = create_test_state();
    let (document, _, _) = command.execute(&request);
    let mut output = String::new();
    let render_context = RenderContext::new().with_output_mode(output_mode);
    render(&document, &render_context, &mut output).unwrap();

    // Normalize the test crate path for consistent snapshots across environments
    let test_crate_path = get_test_crate_path();
    let test_crate_path_str = test_crate_path
        .canonicalize()
        .unwrap_or(test_crate_path)
        .to_string_lossy()
        .to_string();
    output.replace(&test_crate_path_str, "/TEST_CRATE_ROOT")
}

#[test]
fn test_get_crate_root_test_mode() {
    insta::assert_snapshot!(render_for_tests(
        Commands::get("crate"),
        OutputMode::TestMode
    ));
}

#[test]
fn test_get_crate_root_tty_mode() {
    insta::assert_snapshot!(render_for_tests(Commands::get("crate"), OutputMode::Tty));
}

#[test]
fn test_get_crate_root_plain_mode() {
    insta::assert_snapshot!(render_for_tests(Commands::get("crate"), OutputMode::Plain));
}

#[test]
fn test_get_struct_details() {
    insta::assert_snapshot!(render_for_tests(
        Commands::get("crate::TestStruct"),
        OutputMode::TestMode
    ));
}

#[test]
fn test_get_struct_with_source() {
    insta::assert_snapshot!(render_for_tests(
        Commands::get("crate::TestStruct").with_source(),
        OutputMode::TestMode
    ));
}

#[test]
fn test_get_submodule() {
    insta::assert_snapshot!(render_for_tests(
        Commands::get("crate::submodule"),
        OutputMode::TestMode
    ));
}

#[test]
fn test_get_enum_details() {
    insta::assert_snapshot!(render_for_tests(
        Commands::get("crate::submodule::TestEnum"),
        OutputMode::TestMode
    ));
}

#[test]
fn test_get_generic_enum() {
    insta::assert_snapshot!(render_for_tests(
        Commands::get("crate::GenericEnum"),
        OutputMode::TestMode
    ));
}

#[test]
fn test_nonexistent_item() {
    insta::assert_snapshot!(render_for_tests(
        Commands::get("crate::DoesNotExist"),
        OutputMode::TestMode
    ));
}

#[test]
fn test_recursive_module_listing() {
    insta::assert_snapshot!(render_for_tests(
        Commands::get("crate").recursive(),
        OutputMode::TestMode
    ));
}

#[test]
fn test_recursive_submodule_listing() {
    insta::assert_snapshot!(render_for_tests(
        Commands::get("crate::submodule").recursive(),
        OutputMode::TestMode
    ));
}

#[test]
fn test_get_item_with_normalized_crate_name() {
    insta::assert_snapshot!(render_for_tests(
        Commands::get("test-crate::TestStruct"),
        OutputMode::TestMode
    ));
}

#[test]
fn test_list_crates() {
    insta::assert_snapshot!(render_for_tests(Commands::list(), OutputMode::TestMode));
}

#[test]
fn test_search() {
    insta::assert_snapshot!(render_for_tests(
        Commands::search("trigger line-based truncation"),
        OutputMode::TestMode
    ));
}

#[test]
fn test_search_2() {
    insta::assert_snapshot!(render_for_tests(
        Commands::search("generic struct"),
        OutputMode::TestMode
    ));
}

#[test]
fn test_fuzzy_matching_typo() {
    // Try to access a trait method with a typo - should suggest correct spelling
    insta::assert_snapshot!(render_for_tests(
        Commands::get("crate::TestStruct::test_metod"), // typo: should suggest "test_method"
        OutputMode::TestMode
    ));
}

#[test]
fn test_fuzzy_matching_trait_methods() {
    // Try to access a trait method that should be available via impl
    // This tests whether we collect trait implementation methods
    insta::assert_snapshot!(render_for_tests(
        Commands::get("crate::TestStruct::cute"), // Should suggest "clone" from Clone trait
        OutputMode::TestMode
    ));
}

#[test]
fn test_fuzzy_matching_suggestions() {
    // Try to get a non-existent item that should trigger fuzzy suggestions
    insta::assert_snapshot!(render_for_tests(
        Commands::get("crate::TestStruct::incrementCount"), // typo: should be increment_count
        OutputMode::TestMode
    ));
}
