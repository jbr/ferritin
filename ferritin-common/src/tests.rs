use rustdoc_types::ItemKind;
use std::path::PathBuf;

use crate::{
    CrateName, Navigator, RustdocData,
    sources::{CrateProvenance, LocalSource, StdSource},
};

fn get_fixture_crate_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixture-crate")
}

fn test_navigator() -> Navigator {
    Navigator::default()
        .with_local_source(LocalSource::load(&get_fixture_crate_path()).ok())
        .with_std_source(StdSource::from_rustup())
}

/// Resolve a path, panicking with a helpful message on failure.
fn resolve<'a>(nav: &'a Navigator, path: &str) -> crate::DocRef<'a, rustdoc_types::Item> {
    nav.resolve_path(path, &mut vec![])
        .unwrap_or_else(|| panic!("failed to resolve {path:?}"))
}

/// Check that `discriminated_path()` produces the expected string.
#[test]
fn discriminated_path_values() {
    let nav = test_navigator();
    // The crate name in discriminated_path uses crate_docs().name() which matches the
    // Cargo.toml name ("fixture-crate" with dashes, not underscores).
    let cases = [
        ("crate::TestStruct", "fixture-crate::struct@TestStruct"),
        ("crate::TestTrait", "fixture-crate::trait@TestTrait"),
        ("crate::test_function", "fixture-crate::fn@test_function"),
        ("crate::submodule", "fixture-crate::mod@submodule"),
        ("crate::TEST_CONSTANT", "fixture-crate::const@TEST_CONSTANT"),
        ("crate::TEST_STATIC", "fixture-crate::static@TEST_STATIC"),
        ("crate::GenericEnum", "fixture-crate::enum@GenericEnum"),
        (
            "crate::namespace_collisions",
            "fixture-crate::mod@namespace_collisions",
        ),
    ];

    for (path, expected_disc) in cases {
        let item = resolve(&nav, path);
        let disc = item
            .discriminated_path()
            .unwrap_or_else(|| panic!("no discriminated_path for {path:?}"));
        assert_eq!(disc, expected_disc, "wrong discriminated_path for {path:?}");
    }
}

/// Check that `discriminated_path()` → `resolve_path()` returns the same item.
#[test]
fn discriminated_path_round_trips() {
    let nav = test_navigator();
    let paths = [
        "crate::TestStruct",
        "crate::TestTrait",
        "crate::test_function",
        "crate::submodule",
        "crate::TEST_CONSTANT",
        "crate::GenericEnum",
        "crate::submodule::SubStruct",
        "crate::namespace_collisions",
    ];

    for path in paths {
        let item = resolve(&nav, path);
        let disc_path = item
            .discriminated_path()
            .unwrap_or_else(|| panic!("no discriminated_path for {path:?}"));
        let round_tripped = nav
            .resolve_path(&disc_path, &mut vec![])
            .unwrap_or_else(|| panic!("discriminated path {disc_path:?} failed to resolve"));
        assert_eq!(
            item, round_tripped,
            "round-trip mismatch for {path:?} (discriminated: {disc_path:?})"
        );
    }
}

/// Methods have no `ItemSummary` in rustdoc's `paths` map (rust-lang/rust#152511), so
/// `discriminated_path()` falls back to the `parent` set during tree traversal.
#[test]
fn discriminated_path_round_trips_method() {
    let nav = test_navigator();
    let item = resolve(&nav, "crate::submodule::SubStruct::new");
    let disc_path = item
        .discriminated_path()
        .expect("discriminated_path should work for methods once the upstream bug is fixed");
    let round_tripped = nav
        .resolve_path(&disc_path, &mut vec![])
        .unwrap_or_else(|| panic!("discriminated path {disc_path:?} failed to resolve"));
    assert_eq!(item, round_tripped);
}

/// A discriminator prefix selects the right item when a module and function share a name.
#[test]
fn discriminator_resolves_module_function_collision() {
    let nav = test_navigator();

    let by_mod = resolve(&nav, "crate::namespace_collisions::mod@both");
    let by_fn = resolve(&nav, "crate::namespace_collisions::fn@both");

    assert_eq!(
        by_mod.kind(),
        ItemKind::Module,
        "mod@both should be a module"
    );
    assert_eq!(
        by_fn.kind(),
        ItemKind::Function,
        "fn@both should be a function"
    );
    assert_ne!(
        by_mod, by_fn,
        "mod@both and fn@both should be different items"
    );
}

/// A discriminated path round-trips for both sides of a module-function collision.
#[test]
fn discriminated_path_round_trips_through_collision() {
    let nav = test_navigator();

    for disc_path in [
        "fixture-crate::namespace_collisions::mod@both",
        "fixture-crate::namespace_collisions::fn@both",
    ] {
        let item = nav
            .resolve_path(disc_path, &mut vec![])
            .unwrap_or_else(|| panic!("failed to resolve {disc_path:?}"));
        let generated = item
            .discriminated_path()
            .unwrap_or_else(|| panic!("no discriminated_path for {disc_path:?}"));
        assert_eq!(
            generated, disc_path,
            "discriminated_path should reproduce the qualified path"
        );
    }
}

/// A method on a struct in a private module round-trips through `discriminated_path`.
///
/// This is the hardest combined case: the method is absent from rustdoc's `paths` map
/// (rust-lang/rust#152511), and its parent struct's `ItemSummary::path` passes through
/// a private module, so tree traversal alone cannot anchor the parent either.
/// Resolution must use the path_to_id index to find the parent, then traverse into
/// the method via `find_children_recursive`.
#[test]
fn discriminated_path_round_trips_method_on_private_module_struct() {
    let nav = test_navigator();

    // Resolve via the public re-export path to get a DocRef with parent set.
    let struct_item = resolve(&nav, "crate::ReachableViaPrivateModule");
    let method = struct_item
        .child_items()
        .find(|c| c.name() == Some("private_module_method"))
        .expect("private_module_method not found");

    let disc = method
        .discriminated_path()
        .expect("discriminated_path should work: parent was set during child_items traversal");

    // The path goes through the private module because that's where ItemSummary::path points.
    assert_eq!(
        disc,
        "fixture-crate::private_detail::ReachableViaPrivateModule::fn@private_module_method"
    );

    let round_tripped = nav
        .resolve_path(&disc, &mut vec![])
        .unwrap_or_else(|| panic!("failed to resolve discriminated path {disc:?}"));

    assert_eq!(method, round_tripped);
}

/// Items that live behind a private module are reachable via the path_to_id fallback.
#[test]
fn private_module_path_resolves_via_index() {
    let nav = test_navigator();

    // The public re-export is reachable via normal tree traversal.
    let via_reexport = resolve(&nav, "crate::ReachableViaPrivateModule");

    // The path through the private module fails tree traversal but succeeds via path_to_id.
    let via_private_path = resolve(
        &nav,
        "fixture-crate::private_detail::ReachableViaPrivateModule",
    );

    assert_eq!(via_reexport.kind(), ItemKind::Struct, "should be a struct");
    // Both paths should land on the same underlying item.
    assert_eq!(
        via_reexport, via_private_path,
        "re-export and private-module path should resolve to the same item"
    );
}

/// A failing `pub use` must not abort iteration of its module's remaining children.
///
/// Regression guard: previously, if any one `Use` in a module's `items` list could not
/// be resolved (neither `use.id` in the local index nor `resolve_path(&use.source)`),
/// `IdIter` short-circuited via `?` and yielded nothing further. That silently dropped
/// every sibling after the broken re-export. For example, in `trillium_server_common`
/// the first root-level `pub use futures_lite::AsyncRead` failed to resolve, hiding
/// `ServerHandle` and every other subsequent re-export.
#[test]
fn iterator_skips_unresolvable_use_items() {
    use rustdoc_types::{
        Crate, Generics, Id, Item, ItemEnum, Module, Struct, StructKind, Target, Use, Visibility,
    };
    use std::collections::HashMap;

    fn item(id: u32, name: Option<&str>, inner: ItemEnum) -> Item {
        Item {
            id: Id(id),
            crate_id: 0,
            name: name.map(str::to_owned),
            span: None,
            visibility: Visibility::Public,
            docs: None,
            links: Default::default(),
            attrs: vec![],
            deprecation: None,
            inner,
        }
    }

    fn unit_struct() -> ItemEnum {
        ItemEnum::Struct(Struct {
            kind: StructKind::Unit,
            generics: Generics {
                params: vec![],
                where_predicates: vec![],
            },
            impls: vec![],
        })
    }

    // Root module contains, in order:
    //   [broken_use, valid_struct_a, another_broken_use, valid_struct_b]
    // The two broken uses point at ids not present in the index and at source paths
    // that Navigator cannot resolve (no such crates exist / loaded). A correct
    // iterator skips them and still yields both valid structs.
    let root_id = Id(1);
    let broken_use_a = Id(10);
    let valid_struct_a = Id(11);
    let broken_use_b = Id(12);
    let valid_struct_b = Id(13);

    let mut index: HashMap<Id, Item, rustc_hash::FxBuildHasher> = Default::default();
    index.insert(
        root_id,
        item(
            root_id.0,
            Some("fake_crate"),
            ItemEnum::Module(Module {
                is_crate: true,
                items: vec![broken_use_a, valid_struct_a, broken_use_b, valid_struct_b],
                is_stripped: false,
            }),
        ),
    );
    index.insert(
        broken_use_a,
        item(
            broken_use_a.0,
            None,
            ItemEnum::Use(Use {
                source: "___definitely_not_a_real_crate___::Thing".into(),
                name: "Thing".into(),
                id: Some(Id(9_999)), // deliberately not in the index
                is_glob: false,
            }),
        ),
    );
    index.insert(
        valid_struct_a,
        item(valid_struct_a.0, Some("KeepA"), unit_struct()),
    );
    index.insert(
        broken_use_b,
        item(
            broken_use_b.0,
            None,
            ItemEnum::Use(Use {
                source: "___also_not_a_real_crate___::Other".into(),
                name: "Other".into(),
                id: Some(Id(9_998)),
                is_glob: false,
            }),
        ),
    );
    index.insert(
        valid_struct_b,
        item(valid_struct_b.0, Some("KeepB"), unit_struct()),
    );

    let crate_data = Crate {
        root: root_id,
        crate_version: None,
        includes_private: false,
        index,
        paths: Default::default(),
        external_crates: Default::default(),
        target: Target {
            triple: String::new(),
            target_features: vec![],
        },
        format_version: rustdoc_types::FORMAT_VERSION,
    };

    let data = RustdocData {
        crate_data,
        name: "fake_crate".into(),
        provenance: CrateProvenance::DocsRs,
        fs_path: PathBuf::new(),
        version: None,
        path_to_id: HashMap::new(),
    };

    // Build a Navigator with no real sources; load_crate is stubbed by pre-populating
    // working_set. resolve_path of the broken sources will try to look them up via
    // lookup_crate, which with no sources configured returns None — exactly the
    // unresolvable case we want to exercise.
    let nav = Navigator::default();
    nav.working_set
        .insert(CrateName::from("fake_crate"), Box::new(Some(data)));

    let root = nav
        .load_crate("fake_crate", &semver::VersionReq::STAR)
        .expect("pre-populated crate should be loadable")
        .root_item(&nav);

    let names: Vec<&str> = root.child_items().filter_map(|c| c.name()).collect();

    assert_eq!(
        names,
        vec!["KeepA", "KeepB"],
        "unresolvable `Use` items should be skipped, not terminate the iterator"
    );
}
