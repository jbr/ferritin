use crate::{
    state::RustdocTools,
    tools::{GetItem, ListCrates, SetWorkingDirectory},
};
use mcplease::traits::Tool;
use std::path::PathBuf;

/// Get the path to our test workspace
fn get_test_workspace_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/test-workspace")
}

/// Create a test state with workspace context
fn create_workspace_test_state() -> RustdocTools {
    let mut state = RustdocTools::new(None)
        .expect("Failed to create state")
        .with_default_session_id("workspace_test");

    SetWorkingDirectory {
        path: get_test_workspace_path().to_string_lossy().to_string(),
    }
    .execute(&mut state)
    .unwrap();

    state
}

#[test]
fn test_workspace_list_crates_with_dependencies() {
    let mut state = create_workspace_test_state();

    let result = ListCrates::default().execute(&mut state).unwrap();

    // Should include workspace members (no longer aliased in workspaces)
    assert!(result.contains("• crate-a (workspace-local)"));
    assert!(result.contains("• crate-b (workspace-local)"));
    assert!(!result.contains("aliased as \"crate\""));

    // Should include external dependencies from all workspace members
    assert!(result.contains("• serde "));
    assert!(result.contains("• anyhow "));
    assert!(result.contains("• regex "));
    assert!(result.contains("• log "));

    // Should include dev dependencies
    assert!(result.contains("• tempfile ") && result.contains("(dev-dep)"));
    assert!(result.contains("• env_logger ") && result.contains("(dev-dep)"));

    // Should NOT include workspace-internal dependencies
    assert!(!result.contains("• crate-a") || !result.contains(" 0.1.0")); // crate-a shouldn't be listed as external dep

    insta::assert_snapshot!(result);
}
#[test]
fn test_workspace_member_parameter_crate_a() {
    let mut state = create_workspace_test_state();

    let result = ListCrates {
        workspace_member: Some("crate-a".to_string()),
        for_schemars: (),
    }
    .execute(&mut state)
    .unwrap();

    // Should show both workspace members (for reference)
    assert!(result.contains("• crate-a (workspace-local)"));
    assert!(result.contains("• crate-b (workspace-local)"));
    assert!(!result.contains("aliased as \"crate\""));

    // Should show crate-a's dependencies only
    assert!(result.contains("• serde ")); // crate-a dependency
    assert!(result.contains("• regex ")); // crate-a dependency
    assert!(result.contains("• tempfile ") && result.contains("(dev-dep)")); // crate-a dev-dep

    // Should NOT show crate-b's dependencies
    assert!(!result.contains("• anyhow ")); // crate-b dependency
    assert!(!result.contains("• log ")); // crate-b dependency
    assert!(!result.contains("• env_logger ")); // crate-b dev-dep

    insta::assert_snapshot!(result);
}

#[test]
fn test_workspace_member_parameter_crate_b() {
    let mut state = create_workspace_test_state();

    let result = ListCrates {
        workspace_member: Some("crate-b".to_string()),
        for_schemars: (),
    }
    .execute(&mut state)
    .unwrap();

    // Should show both workspace members (for reference)
    assert!(result.contains("• crate-a (workspace-local)"));
    assert!(result.contains("• crate-b (workspace-local)"));
    assert!(!result.contains("aliased as \"crate\""));

    // Should show crate-b's dependencies only
    assert!(result.contains("• anyhow ")); // crate-b dependency
    assert!(result.contains("• log ")); // crate-b dependency
    assert!(result.contains("• env_logger ") && result.contains("(dev-dep)")); // crate-b dev-dep

    // Should NOT show crate-a's dependencies
    assert!(!result.contains("• serde ")); // crate-a dependency
    assert!(!result.contains("• regex ")); // crate-a dependency
    assert!(!result.contains("• tempfile ")); // crate-a dev-dep

    insta::assert_snapshot!(result);
}

#[test]
fn test_workspace_member_parameter_vs_working_directory() {
    // Test that workspace_member parameter overrides working directory context
    let mut state = create_subcrate_test_state(); // Working in crate-a directory

    let result = ListCrates {
        workspace_member: Some("crate-b".to_string()), // But request crate-b scope
        for_schemars: (),
    }
    .execute(&mut state)
    .unwrap();

    // Should show crate-b's dependencies (from parameter) not crate-a's (from working dir)
    assert!(result.contains("• anyhow ")); // crate-b dependency
    assert!(result.contains("• log ")); // crate-b dependency
    assert!(!result.contains("• serde ")); // crate-a dependency (should be excluded)
    assert!(!result.contains("• regex ")); // crate-a dependency (should be excluded)

    insta::assert_snapshot!(result);
}
/// Create a test state with subcrate working directory (crate-b)
fn create_subcrate_b_test_state() -> RustdocTools {
    let mut state = RustdocTools::new(None)
        .expect("Failed to create state")
        .with_default_session_id("subcrate_b_test");

    SetWorkingDirectory {
        path: get_test_workspace_path()
            .join("crate-b")
            .to_string_lossy()
            .to_string(),
    }
    .execute(&mut state)
    .unwrap();

    state
}

#[test]
fn test_subcrate_b_scoped_dependencies() {
    let mut state = create_subcrate_b_test_state();

    let result = ListCrates::default().execute(&mut state).unwrap();

    // Should show crate-b as workspace-local (no longer aliased in workspaces)
    assert!(result.contains("• crate-b (workspace-local)"));
    assert!(!result.contains("aliased as \"crate\""));

    // Should show crate-b's dependencies only
    assert!(result.contains("• anyhow ")); // crate-b dependency
    assert!(result.contains("• log ")); // crate-b dependency
    assert!(result.contains("• env_logger ") && result.contains("(dev-dep)")); // crate-b dev-dep

    // Should NOT show crate-a's dependencies
    assert!(!result.contains("• serde ")); // crate-a dependency
    assert!(!result.contains("• regex ")); // crate-a dependency
    assert!(!result.contains("• tempfile ")); // crate-a dev-dep

    // Should still show other workspace members (for reference)
    assert!(result.contains("• crate-a (workspace-local)"));
    // No crate should be aliased in workspace contexts
    assert!(!result.contains("aliased as \"crate\""));

    insta::assert_snapshot!(result);
}

#[test]
fn test_subcrate_b_crate_alias_resolution() {
    let mut state = create_subcrate_b_test_state();

    let tool = GetItem {
        name: "crate".to_string(),
        ..Default::default()
    };

    let result = tool.execute(&mut state).expect("Tool execution failed");

    // When in crate-b directory, "crate" should refer to crate-b, not crate-a

    insta::assert_snapshot!(result);
}
/// Create a test state with subcrate working directory (crate-a)
fn create_subcrate_test_state() -> RustdocTools {
    let mut state = RustdocTools::new(None)
        .expect("Failed to create state")
        .with_default_session_id("subcrate_test");

    SetWorkingDirectory {
        path: get_test_workspace_path()
            .join("crate-a")
            .to_string_lossy()
            .to_string(),
    }
    .execute(&mut state)
    .unwrap();

    state
}

#[test]
fn test_subcrate_scoped_dependencies() {
    let mut state = create_subcrate_test_state();

    let result = ListCrates::default().execute(&mut state).unwrap();

    // Should show crate-a as workspace-local (no longer aliased in workspaces)
    assert!(result.contains("• crate-a (workspace-local)"));
    assert!(!result.contains("aliased as \"crate\""));

    // Should show crate-a's dependencies only
    assert!(result.contains("• serde ")); // crate-a dependency
    assert!(result.contains("• regex ")); // crate-a dependency
    assert!(result.contains("• tempfile ") && result.contains("(dev-dep)")); // crate-a dev-dep

    // Should NOT show crate-b's dependencies
    assert!(!result.contains("• anyhow ")); // crate-b dependency
    assert!(!result.contains("• log ")); // crate-b dependency
    assert!(!result.contains("• env_logger ")); // crate-b dev-dep

    // Should still show other workspace members (for reference)
    assert!(result.contains("• crate-b (workspace-local)"));

    insta::assert_snapshot!(result);
}

#[test]
fn test_subcrate_crate_alias_resolution() {
    let mut state = create_subcrate_test_state();

    let tool = GetItem {
        name: "crate".to_string(),
        ..Default::default()
    };

    let result = tool.execute(&mut state).expect("Tool execution failed");

    // Note: Currently "crate" alias resolution has some limitations in subcrate context
    // This test documents the current behavior - the alias is set correctly in list_crates
    // but resolution when accessing items may need rustdoc files to be built

    insta::assert_snapshot!(result);
}

#[test]
fn test_workspace_get_crate_a() {
    let mut state = create_workspace_test_state();

    let tool = GetItem {
        name: "crate-a".to_string(),
        ..Default::default()
    };

    let result = tool.execute(&mut state).expect("Tool execution failed");

    // Should contain crate-a items
    assert!(result.contains("CrateAStruct"));
    assert!(result.contains("process_data"));

    insta::assert_snapshot!(result);
}

#[test]
fn test_workspace_get_crate_b() {
    let mut state = create_workspace_test_state();

    let tool = GetItem {
        name: "crate-b".to_string(),
        ..Default::default()
    };

    let result = tool.execute(&mut state).expect("Tool execution failed");

    // Should contain crate-b items
    assert!(result.contains("CrateBProcessor"));

    insta::assert_snapshot!(result);
}

#[test]
fn test_workspace_access_dependency() {
    let mut state = create_workspace_test_state();

    // Try to access a dependency that should be available
    let tool = GetItem {
        name: "serde".to_string(),
        ..Default::default()
    };

    let result = tool.execute(&mut state).expect("Tool execution failed");

    // Should be able to access serde documentation
    assert!(result.contains("serde"));

    insta::assert_snapshot!(result);
}
