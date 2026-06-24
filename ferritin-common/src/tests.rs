use rustdoc_types::ItemKind;
use std::path::PathBuf;

use crate::{
    CrateName, Navigator, Resolver, RustdocData,
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
    Resolver::new(nav)
        .resolve_path(path, &mut vec![])
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
        let round_tripped = Resolver::new(&nav)
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
    let round_tripped = Resolver::new(&nav)
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
        let item = Resolver::new(&nav)
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
    let mut resolver = Resolver::new(&nav);
    let struct_item = resolver
        .resolve_path("crate::ReachableViaPrivateModule", &mut vec![])
        .unwrap_or_else(|| panic!("failed to resolve crate::ReachableViaPrivateModule"));
    let method = resolver
        .children(struct_item)
        .into_iter()
        .find(|c| c.name() == Some("private_module_method"))
        .expect("private_module_method not found");

    let disc = method
        .discriminated_path()
        .expect("discriminated_path should work: parent was set during child traversal");

    // The path goes through the private module because that's where ItemSummary::path points.
    assert_eq!(
        disc,
        "fixture-crate::private_detail::ReachableViaPrivateModule::fn@private_module_method"
    );

    let round_tripped = resolver
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

/// Build a minimal `RustdocData` directly for use in synthetic iterator tests.
///
/// The returned crate has `name` as both the Cargo crate name and the first segment
/// of every item path, so it round-trips through `resolve_path`. `modules` and
/// `items` are keyed by `Id`; IDs in `items` should not collide with module IDs.
#[cfg(test)]
fn synth_crate(
    name: &str,
    root_id: u32,
    modules: Vec<(u32, Option<&str>, Vec<u32>)>,
    items: Vec<rustdoc_types::Item>,
) -> RustdocData {
    use rustdoc_types::{Crate, Id, Item, ItemEnum, ItemSummary, Module, Target, Visibility};
    use std::collections::HashMap;

    let mut index: HashMap<Id, Item, rustc_hash::FxBuildHasher> = Default::default();
    let mut paths: HashMap<Id, ItemSummary, rustc_hash::FxBuildHasher> = Default::default();
    for (id, mod_name, children) in modules {
        let ids: Vec<Id> = children.into_iter().map(Id).collect();
        index.insert(
            Id(id),
            Item {
                id: Id(id),
                crate_id: 0,
                name: mod_name.map(str::to_owned),
                span: None,
                visibility: Visibility::Public,
                docs: None,
                links: Default::default(),
                attrs: vec![],
                deprecation: None,
                stability: None,
                inner: ItemEnum::Module(Module {
                    is_crate: id == root_id,
                    items: ids,
                    is_stripped: false,
                }),
            },
        );
        let summary_path = if id == root_id {
            vec![name.to_string()]
        } else {
            vec![
                name.to_string(),
                mod_name.expect("non-root module needs a name").to_string(),
            ]
        };
        paths.insert(
            Id(id),
            ItemSummary {
                crate_id: 0,
                path: summary_path,
                kind: rustdoc_types::ItemKind::Module,
            },
        );
    }
    for item in items {
        index.insert(item.id, item);
    }

    let crate_data = Crate {
        root: Id(root_id),
        crate_version: None,
        includes_private: false,
        index,
        paths,
        external_crates: Default::default(),
        target: Target {
            triple: String::new(),
            target_features: vec![],
        },
        format_version: rustdoc_types::FORMAT_VERSION,
    };
    RustdocData::from_crate(
        crate_data,
        name.to_owned(),
        CrateProvenance::DocsRs,
        PathBuf::new(),
        None,
    )
}

#[cfg(test)]
fn synth_item(id: u32, name: Option<&str>, inner: rustdoc_types::ItemEnum) -> rustdoc_types::Item {
    use rustdoc_types::{Id, Item, Visibility};
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
        stability: None,
    }
}

#[cfg(test)]
fn synth_unit_struct() -> rustdoc_types::ItemEnum {
    use rustdoc_types::{Generics, ItemEnum, Struct, StructKind};
    ItemEnum::Struct(Struct {
        kind: StructKind::Unit,
        generics: Generics {
            params: vec![],
            where_predicates: vec![],
        },
        impls: vec![],
    })
}

#[cfg(test)]
fn synth_use(id: u32, name: &str, source: &str, target: Option<u32>) -> rustdoc_types::Item {
    use rustdoc_types::{ItemEnum, Use};
    synth_item(
        id,
        None,
        ItemEnum::Use(Use {
            source: source.to_owned(),
            name: name.to_owned(),
            id: target.map(rustdoc_types::Id),
            is_glob: false,
        }),
    )
}

/// Build the two-crate test setup used by the prefix-resolution tests below.
///
/// Layout:
/// ```text
/// target_crate::TargetStruct   (id 100 in target_crate)
/// home_crate
///   RootTarget                 (struct, id 50 — anchor for crate::/self:: at root)
///   BaseAlias                  (use → target_crate::TargetStruct, id=100 foreign)
///   AbsAlias                   (use → target_crate::TargetStruct, id=100 foreign)
///   CrateAlias                 (use → crate::RootTarget, id=100 foreign)
///   SelfAlias                  (use → self::RootTarget, id=100 foreign)
///   SuperAlias                 (use → super::RootTarget, id=100 foreign) — at root, no parent
///   inner
///     InnerTarget              (struct, id 60 — anchor for self:: in `inner`)
///     AbsAliasInner            (use → target_crate::TargetStruct)
///     CrateAliasInner          (use → crate::RootTarget)
///     SelfAliasInner           (use → self::InnerTarget)
///     SuperAliasInner          (use → super::RootTarget)
/// ```
///
/// Every `use` below has `use.id = 100`, which is not in home_crate's local
/// index — forcing `IdIter` to fall through to `Navigator::resolve_path(&source)`.
/// Anchor structs (RootTarget, InnerTarget) let the `crate::` / `self::` /
/// `super::` rewrites resolve to a real item without iterating the uses
/// themselves (which would cycle).
#[cfg(test)]
fn build_prefix_test_navigator() -> Navigator {
    use rustdoc_types::{Id, ItemSummary};

    let mut target = synth_crate(
        "target_crate",
        1,
        vec![(1, None, vec![100])],
        vec![synth_item(100, Some("TargetStruct"), synth_unit_struct())],
    );
    target.insert_path_for_test(
        Id(100),
        ItemSummary {
            crate_id: 0,
            path: vec!["target_crate".into(), "TargetStruct".into()],
            kind: rustdoc_types::ItemKind::Struct,
        },
    );

    const FOREIGN: Option<u32> = Some(100);
    let home_items = vec![
        synth_item(50, Some("RootTarget"), synth_unit_struct()),
        synth_use(20, "BaseAlias", "target_crate::TargetStruct", FOREIGN),
        synth_use(21, "AbsAlias", "target_crate::TargetStruct", FOREIGN),
        synth_use(22, "CrateAlias", "crate::RootTarget", FOREIGN),
        synth_use(23, "SelfAlias", "self::RootTarget", FOREIGN),
        synth_use(24, "SuperAlias", "super::RootTarget", FOREIGN),
        synth_item(60, Some("InnerTarget"), synth_unit_struct()),
        synth_use(30, "AbsAliasInner", "target_crate::TargetStruct", FOREIGN),
        synth_use(31, "CrateAliasInner", "crate::RootTarget", FOREIGN),
        synth_use(32, "SelfAliasInner", "self::InnerTarget", FOREIGN),
        synth_use(33, "SuperAliasInner", "super::RootTarget", FOREIGN),
    ];
    let home = synth_crate(
        "home_crate",
        1,
        vec![
            (1, None, vec![50, 20, 21, 22, 23, 24, 2]),
            (2, Some("inner"), vec![60, 30, 31, 32, 33]),
        ],
        home_items,
    );

    let nav = Navigator::default();
    nav.working_set
        .insert(CrateName::from("home_crate"), Box::new(Some(home)));
    nav.working_set
        .insert(CrateName::from("target_crate"), Box::new(Some(target)));
    nav
}

/// Cross-crate prefix resolution at a crate root.
///
/// Every prefix in the re-exports below has `use.id` pointing into `target_crate`,
/// so every resolution goes through `Navigator::resolve_path(&source)`. What
/// resolves:
///
/// | prefix                          | resolves | why                             |
/// |---------------------------------|----------|----------------------------------|
/// | `target_crate::TargetStruct`    | ✅       | absolute path                    |
/// | `crate::RootTarget`             | ✅       | rewritten to `home_crate::…`    |
/// | `self::RootTarget`              | ✅       | rewritten to `home_crate::…`    |
/// | `super::RootTarget` (at root)   | ❌       | no parent above the crate root   |
#[test]
fn cross_crate_prefix_resolves_at_root() {
    let nav = build_prefix_test_navigator();

    let root = nav
        .load_crate("home_crate", &semver::VersionReq::STAR)
        .expect("home_crate pre-populated")
        .root_item(&nav);

    let mut resolver = Resolver::new(&nav);
    let names: Vec<&str> = resolver
        .children(root)
        .into_iter()
        .filter_map(|c| c.name())
        .collect();

    assert_eq!(
        names,
        vec![
            "RootTarget",
            "BaseAlias",
            "AbsAlias",
            "CrateAlias",
            "SelfAlias",
            // SuperAlias stays dropped: super:: at the crate root has no parent.
            "inner",
        ],
    );
}

/// Cross-crate prefix resolution inside a nested module, where `self::` and
/// `super::` each have a meaningful module context.
///
/// | prefix                          | resolves | rewritten to                     |
/// |---------------------------------|----------|----------------------------------|
/// | `target_crate::TargetStruct`    | ✅       | (unchanged)                      |
/// | `crate::RootTarget`             | ✅       | `home_crate::RootTarget`         |
/// | `self::InnerTarget`             | ✅       | `home_crate::inner::InnerTarget` |
/// | `super::RootTarget`             | ✅       | `home_crate::RootTarget`         |
#[test]
fn cross_crate_prefix_resolves_in_nested_module() {
    let nav = build_prefix_test_navigator();

    let mut resolver = Resolver::new(&nav);
    let inner = resolver
        .resolve_path("home_crate::inner", &mut vec![])
        .expect("home_crate::inner should resolve");

    let names: Vec<&str> = resolver
        .children(inner)
        .into_iter()
        .filter_map(|c| c.name())
        .collect();

    assert_eq!(
        names,
        vec![
            "InnerTarget",
            "AbsAliasInner",
            "CrateAliasInner",
            "SelfAliasInner",
            "SuperAliasInner",
        ],
    );
}

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
            stability: None,
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

    let data = RustdocData::from_crate(
        crate_data,
        "fake_crate".into(),
        CrateProvenance::DocsRs,
        PathBuf::new(),
        None,
    );

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

    let mut resolver = Resolver::new(&nav);
    let names: Vec<&str> = resolver
        .children(root)
        .into_iter()
        .filter_map(|c| c.name())
        .collect();

    assert_eq!(
        names,
        vec!["KeepA", "KeepB"],
        "unresolvable `Use` items should be skipped, not terminate the iterator"
    );
}
