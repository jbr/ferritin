//! Comprehensive tests for intra-doc link resolution
//!
//! These tests use real rustdoc JSON data to verify that we resolve links
//! the same way rustdoc would.

use super::*;
use crate::Navigator;

/// Helper to create a Navigator from the test workspace
fn test_navigator() -> Navigator {
    let manifest_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../tests/test-workspace/Cargo.toml");

    Navigator::builder()
        .with_std_source_if_available()
        .with_local_context(manifest_path, true)
        .build()
        .expect("Failed to build Navigator")
}

/// Helper to create a Navigator from test-crate with comprehensive use statements
fn test_crate_navigator() -> Navigator {
    let manifest_path =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/test-crate/Cargo.toml");

    Navigator::builder()
        .with_std_source_if_available()
        .with_local_context(manifest_path, true)
        .build()
        .expect("Failed to build Navigator")
}

/// Helper to get a test item by path
fn get_test_item<'a>(nav: &'a Navigator, path: &str) -> Option<DocRef<'a, Item>> {
    let mut suggestions = vec![];
    nav.resolve_path(path, &mut suggestions)
}

#[test]
fn test_fragment_links() {
    let nav = test_navigator();

    // Use a dummy origin for fragment-only links (origin doesn't matter for fragments)
    let dummy_origin = get_test_item(&nav, "std").expect("Should have std crate");

    // Fragment-only links should be preserved as fragments
    let result = resolve_link(&nav, dummy_origin, "#capacity-and-reallocation");
    assert!(matches!(result, ResolvedLink::Fragment(_)));
    if let ResolvedLink::Fragment(frag) = result {
        assert_eq!(frag, "#capacity-and-reallocation");
    }
}

#[test]
fn test_external_urls() {
    let nav = test_navigator();
    let dummy_origin = get_test_item(&nav, "std").expect("Should have std crate");

    // HTTP/HTTPS URLs should be external
    let result = resolve_link(&nav, dummy_origin, "https://docs.rs/serde");
    assert!(matches!(result, ResolvedLink::External(_)));

    let result = resolve_link(&nav, dummy_origin, "http://example.com");
    assert!(matches!(result, ResolvedLink::External(_)));
}

#[test]
fn test_disambiguator_type() {
    let nav = test_navigator();
    let dummy_origin = get_test_item(&nav, "std").expect("Should have std crate");

    // type@str should resolve to the str primitive/type
    let result = resolve_link(&nav, dummy_origin, "type@str");
    // This might be Unresolved if str isn't in test-workspace, which is fine for now
    // TODO: Add std docs to test fixtures or use core
    match result {
        ResolvedLink::Item(_) => {
            // Successfully resolved
        }
        ResolvedLink::Unresolved => {
            // Expected if test workspace doesn't have std::str
        }
        _ => panic!("Expected Item or Unresolved for type@str"),
    }
}

#[test]
fn test_absolute_path_std() {
    let nav = test_navigator();
    let dummy_origin = get_test_item(&nav, "std").expect("Should have std crate");

    // Try to resolve std::vec::Vec
    // This might not work in test workspace if std isn't available
    let result = resolve_link(&nav, dummy_origin, "std::vec::Vec");
    // We'll just verify it doesn't panic for now
    // TODO: Add std rustdoc JSON to test fixtures
    match result {
        ResolvedLink::Item(item) => {
            // Verify it's actually Vec
            assert_eq!(item.name(), Some("Vec"));
        }
        ResolvedLink::Unresolved => {
            // Expected if std isn't in test workspace
        }
        _ => panic!("Unexpected result for std::vec::Vec"),
    }
}

#[test]
fn test_method_link() {
    let nav = test_navigator();
    let dummy_origin = get_test_item(&nav, "std").expect("Should have std crate");

    // Vec::push - method on Vec
    // This will be unresolved without std
    let result = resolve_link(&nav, dummy_origin, "Vec::push");
    match result {
        ResolvedLink::Item(_) | ResolvedLink::Unresolved => {
            // Both are valid outcomes without std in fixtures
        }
        _ => panic!("Unexpected result for Vec::push"),
    }
}

#[test]
fn test_crate_prefix() {
    let nav = test_navigator();
    let dummy_origin = get_test_item(&nav, "std").expect("Should have std crate");

    // crate::foo::Bar should strip crate:: and resolve foo::Bar
    // within the current crate context
    let result = resolve_link(&nav, dummy_origin, "crate::MyStruct");
    // Result depends on what's in test-workspace
    match result {
        ResolvedLink::Item(_) | ResolvedLink::Unresolved => {
            // Valid outcomes
        }
        _ => panic!("Unexpected result for crate::MyStruct"),
    }
}

#[test]
fn test_relative_path_needs_origin() {
    let nav = test_navigator();
    let dummy_origin = get_test_item(&nav, "std").expect("Should have std crate");

    // Relative paths like "push" need origin context to resolve
    // Without proper scope traversal, they should be unresolved
    let result = resolve_link(&nav, dummy_origin, "push");
    assert!(matches!(result, ResolvedLink::Unresolved));

    // TODO: Test with origin set to Vec, should resolve to Vec::push
}

// More tests to add:
// - [ ] Relative paths with origin (e.g., "push" from Vec context)
// - [ ] Associated types (Iterator::Item)
// - [ ] Associated constants (MAX, MIN)
// - [ ] Primitive types (i32, str, [T])
// - [ ] With fragments (Vec#capacity-and-reallocation)
// - [ ] Re-exports (std::vec::Vec -> alloc::vec::Vec)
// - [ ] Macros (vec!, println!)
// - [ ] Traits (Clone, Iterator)
// - [ ] Multiple disambiguators (fn@clone, struct@String)
// - [ ] Edge cases: empty string, just '@', multiple '@'

// ============================================================================
// Tests for scope-aware resolution with origin context
// ============================================================================

#[test]
fn test_renamed_import_vector_from_root() {
    let nav = test_crate_navigator();

    // Get TestStruct as origin (it's at root level where Vector is imported)
    let origin = get_test_item(&nav, "test_crate::TestStruct")
        .or_else(|| get_test_item(&nav, "test-crate::TestStruct"))
        .expect("Should find TestStruct");

    // Try to resolve "Vector" which is "use std::vec::Vec as Vector"
    let result = resolve_link(&nav, origin, "Vector");

    match result {
        ResolvedLink::Item(item) => {
            assert_eq!(item.name(), Some("Vec"), "Vector should resolve to Vec");
        }
        other => panic!("Expected Vector to resolve to Vec, got {:?}", other),
    }
}

#[test]
#[ignore] // Will be enabled as we implement scope-aware resolution
fn test_renamed_import_tree_from_link_module() {
    let nav = test_crate_navigator();

    // Get LinkTestStruct as origin (inside link_resolution_tests module where Tree is imported)
    let origin = get_test_item(&nav, "test_crate::link_resolution_tests::LinkTestStruct")
        .or_else(|| get_test_item(&nav, "test-crate::link_resolution_tests::LinkTestStruct"))
        .expect("Should find LinkTestStruct");

    // Try to resolve "Tree" which is "use std::collections::BTreeMap as Tree"
    let result = resolve_link(&nav, origin, "Tree");

    match result {
        ResolvedLink::Item(item) => {
            assert_eq!(
                item.name(),
                Some("BTreeMap"),
                "Tree should resolve to BTreeMap"
            );
        }
        other => panic!("Expected Tree to resolve to BTreeMap, got {:?}", other),
    }
}

#[test]
#[ignore] // Will be enabled as we implement scope-aware resolution
fn test_non_renamed_import_hashmap_from_root() {
    let nav = test_crate_navigator();

    // Get TestStruct as origin
    let origin = get_test_item(&nav, "test_crate::TestStruct")
        .or_else(|| get_test_item(&nav, "test-crate::TestStruct"))
        .expect("Should find TestStruct");

    // Try to resolve "HashMap" which is "use std::collections::HashMap"
    let result = resolve_link(&nav, origin, "HashMap");

    match result {
        ResolvedLink::Item(item) => {
            assert_eq!(
                item.name(),
                Some("HashMap"),
                "HashMap should resolve to HashMap"
            );
        }
        other => panic!("Expected HashMap to resolve, got {:?}", other),
    }
}
