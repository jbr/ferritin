use elsa::sync::FrozenMap;
use fieldwork::Fieldwork;
use rkyv::rancor::Error;
use rustc_hash::FxHashMap;
use rustdoc_types::{ArchivedId, Crate, ExternalCrate, Id, Item, ItemKind, ItemSummary};
use semver::{Version, VersionReq};
use std::collections::HashMap;
use std::fmt::{self, Debug, Formatter};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::thread::JoinHandle;

use crate::CrateProvenance;
use crate::archive::{self, Archive};
use crate::crate_name::CrateName;
use crate::doc_ref::{self, DocRef};
use crate::indexes::DerivedIndexes;
use crate::navigator::Navigator;
use crate::store::parse_docsrs_url;

/// A dependency pin extracted from an `external_crates` entry's docs.rs
/// `html_root_url`: the real crates.io name (which can differ from the
/// internal name beyond dash/underscore folding, e.g. `sha1` → `sha-1`) and
/// the exact version the referencing crate was built against.
#[derive(Debug, Clone, Fieldwork)]
#[fieldwork(get)]
pub(crate) struct ExternalCrateInfo {
    name: String,
    version: Version,
}

/// Wrapper around a rustdoc `Crate` with convenient query methods.
///
/// Storage has two shapes:
///
/// - **Cold path** (a freshly-parsed `Crate`): the whole crate is kept `resident`
///   behind an `Arc`, and a background thread serializes it to an rkyv sidecar for
///   next time (`write_handle`, joined on drop). Accessors read straight from the
///   resident `Crate`.
/// - **Warm path** (an existing sidecar): the heavy item `index` stays in the
///   memory-mapped `archive` and individual items are deserialized on demand into
///   `item_cache`, so a lookup that touches one item out of a large crate does not
///   pay to parse the whole thing. The small structural maps (`paths`,
///   `external_crates`, `root`, `crate_version`) are materialized eagerly so
///   accessors can hand out borrows; impl lookups go through the precomputed
///   reverse indexes in the archive ([`crate::indexes`]), so nothing ever
///   materializes the full index.
#[derive(Fieldwork)]
#[fieldwork(get, rename_predicates)]
pub struct RustdocData {
    #[field = false]
    resident: Option<Arc<Crate>>,
    #[field = false]
    write_handle: Option<JoinHandle<()>>,
    #[field = false]
    archive: Option<Archive>,
    #[field = false]
    item_cache: FrozenMap<Id, Box<Item>>,
    /// Cold path only: reverse impl indexes computed from the resident crate on
    /// first use. On the warm path the equivalent maps are read directly from
    /// the archive (they're precomputed at sidecar-write time).
    #[field = false]
    derived_indexes: OnceLock<DerivedIndexes>,
    /// The dependency versions this crate was built against, extracted from
    /// `external_crates` on first use; see [`RustdocData::built_against`].
    #[field = false]
    external_versions: OnceLock<FxHashMap<CrateName<'static>, ExternalCrateInfo>>,
    // The eager small maps below are populated only on the warm path; on the cold
    // path the resident `Crate` is consulted instead and these stay empty.
    #[field = false]
    pub(crate) paths: FxHashMap<Id, ItemSummary>,
    #[field = false]
    external_crates: FxHashMap<u32, ExternalCrate>,
    #[field = false]
    root: Id,
    #[field = false]
    crate_version: Option<String>,

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

impl Drop for RustdocData {
    /// Wait for the background sidecar write (if any) to finish, so a short-lived
    /// process doesn't exit before the write completes — the result has already
    /// been rendered, so this only delays teardown, not user-visible output.
    fn drop(&mut self) {
        if let Some(handle) = self.write_handle.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
impl RustdocData {
    /// Insert an extra path summary into the resident crate (synthetic test crates
    /// are always resident and uniquely owned, since no sidecar write is spawned).
    pub(crate) fn insert_path_for_test(&mut self, id: Id, summary: ItemSummary) {
        self.resident
            .as_mut()
            .and_then(Arc::get_mut)
            .expect("test crate must be uniquely resident")
            .paths
            .insert(id, summary);
    }
}

impl RustdocData {
    /// Construct from a freshly-parsed `Crate` (the cold path). The crate is kept
    /// resident so this load is fully materialized, while a background thread
    /// serializes it to an rkyv sidecar beside `fs_path` so the next load can take
    /// the warm path. The write thread is joined on drop.
    pub(crate) fn from_crate(
        crate_data: Crate,
        name: String,
        provenance: CrateProvenance,
        fs_path: PathBuf,
        version: Option<Version>,
    ) -> Self {
        let resident = Arc::new(crate_data);
        let write_handle = archive::write_archive_async(Arc::clone(&resident), fs_path.clone());
        Self {
            resident: Some(resident),
            write_handle,
            archive: None,
            item_cache: FrozenMap::new(),
            derived_indexes: OnceLock::new(),
            external_versions: OnceLock::new(),
            paths: FxHashMap::default(),
            external_crates: FxHashMap::default(),
            root: Id(0),
            crate_version: None,
            name,
            provenance,
            fs_path,
            version,
            path_to_id: HashMap::new(),
        }
    }

    /// Try to construct from an existing rkyv sidecar (the warm fast path). Returns
    /// `None` if the sidecar is missing, stale, or unreadable, so the caller falls
    /// back to parsing JSON. `version`, if not supplied, is taken from the archived
    /// crate-version string.
    pub(crate) fn try_from_sidecar(
        fs_path: &Path,
        name: String,
        provenance: CrateProvenance,
        version: Option<Version>,
    ) -> Option<Self> {
        let archive = Archive::open(fs_path)?;
        let parts = archive.eager_parts()?;
        let version = version.or_else(|| {
            parts
                .crate_version
                .as_deref()
                .and_then(|v| Version::parse(v).ok())
        });
        Some(Self {
            resident: None,
            write_handle: None,
            archive: Some(archive),
            item_cache: FrozenMap::new(),
            derived_indexes: OnceLock::new(),
            external_versions: OnceLock::new(),
            paths: parts.paths,
            external_crates: parts.external_crates,
            root: parts.root,
            crate_version: parts.crate_version,
            name,
            provenance,
            fs_path: fs_path.to_owned(),
            version,
            path_to_id: HashMap::new(),
        })
    }

    // ---- Accessors ----
    //
    // Each consults the resident `Crate` (cold path) when present, otherwise the
    // eager maps / lazily-materialized archive (warm path). All item access is
    // point lookups; impl relationships come from the derived reverse indexes.

    /// The eagerly-available `paths` map — the resident crate's, or the small map
    /// deserialized from the archive on the warm path.
    fn paths_map(&self) -> &FxHashMap<Id, ItemSummary> {
        self.resident.as_ref().map_or(&self.paths, |c| &c.paths)
    }

    /// The eagerly-available `external_crates` map (resident or warm).
    fn external_map(&self) -> &FxHashMap<u32, ExternalCrate> {
        self.resident
            .as_ref()
            .map_or(&self.external_crates, |c| &c.external_crates)
    }

    /// Look up a single item by `Id`, deserializing it from the archive on first
    /// access (and caching it) on the warm path.
    pub fn get_item(&self, id: &Id) -> Option<&Item> {
        if let Some(crate_data) = &self.resident {
            return crate_data.index.get(id);
        }
        if let Some(item) = self.item_cache.get(id) {
            return Some(item);
        }
        let archived = self.archive.as_ref()?.krate();
        let archived_item = archived
            .index
            .get(&ArchivedId(rkyv::rend::u32_le::from_native(id.0)))?;
        let item = rkyv::deserialize::<Item, Error>(archived_item).ok()?;
        Some(self.item_cache.insert(*id, Box::new(item)))
    }

    /// Look up an item's summary (definition path, kind, owning crate) by `Id`.
    pub fn path_summary(&self, id: &Id) -> Option<&ItemSummary> {
        self.paths_map().get(id)
    }

    /// The `Id` of this crate's root module.
    pub fn root_id(&self) -> &Id {
        self.resident.as_ref().map_or(&self.root, |c| &c.root)
    }

    /// The crate's *library* name — the Rust identifier rustdoc writes into every
    /// item path (`futures_io`), and the directory its rendered output is rooted at.
    ///
    /// Distinct from [`name`](Self::name), the package name whichever `Source`
    /// resolved this crate (`futures-io`) — that is what carries the version and
    /// what the Store is keyed by. The two coincide for std, fold together for most
    /// crates, and genuinely diverge when a crate declares an explicit `[lib] name`
    /// (`sha-1` → `sha1`, an unrelated crate on crates.io).
    ///
    /// Read from the eagerly-materialized `paths` map, so this costs no item
    /// materialization. Falls back to the package name if the root somehow has no
    /// path entry.
    pub fn lib_name(&self) -> &str {
        self.path(self.root_id())
            .and_then(|path| path.into_iter().next())
            .unwrap_or(&self.name)
    }

    /// Look up an external-crate entry by its `crate_id`.
    pub fn external_crate(&self, crate_id: &u32) -> Option<&ExternalCrate> {
        self.external_map().get(crate_id)
    }

    /// Iterate every external-crate entry.
    pub fn external_crates_iter(&self) -> impl Iterator<Item = &ExternalCrate> {
        self.external_map().values()
    }

    /// The exact version of `name` this crate was built against, if its
    /// `external_crates` table records one (docs.rs builds record the whole
    /// build graph in `html_root_url`s; local builds only what dependencies
    /// self-declare).
    ///
    /// Indexed under both the internal (underscored) and real (crates.io)
    /// names, which can differ beyond what `CrateName` folding equates.
    pub(crate) fn built_against(&self, name: &CrateName<'static>) -> Option<&ExternalCrateInfo> {
        self.external_versions
            .get_or_init(|| {
                let mut map = FxHashMap::default();
                for external in self.external_crates_iter() {
                    let Some((real_name, version)) =
                        external.html_root_url.as_deref().and_then(parse_docsrs_url)
                    else {
                        continue;
                    };
                    let Ok(version) = Version::parse(version) else {
                        continue;
                    };
                    let info = ExternalCrateInfo {
                        name: real_name.to_string(),
                        version,
                    };
                    map.entry(CrateName::from(real_name.to_string()))
                        .or_insert_with(|| info.clone());
                    map.entry(CrateName::from(external.name.clone()))
                        .or_insert(info);
                }
                map
            })
            .get(name)
    }

    /// Cold-path derived indexes, computed from the resident crate on first use.
    fn cold_indexes(&self, resident: &Crate) -> &DerivedIndexes {
        self.derived_indexes
            .get_or_init(|| DerivedIndexes::build(resident))
    }

    /// Impl blocks with no trait targeting `type_id` (inherent impls).
    pub(crate) fn inherent_impl_ids(&self, type_id: &Id) -> Vec<Id> {
        if let Some(resident) = &self.resident {
            let map = &self.cold_indexes(resident).inherent_impls;
            map.get(type_id).cloned().unwrap_or_default()
        } else if let Some(archive) = &self.archive {
            archive.inherent_impl_ids(type_id)
        } else {
            Vec::new()
        }
    }

    /// Trait impl blocks targeting `type_id`.
    pub(crate) fn trait_impl_ids(&self, type_id: &Id) -> Vec<Id> {
        if let Some(resident) = &self.resident {
            let map = &self.cold_indexes(resident).trait_impls;
            map.get(type_id).cloned().unwrap_or_default()
        } else if let Some(archive) = &self.archive {
            archive.trait_impl_ids(type_id)
        } else {
            Vec::new()
        }
    }

    /// Impl blocks implementing the trait `trait_id` (negative impls included).
    pub(crate) fn implementor_ids(&self, trait_id: &Id) -> Vec<Id> {
        if let Some(resident) = &self.resident {
            let map = &self.cold_indexes(resident).implementors;
            map.get(trait_id).cloned().unwrap_or_default()
        } else if let Some(archive) = &self.archive {
            archive.implementor_ids(trait_id)
        } else {
            Vec::new()
        }
    }

    /// The containing item of an associated item / variant / field, if indexed
    /// (impl-block members map to the impl's target type; blanket-impl members
    /// are absent). See [`crate::indexes::DerivedIndexes::parents`].
    pub(crate) fn assoc_parent_id(&self, id: &Id) -> Option<Id> {
        if let Some(resident) = &self.resident {
            self.cold_indexes(resident).parents.get(id).copied()
        } else {
            self.archive.as_ref()?.assoc_parent_id(id)
        }
    }

    /// The rustdoc JSON's own crate-version string, if present.
    pub fn crate_version(&self) -> Option<&str> {
        self.resident
            .as_ref()
            .map_or(self.crate_version.as_deref(), |c| {
                c.crate_version.as_deref()
            })
    }

    pub(crate) fn get<'a>(&'a self, navigator: &'a Navigator, id: &Id) -> Option<DocRef<'a, Item>> {
        let item = self.get_item(id)?;
        Some(DocRef::new(navigator, self, item))
    }

    /// Resolve a local item by its definition path (the `paths` path with the
    /// crate-name prefix already stripped) and kind, via the reverse path
    /// index. Unlike a public-tree walk, this reaches items whose definition
    /// path passes through private modules — e.g. `quinn_proto`'s
    /// `config::ServerConfig`, where `config` is private but `ServerConfig` is
    /// re-exported at the crate root. An empty `tail` resolves to the crate
    /// root module.
    ///
    /// The lookup is kind-qualified (the `paths` summary carries the kind), so
    /// it stays unambiguous even where a module and a value share a path and
    /// [`build_path_index`](Self::build_path_index) therefore omitted the plain
    /// key.
    pub(crate) fn lookup_definition_path<'a>(
        &'a self,
        navigator: &'a Navigator,
        tail: &[String],
        kind: ItemKind,
    ) -> Option<DocRef<'a, Item>> {
        if tail.is_empty() {
            return Some(self.root_item(navigator));
        }
        let unqualified = tail.join("::");
        let (prefix, last_name) = match unqualified.rfind("::") {
            Some(sep) => (&unqualified[..sep + 2], &unqualified[sep + 2..]),
            None => ("", unqualified.as_str()),
        };
        let qualified = format!("{prefix}{}@{last_name}", kind_discriminator(kind));
        let id = self
            .path_to_id
            .get(&qualified)
            .or_else(|| self.path_to_id.get(&unqualified))?;
        self.get(navigator, id)
    }

    pub fn path<'a>(&'a self, id: &Id) -> Option<doc_ref::Path<'a>> {
        self.paths_map().get(id).map(|summary| summary.into())
    }

    pub fn root_item<'a>(&'a self, navigator: &'a Navigator) -> DocRef<'a, Item> {
        let item = self
            .get_item(self.root_id())
            .expect("crate root item must exist in the index");
        DocRef::new(navigator, self, item)
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
        } = self.external_crate(&id)?;

        // Version fallback chain: the query entrypoint's build graph first —
        // so any name the entrypoint knows resolves to the entrypoint's
        // version, independent of traversal order — then this crate's own
        // html_root_url (for names outside the entrypoint's graph), then
        // latest.
        if let Some((name, version_req)) = navigator.built_against(name) {
            return navigator.load_crate(name, &version_req);
        }

        let (name, version_req) = html_root_url.as_deref().and_then(parse_docsrs_url).map_or(
            (&**name, VersionReq::STAR),
            |(name, version)| {
                let version_req = Version::parse(version)
                    .map(|v| crate::store::exact_req(&v))
                    .unwrap_or(VersionReq::STAR);

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
        for (id, summary) in self.paths_map() {
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
