use crate::{
    filter::Filter,
    state::RustdocTools,
    tools::{GetItem, ListCrates, Search, SetWorkingDirectory},
    verbosity::Verbosity,
};
use mcplease::{traits::Tool, types::RequestContext};
use std::path::PathBuf;

/// Get the path to our test crate (fast to build, minimal dependencies)
fn get_fixture_crate_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixture-crate")
}

/// Create a test state with isolated session
fn create_test_state() -> RustdocTools {
    let mut state = RustdocTools::new(None)
        .expect("Failed to create state")
        .with_default_session_id("test");

    SetWorkingDirectory {
        path: get_fixture_crate_path().to_string_lossy().to_string(),
    }
    .execute(&mut state, &RequestContext::default())
    .unwrap();

    state
}

#[test]
fn test_get_crate_root() {
    let mut state = create_test_state();

    // Get the crate root
    let tool = GetItem {
        name: "crate".to_string(),
        ..Default::default()
    };

    let result = tool
        .execute(&mut state, &RequestContext::default())
        .expect("Tool execution failed");

    insta::assert_snapshot!(result);
}

#[test]
fn test_show_docs_vs_hide_docs_comparison() {
    let mut state = create_test_state();

    // First, get TestStruct with docs shown (default)
    let tool_with_docs = GetItem {
        name: "crate::TestStruct".to_string(),
        ..Default::default()
    };

    let result_with_docs = tool_with_docs
        .execute(&mut state, &RequestContext::default())
        .expect("Tool execution failed");

    // Then get TestStruct with docs hidden
    let tool_no_docs = GetItem {
        name: "crate::TestStruct".to_string(),
        verbosity: Some(Verbosity::Minimal),
        ..Default::default()
    };

    let result_no_docs = tool_no_docs
        .execute(&mut state, &RequestContext::default())
        .expect("Tool execution failed");

    // Verify the difference
    assert!(result_with_docs.len() > result_no_docs.len());

    // Both should contain the struct signature
    assert!(result_with_docs.contains("struct TestStruct"));
    assert!(result_no_docs.contains("struct TestStruct"));

    println!("=== WITH DOCS ({} chars) ===", result_with_docs.len());
    println!("{result_with_docs}");
    println!("\n=== WITHOUT DOCS ({} chars) ===", result_no_docs.len());
    println!("{result_no_docs}");
}

#[test]
fn test_verbosity_minimal() {
    let mut state = create_test_state();

    // Get the crate root with documentation hidden
    let tool = GetItem {
        name: "crate".to_string(),
        verbosity: Some(Verbosity::Minimal),
        ..Default::default()
    };

    let result = tool
        .execute(&mut state, &RequestContext::default())
        .expect("Tool execution failed");

    // The result should not contain documentation text
    assert!(!result.contains("Documentation:"));

    // But should still contain structure information
    assert!(result.contains("Item: fixture_crate"));
    assert!(
        result.contains("Structs:") || result.contains("Enums:") || result.contains("Functions:")
    );

    insta::assert_snapshot!(result);
}

#[test]
fn test_fuzzy_matching_tool_execute() {
    let mut state = create_test_state();

    // Try to access a trait method with a typo - should find TestTrait methods
    let tool = GetItem {
        name: "crate::TestStruct::test_metod".to_string(), // typo: should suggest "test_method"
        ..Default::default()
    };

    let result = tool
        .execute(&mut state, &RequestContext::default())
        .expect("Tool execution failed");

    insta::assert_snapshot!(result);
}
#[test]
fn test_fuzzy_matching_trait_methods() {
    let mut state = create_test_state();

    // Try to access a trait method that should be available via impl
    // This tests whether we collect trait implementation methods
    let tool = GetItem {
        name: "crate::TestStruct::cute".to_string(), // Should suggest "clone" from Clone trait
        ..Default::default()
    };

    let result = tool
        .execute(&mut state, &RequestContext::default())
        .expect("Tool execution failed");

    // Should contain suggestions from trait implementations
    assert!(result.contains("Did you mean"));
    // Should suggest trait methods that are actually available
    // TestStruct implements Clone, so "clone" should be suggested for "cute"

    insta::assert_snapshot!(result);
}

#[test]
fn test_get_struct_details() {
    let mut state = create_test_state();

    // Get TestStruct details
    let tool = GetItem {
        name: "crate::TestStruct".to_string(),
        ..Default::default()
    };

    let result = tool
        .execute(&mut state, &RequestContext::default())
        .expect("Tool execution failed");

    insta::assert_snapshot!(result);
}

#[test]
fn test_get_struct_with_source() {
    let mut state = create_test_state();

    // Get TestStruct details with source
    let tool = GetItem {
        name: "crate::TestStruct".to_string(),
        include_source: Some(true),
        ..Default::default()
    };

    let result = tool
        .execute(&mut state, &RequestContext::default())
        .expect("Tool execution failed");
    let fixture_crate_dir = state.get_context(None).unwrap().unwrap();

    // Normalize project path in source output
    let normalized_result = result.replace(
        &fixture_crate_dir.to_string_lossy().to_string(),
        "/TEST_CRATE_ROOT",
    );
    insta::assert_snapshot!(normalized_result);
}

#[test]
fn test_get_function_details() {
    let mut state = create_test_state();

    // Get test_function details with source
    let tool = GetItem {
        name: "crate::test_function".to_string(),
        include_source: Some(true),
        ..Default::default()
    };

    let result = tool
        .execute(&mut state, &RequestContext::default())
        .expect("Tool execution failed");
    let fixture_crate_dir = state.get_context(None).unwrap().unwrap();

    // Normalize project path in source output
    let normalized_result = result.replace(
        &fixture_crate_dir.to_string_lossy().to_string(),
        "/TEST_CRATE_ROOT",
    );
    insta::assert_snapshot!(normalized_result);
}

#[test]
fn test_get_submodule() {
    let mut state = create_test_state();

    // Get submodule listing
    let tool = GetItem {
        name: "crate::submodule".to_string(),
        include_source: None,
        ..Default::default()
    };

    let result = tool
        .execute(&mut state, &RequestContext::default())
        .expect("Tool execution failed");

    insta::assert_snapshot!(result);
}

#[test]
fn test_get_enum_details() {
    let mut state = create_test_state();

    // Get TestEnum from submodule
    let tool = GetItem {
        name: "crate::submodule::TestEnum".to_string(),
        ..Default::default()
    };

    let result = tool
        .execute(&mut state, &RequestContext::default())
        .expect("Tool execution failed");

    insta::assert_snapshot!(result);
}

#[test]
fn test_get_generic_struct() {
    let mut state = create_test_state();

    // Get GenericStruct details
    let tool = GetItem {
        name: "crate::GenericStruct".to_string(),
        ..Default::default()
    };

    let result = tool
        .execute(&mut state, &RequestContext::default())
        .expect("Tool execution failed");

    insta::assert_snapshot!(result);
}

#[test]
fn test_get_generic_function() {
    let mut state = create_test_state();

    // Get generic_function details
    let tool = GetItem {
        name: "crate::generic_function".to_string(),
        ..Default::default()
    };

    let result = tool
        .execute(&mut state, &RequestContext::default())
        .expect("Tool execution failed");

    insta::assert_snapshot!(result);
}

#[test]
fn test_get_constants() {
    let mut state = create_test_state();

    // Get constant
    let tool = GetItem {
        name: "crate::TEST_CONSTANT".to_string(),
        ..Default::default()
    };

    let result = tool
        .execute(&mut state, &RequestContext::default())
        .expect("Tool execution failed");

    insta::assert_snapshot!(result);
}

#[test]
fn test_get_struct_with_private_fields() {
    let mut state = create_test_state();

    // Get GenericStruct to see hidden field indicator
    let tool = GetItem {
        name: "crate::GenericStruct".to_string(),
        ..Default::default()
    };

    let result = tool
        .execute(&mut state, &RequestContext::default())
        .expect("Tool execution failed");

    insta::assert_snapshot!(result);
}

#[test]
fn test_fuzzy_matching_suggestions() {
    let mut state = create_test_state();

    // Try to get a non-existent item that should trigger fuzzy suggestions
    let tool = GetItem {
        name: "crate::TestStruct::incrementCount".to_string(), // typo: should be increment_count
        ..Default::default()
    };

    let result = tool
        .execute(&mut state, &RequestContext::default())
        .expect("Tool execution failed");

    // Should contain suggestions
    assert!(result.contains("Did you mean"));
    assert!(result.contains("increment_count"));

    insta::assert_snapshot!(result);
}
#[test]
fn test_nonexistent_item() {
    let mut state = create_test_state();

    // Try to get a nonexistent item
    let tool = GetItem {
        name: "crate::DoesNotExist".to_string(),
        include_source: None,
        ..Default::default()
    };

    let result = tool
        .execute(&mut state, &RequestContext::default())
        .expect("Tool execution failed");

    insta::assert_snapshot!(result);
}

#[test]
fn test_get_unit_struct() {
    let mut state = create_test_state();

    // Get unit struct details
    let tool = GetItem {
        name: "crate::UnitStruct".to_string(),
        ..Default::default()
    };

    let result = tool
        .execute(&mut state, &RequestContext::default())
        .expect("Tool execution failed");

    insta::assert_snapshot!(result);
}

#[test]
fn test_get_tuple_struct() {
    let mut state = create_test_state();

    // Get tuple struct details
    let tool = GetItem {
        name: "crate::TupleStruct".to_string(),
        ..Default::default()
    };

    let result = tool
        .execute(&mut state, &RequestContext::default())
        .expect("Tool execution failed");

    insta::assert_snapshot!(result);
}

#[test]
fn test_get_generic_enum() {
    let mut state = create_test_state();

    // Get generic enum details
    let tool = GetItem {
        name: "crate::GenericEnum".to_string(),
        ..Default::default()
    };

    let result = tool
        .execute(&mut state, &RequestContext::default())
        .expect("Tool execution failed");

    insta::assert_snapshot!(result);
}

#[test]
fn test_get_trait_details() {
    let mut state = create_test_state();

    // Get TestTrait details
    let tool = GetItem {
        name: "crate::TestTrait".to_string(),
        ..Default::default()
    };

    let result = tool
        .execute(&mut state, &RequestContext::default())
        .expect("Tool execution failed");

    insta::assert_snapshot!(result);
}

#[test]
fn test_recursive_module_listing() {
    let mut state = create_test_state();

    // Get recursive listing of the crate root
    let tool = GetItem {
        name: "crate".to_string(),
        recursive: Some(true),
        ..Default::default()
    };

    let result = tool
        .execute(&mut state, &RequestContext::default())
        .expect("Tool execution failed");

    insta::assert_snapshot!(result);
}

#[test]
fn test_recursive_submodule_listing() {
    let mut state = create_test_state();

    // Get recursive listing of a submodule
    let tool = GetItem {
        name: "crate::submodule".to_string(),
        recursive: Some(true),
        ..Default::default()
    };

    let result = tool
        .execute(&mut state, &RequestContext::default())
        .expect("Tool execution failed");

    insta::assert_snapshot!(result);
}

#[test]
fn test_recursive_filtering() {
    let mut state = create_test_state();

    // Get recursive listing with struct filter only
    let tool = GetItem {
        name: "crate".to_string(),
        recursive: Some(true),
        filter: Some(vec![Filter::Struct]),
        ..Default::default()
    };

    let result = tool
        .execute(&mut state, &RequestContext::default())
        .expect("Tool execution failed");

    insta::assert_snapshot!(result);
}

#[test]
fn test_non_recursive_filtering() {
    let mut state = create_test_state();
    // Get non-recursive listing with struct filter
    let tool = GetItem {
        name: "crate".to_string(),
        filter: Some(vec![Filter::Struct]),
        ..Default::default()
    };

    let result = tool
        .execute(&mut state, &RequestContext::default())
        .expect("Tool execution failed");

    insta::assert_snapshot!(result);
}

#[test]
fn test_recursive_multiple_filters() {
    let mut state = create_test_state();

    // Get recursive listing with function and trait filters
    let tool = GetItem {
        name: "crate".to_string(),
        recursive: Some(true),
        filter: Some(vec![Filter::Function, Filter::Trait]),
        ..Default::default()
    };

    let result = tool
        .execute(&mut state, &RequestContext::default())
        .expect("Tool execution failed");

    insta::assert_snapshot!(result);
}

#[test]
fn test_get_std_vec() {
    let mut state = create_test_state();

    // Get the root of the std crate
    let tool_std_root = GetItem {
        name: "std".to_string(),
        ..Default::default()
    };
    let result_std_root = tool_std_root
        .execute(&mut state, &RequestContext::default())
        .expect("Tool execution failed for std root");
    insta::assert_snapshot!(result_std_root);

    // Get std::collections::HashMap
    let tool_std_collections_hashmap = GetItem {
        name: "std::collections::HashMap".to_string(),
        ..Default::default()
    };
    let result_std_collections_hashmap = tool_std_collections_hashmap
        .execute(&mut state, &RequestContext::default())
        .expect("Tool execution failed for std::collections::HashMap");
    insta::assert_snapshot!(result_std_collections_hashmap);

    // Get std::vec::Vec
    let tool_std_vec_vec = GetItem {
        name: "std::vec::Vec".to_string(),
        ..Default::default()
    };
    let result_std_vec_vec = tool_std_vec_vec
        .execute(&mut state, &RequestContext::default())
        .expect("Tool execution failed for std::vec::Vec");
    insta::assert_snapshot!(result_std_vec_vec);
}
#[test]
fn test_get_item_with_normalized_crate_name() {
    let mut state = create_test_state();

    // Get an item from the fixture-crate using a hyphen in the name
    let tool = GetItem {
        name: "fixture-crate::TestStruct".to_string(),
        ..Default::default()
    };

    let result = tool
        .execute(&mut state, &RequestContext::default())
        .expect("Tool execution failed");

    insta::assert_snapshot!(result);
}
#[test]
fn test_get_complex_trait_details() {
    let mut state = create_test_state();

    // Get ComplexTrait details
    let tool = GetItem {
        name: "crate::ComplexTrait".to_string(),
        ..Default::default()
    };

    let result = tool
        .execute(&mut state, &RequestContext::default())
        .expect("Tool execution failed");

    insta::assert_snapshot!(result);
}

#[test]
fn tools_doesnt_panic() {
    use crate::tools::Tools;
    use mcplease::traits::AsToolsList;
    Tools::tools_list();
}

#[test]
fn list_crates() {
    let mut state = create_test_state();
    let result = ListCrates::default()
        .execute(&mut state, &RequestContext::default())
        .unwrap();
    insta::assert_snapshot!(result);
}

#[test]
fn search() {
    let mut state = create_test_state();
    let result = Search {
        crate_name: "crate".into(),
        query: "trigger line-based truncation".into(),
        limit: None,
    }
    .execute(&mut state, &RequestContext::default())
    .unwrap();
    insta::assert_snapshot!(result);
}

#[test]
fn search_2() {
    let mut state = create_test_state();
    let result = Search {
        crate_name: "crate".into(),
        query: "generic struct".into(),
        limit: None,
    }
    .execute(&mut state, &RequestContext::default())
    .unwrap();
    insta::assert_snapshot!(result);
}

/// The generics fixture module (`tests/fixture-crate/src/generics.rs`) rendered
/// signature-only into one snapshot, mirroring `ferritin`'s `generics_signatures`.
/// This crate carries its own string-based copy of the signature formatters, so
/// the shapes rustdoc records indirectly — synthetic `impl Trait` parameters,
/// higher-ranked binders, a `dyn` object's own lifetime — have to be fixed and
/// pinned here separately.
#[test]
fn generics_signatures() {
    let mut state = create_test_state();
    let rendered = [
        "crate::generics::set_data",
        "crate::generics::mixed_params",
        "crate::generics::two_impls",
        "crate::generics::nested_impl_trait",
        "crate::generics::impl_and_where",
        "crate::generics::returns_impl_trait",
        "crate::generics::precise_capturing",
        "crate::generics::impl_trait_outlives",
        "crate::generics::hrtb_inline",
        "crate::generics::hrtb_where",
        "crate::generics::hrtb_dyn",
        "crate::generics::hrtb_fn_pointer",
        "crate::generics::dyn_with_lifetime",
        "crate::generics::dyn_needs_parens",
        "crate::generics::dyn_parenthesized_args",
        "crate::generics::maybe_sized",
        "crate::generics::lifetime_outlives",
        "crate::generics::assoc_equality",
        "crate::generics::assoc_bound",
        "crate::generics::qualified_return",
        "crate::generics::ConstGeneric",
        "crate::generics::ManyPredicates",
        "crate::generics::TupleWithWhere",
        "crate::generics::EnumManyPredicates",
        "crate::generics::ImplTraitMethods",
    ]
    .into_iter()
    .map(|name| {
        GetItem {
            name: name.to_string(),
            verbosity: Some(Verbosity::Minimal),
            ..Default::default()
        }
        .execute(&mut state, &RequestContext::default())
        .expect("Tool execution failed")
    })
    .collect::<Vec<_>>()
    .join("\n");

    insta::assert_snapshot!(rendered);
}
