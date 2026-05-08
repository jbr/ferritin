use fieldwork::Fieldwork;
use rustdoc_types::{Crate, ExternalCrate, Id, Item, ItemKind};
use semver::{Version, VersionReq};
use std::collections::HashMap;
use std::fmt::{self, Debug, Formatter};
use std::ops::Deref;
use std::path::PathBuf;

use crate::CrateProvenance;
use crate::doc_ref::{self, DocRef};
use crate::navigator::{Navigator, parse_docsrs_url};

/// Wrapper around rustdoc JSON data that provides convenient query methods
#[derive(Clone, Fieldwork, PartialEq, Eq)]
#[fieldwork(get, rename_predicates)]
pub struct RustdocData {
    pub(crate) crate_data: Crate,
    pub(crate) name: String,
    pub(crate) provenance: CrateProvenance,
    pub(crate) fs_path: PathBuf,
    pub(crate) version: Option<Version>,

    /// Reverse index from path string (excluding crate name) to `Id`, for local items.
    ///
    /// Populated by [`RustdocData::build_path_index`] before crate insertion into Navigator.
    /// Used as a fallback in `Navigator::resolve_path` when tree traversal fails (e.g. when
    /// the path passes through a private module not visible in the public item tree).
    ///
    /// Contains two kinds of entries per item:
    /// - A kind-qualified key: `"mod1::mod@name"` or `"mod1::fn@name"` — always present,
    ///   allows users to explicitly request a specific kind when names collide.
    /// - An unqualified key: `"mod1::name"` — present only when no other item of a different
    ///   kind shares this path (i.e. unambiguous).
    #[field = false]
    pub(crate) path_to_id: HashMap<String, Id>,
}

impl Debug for RustdocData {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("RustdocData")
            .field("name", &self.name)
            .field("crate_type", &self.provenance)
            .field("fs_path", &self.fs_path)
            .field("version", &self.version)
            .finish()
    }
}

impl Deref for RustdocData {
    type Target = Crate;

    fn deref(&self) -> &Self::Target {
        &self.crate_data
    }
}

impl RustdocData {
    pub(crate) fn get<'a>(&'a self, navigator: &'a Navigator, id: &Id) -> Option<DocRef<'a, Item>> {
        let item = self.crate_data.index.get(id)?;
        Some(DocRef::new(navigator, self, item))
    }

    pub fn path<'a>(&'a self, id: &Id) -> Option<doc_ref::Path<'a>> {
        self.paths.get(id).map(|summary| summary.into())
    }

    pub fn root_item<'a>(&'a self, navigator: &'a Navigator) -> DocRef<'a, Item> {
        DocRef::new(navigator, self, &self.index[&self.root])
    }

    pub fn traverse_to_crate_by_id<'a>(
        &'a self,
        navigator: &'a Navigator,
        id: u32,
    ) -> Option<&'a RustdocData> {
        if id == 0 {
            //special case: 0 is not in external crates, and it always means "this crate"
            return Some(self);
        }

        let ExternalCrate {
            name,
            html_root_url,
            ..
        } = self.external_crates.get(&id)?;

        let (name, version_req) = html_root_url.as_deref().and_then(parse_docsrs_url).map_or(
            (&**name, VersionReq::STAR),
            |(name, version)| {
                let version_req =
                    VersionReq::parse(&format!("={version}")).unwrap_or(VersionReq::STAR);

                (name, version_req)
            },
        );

        navigator.load_crate(name, &version_req)
    }

    /// Build the reverse path index from `paths`, for use by `Navigator::resolve_path`.
    ///
    /// Indexes local items (`crate_id == 0`) by their path string (excluding the crate name
    /// prefix). For example, an item at `["my_crate", "private", "MyStruct"]` gets:
    ///
    /// - A kind-qualified entry: `"private::struct@MyStruct"` → Id (always)
    /// - An unqualified entry: `"private::MyStruct"` → Id (only if no collision at that path)
    pub(crate) fn build_path_index(&mut self) {
        // Collect all local items grouped by their unqualified path.
        let mut by_unqualified: HashMap<String, Vec<(Id, ItemKind)>> = HashMap::new();
        for (id, summary) in &self.crate_data.paths {
            if summary.crate_id != 0 {
                continue;
            }
            let Some(tail) = summary.path.get(1..) else {
                continue;
            };
            if tail.is_empty() {
                continue;
            }
            by_unqualified
                .entry(tail.join("::"))
                .or_default()
                .push((*id, summary.kind));
        }

        let mut map = HashMap::new();
        for (unqualified, items) in &by_unqualified {
            // Split into prefix and last segment name so the discriminator goes on the
            // final segment only: e.g. "mod1::mod2::fn@name" not "fn@mod1::mod2::name".
            let (prefix, last_name) = match unqualified.rfind("::") {
                Some(sep) => (&unqualified[..sep + 2], &unqualified[sep + 2..]),
                None => ("", unqualified.as_str()),
            };

            // Always insert a kind-qualified entry for each item.
            for (id, kind) in items {
                let qualified = format!("{prefix}{}@{last_name}", kind_discriminator(*kind));
                map.insert(qualified, *id);
            }

            // Insert the unqualified entry only when it is unambiguous (exactly one item).
            if items.len() == 1 {
                map.insert(unqualified.clone(), items[0].0);
            }
        }

        self.path_to_id = map;
    }
}

/// Returns the rustdoc discriminator prefix for an item kind, e.g. `"mod"` for `Module`.
///
/// Matches rustdoc's intra-doc link disambiguator syntax. Notably:
/// - `"tyalias"` for `TypeAlias` (rustdoc uses `tyalias@` / `typealias@`)
/// - `"type"` for `AssocType` (rustdoc uses `type@` for associated types)
/// - `"fn"` for both functions and methods
pub(crate) fn kind_discriminator(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Module => "mod",
        ItemKind::Struct => "struct",
        ItemKind::Enum => "enum",
        ItemKind::Union => "union",
        ItemKind::Trait => "trait",
        ItemKind::TraitAlias => "traitalias",
        ItemKind::Function => "fn",
        ItemKind::TypeAlias => "tyalias",
        ItemKind::AssocType => "type",
        ItemKind::Constant | ItemKind::AssocConst => "const",
        ItemKind::Static => "static",
        ItemKind::Macro => "macro",
        ItemKind::ProcAttribute => "attr",
        ItemKind::ProcDerive => "derive",
        ItemKind::Primitive => "prim",
        ItemKind::Variant => "variant",
        ItemKind::StructField => "field",
        ItemKind::Keyword => "keyword",
        ItemKind::Attribute => "attribute",
        ItemKind::ExternCrate | ItemKind::Use | ItemKind::Impl | ItemKind::ExternType => "item",
    }
}
