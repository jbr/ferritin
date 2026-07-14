//! Memory-mapped rkyv archive of a rustdoc `Crate`, for zero-parse partial reads.
//!
//! docs.rs only serves JSON, so the archive is a purely local, derived cache: we
//! parse the JSON once, serialize the `Crate` to an rkyv archive beside it, and on
//! subsequent loads memory-map the archive instead of re-parsing megabytes of JSON.
//! Individual items are then deserialized lazily (see [`RustdocData`]), so a lookup
//! that touches one item out of a 61 MB `core` no longer pays to parse the whole file.
//!
//! The archive is disposable: it is keyed by a [schema tag](schema_tag) covering the
//! layout-affecting inputs (rustdoc-types/rkyv layout, target arch and pointer width),
//! written atomically, and regenerated on any miss, staleness, or error — callers
//! always fall back to parsing the JSON.
//!
//! [`RustdocData`]: crate::rustdoc_data::RustdocData

use crate::indexes::DerivedIndexes;
use memmap2::Mmap;
use rkyv::rancor::Error;
use rustc_hash::FxHashMap;
use rustdoc_types::{
    ArchivedCrate, ArchivedId, Crate, ExternalCrate, FORMAT_VERSION, Id, ItemSummary,
};
use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::JoinHandle,
};

/// Bumped whenever the on-disk archive layout could change incompatibly in a way
/// not already captured by [`FORMAT_VERSION`], [`RKYV_VERSION`], or the target
/// tag — i.e. a change to which fields we archive.
///
/// It must also be bumped when the archived *contents* change meaning while the
/// layout stays the same: a stale sidecar is found by name and trusted, so a
/// [`DerivedIndexes`] built by older logic would be read back and silently used.
///
/// 2: archive root became [`Sidecar`] (`Crate` + [`DerivedIndexes`])
/// 3: `DerivedIndexes::parents` gained trait members and union fields
/// 4: `DerivedIndexes::parents` peels references (`impl T for &File` → `File`)
const ARCHIVE_SCHEMA: u32 = 4;

/// The exact rkyv version this build serializes with, pinned into the schema
/// tag: the warm path reads archives via `access_unchecked` (no validation),
/// so any change to rkyv's archived representations must invalidate old
/// sidecars *by construction*, not by semver trust — rkyv documents which
/// features are format-breaking but makes no written stability promise
/// between releases. A unit test asserts this matches Cargo.lock, so a rkyv
/// bump fails tests until the constant (and therefore the tag) is updated.
const RKYV_VERSION: &str = "0.8.17";

/// The archive root: the crate plus the derived reverse indexes, so the warm
/// path can resolve impl lookups directly in the mapped bytes instead of
/// scanning (and materializing) the whole item index.
///
/// The `Crate` is borrowed (`Inline`) because at write time it lives in an
/// `Arc` shared with the resident [`RustdocData`](crate::RustdocData); the
/// archived layout is identical to archiving an owned `Crate`.
#[derive(rkyv::Archive, rkyv::Serialize)]
struct Sidecar<'a> {
    #[rkyv(with = rkyv::with::Inline)]
    krate: &'a Crate,
    indexes: DerivedIndexes,
}

/// A tag identifying the archive layout. An rkyv archive is not portable across
/// target architectures or pointer widths, and its contents depend on the rustdoc
/// format version, so all of those go in the sidecar filename: a foreign or stale
/// archive simply isn't found by name and is regenerated from JSON.
fn schema_tag() -> String {
    format!(
        "rkyv{ARCHIVE_SCHEMA}.{RKYV_VERSION}-fmt{FORMAT_VERSION}-{}-{}",
        std::env::consts::ARCH,
        usize::BITS,
    )
}

/// The rkyv sidecar path for a cached JSON file, e.g.
/// `…/1.40.0.json` → `…/1.40.0.json.rkyv2.0.8.17-fmt60-x86_64-64.rkyv`.
pub(crate) fn sidecar_path(json_path: &Path) -> PathBuf {
    let mut name = json_path.as_os_str().to_owned();
    name.push(format!(".{}.rkyv", schema_tag()));
    PathBuf::from(name)
}

/// Serialize `krate` to an rkyv archive beside `json_path`, via a temp file and an
/// atomic rename so a torn write can never be observed. Best-effort: any error is
/// returned for the caller to ignore.
pub(crate) fn write_archive(krate: &Crate, json_path: &Path) -> io::Result<()> {
    // No backing JSON path (e.g. synthetic test crates) — nothing to cache beside.
    if json_path.as_os_str().is_empty() {
        return Ok(());
    }
    let indexes = DerivedIndexes::build(krate);
    let bytes = rkyv::to_bytes::<Error>(&Sidecar { krate, indexes }).map_err(io::Error::other)?;
    let sidecar = sidecar_path(json_path);

    // Unique per write: the pid keeps it distinct across processes, and an atomic
    // counter keeps concurrent writers in the *same* process (e.g. parallel tests
    // each loading the same crate) from clobbering one another's temp file before
    // the rename.
    static TMP_SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let mut tmp = sidecar.clone().into_os_string();
    tmp.push(format!(".tmp.{}.{seq}", std::process::id()));
    let tmp = PathBuf::from(tmp);

    fs::write(&tmp, &bytes)?;
    fs::rename(&tmp, &sidecar)?;
    remove_stale_siblings(json_path, &sidecar);
    Ok(())
}

/// How old an orphaned temp file must be before cleanup will remove it, so an
/// in-flight write from a concurrent process isn't yanked out from under its
/// rename.
const TMP_ORPHAN_AGE: std::time::Duration = std::time::Duration::from_secs(60 * 60);

/// Best-effort removal of `json_path`'s sidecars written under other schema
/// tags, plus orphaned temp files from interrupted writes. Runs right after a
/// successful write — a new sidecar means every differently-tagged sibling is
/// stale for this build — so each regeneration (rkyv bump, format bump, schema
/// bump) reclaims the previous generation instead of accumulating one file per
/// tag forever.
///
/// A sibling another *build* still uses (older binary, different arch sharing
/// the cache) is deleted too; that build regenerates it on its next cold load.
/// Alternating between two builds therefore costs a JSON re-parse per switch —
/// accepted, since coexisting tags should be transient.
fn remove_stale_siblings(json_path: &Path, current: &Path) {
    let Some(dir) = json_path.parent() else {
        return;
    };
    let Some(json_name) = json_path.file_name().and_then(|name| name.to_str()) else {
        return;
    };
    let prefix = format!("{json_name}.");
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        // Only files this module produces: "{json}.{tag}.rkyv" and their
        // ".tmp.{pid}.{seq}" suffixed temp files.
        if !name.starts_with(&prefix) || !name.contains(".rkyv") {
            continue;
        }
        let path = entry.path();
        if path == current {
            continue;
        }
        let fresh_tmp = name.contains(".tmp.")
            && entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .is_ok_and(|modified| modified.elapsed().unwrap_or_default() < TMP_ORPHAN_AGE);
        if fresh_tmp {
            continue;
        }
        log::debug!("removing stale sidecar {}", path.display());
        let _ = fs::remove_file(&path);
    }
}

/// Spawn a background thread to serialize and write the sidecar, so a cold load
/// doesn't block the request on serialization + 60-odd MB of disk I/O. Returns
/// the join handle; the caller ([`RustdocData`](crate::rustdoc_data::RustdocData))
/// joins it on drop, so a short-lived CLI process still finishes the write before
/// exiting (the result has already been rendered by then). Returns `None` when
/// there is nothing to write (no backing path, e.g. synthetic test crates).
///
/// `written` is set once the sidecar has landed on disk (write + atomic
/// rename): from that moment a warm reload is strictly better than the fat
/// resident form, and the Store drops the cold cache entry (supersede-on-
/// sidecar-write). A failed write leaves the flag unset and the entry cached.
pub(crate) fn write_archive_async(
    krate: Arc<Crate>,
    json_path: PathBuf,
    written: Arc<AtomicBool>,
) -> Option<JoinHandle<()>> {
    if json_path.as_os_str().is_empty() {
        return None;
    }
    Some(std::thread::spawn(move || {
        match write_archive(&krate, &json_path) {
            Ok(()) => written.store(true, Ordering::Release),
            Err(e) => log::debug!("could not write rkyv sidecar: {e}"),
        }
    }))
}

/// A memory-mapped rkyv archive of a `Crate`.
pub(crate) struct Archive {
    mmap: Mmap,
}

impl Archive {
    /// Open the sidecar for `json_path` if it exists and is at least as new as the
    /// JSON. Returns `None` on any problem, so the caller falls back to JSON.
    pub(crate) fn open(json_path: &Path) -> Option<Archive> {
        let sidecar = sidecar_path(json_path);
        if !is_fresh(&sidecar, json_path) {
            return None;
        }
        let file = fs::File::open(&sidecar).ok()?;
        // SAFETY: the file is our own cache, keyed by `schema_tag` (which pins the
        // rustdoc/rkyv layout, target arch and pointer width) and written via an
        // atomic rename, so it can only be an archive this exact build produced.
        // A mismatch would be a tag bug, not untrusted input; the archive is
        // disposable and regenerated from JSON on the next load if ever rejected.
        let mmap = unsafe { Mmap::map(&file) }.ok()?;
        Some(Archive { mmap })
    }

    /// The zero-copy archived root. O(1): a pointer cast over the mapped bytes,
    /// with the OS paging in only the regions actually touched.
    fn sidecar(&self) -> &ArchivedSidecar<'_> {
        // SAFETY: see `Archive::open` — the bytes are a validated-by-construction
        // archive produced by this same build.
        unsafe { rkyv::access_unchecked::<ArchivedSidecar>(&self.mmap[..]) }
    }

    /// The zero-copy archived crate view.
    pub(crate) fn krate(&self) -> &ArchivedCrate {
        &self.sidecar().krate
    }

    /// Impl blocks with no trait targeting `type_id`, from the archived index.
    pub(crate) fn inherent_impl_ids(&self, type_id: &Id) -> Vec<Id> {
        archived_ids(&self.sidecar().indexes.inherent_impls, type_id)
    }

    /// Trait impl blocks targeting `type_id`, from the archived index.
    pub(crate) fn trait_impl_ids(&self, type_id: &Id) -> Vec<Id> {
        archived_ids(&self.sidecar().indexes.trait_impls, type_id)
    }

    /// Impl blocks implementing the trait `trait_id`, from the archived index.
    pub(crate) fn implementor_ids(&self, trait_id: &Id) -> Vec<Id> {
        archived_ids(&self.sidecar().indexes.implementors, trait_id)
    }

    /// The containing item of an associated item / variant / field, from the
    /// archived index.
    pub(crate) fn assoc_parent_id(&self, id: &Id) -> Option<Id> {
        self.sidecar()
            .indexes
            .parents
            .get(&ArchivedId(rkyv::rend::u32_le::from_native(id.0)))
            .map(|parent| Id(parent.0.to_native()))
    }

    /// Deserialize the small, structural maps that every load needs eagerly
    /// (kept resident so accessors can hand out borrows). The large `index` is
    /// left in the archive for lazy per-item materialization.
    pub(crate) fn eager_parts(&self) -> Option<EagerParts> {
        let krate = self.krate();
        Some(EagerParts {
            root: rkyv::deserialize::<Id, Error>(&krate.root).ok()?,
            crate_version: rkyv::deserialize::<Option<String>, Error>(&krate.crate_version).ok()?,
            paths: rkyv::deserialize::<FxHashMap<Id, ItemSummary>, Error>(&krate.paths).ok()?,
            external_crates: rkyv::deserialize::<FxHashMap<u32, ExternalCrate>, Error>(
                &krate.external_crates,
            )
            .ok()?,
        })
    }
}

/// The eagerly-materialized structural parts of a `Crate`.
pub(crate) struct EagerParts {
    pub(crate) root: Id,
    pub(crate) crate_version: Option<String>,
    pub(crate) paths: FxHashMap<Id, ItemSummary>,
    pub(crate) external_crates: FxHashMap<u32, ExternalCrate>,
}

/// Look up `id` in an archived id→ids map, converting back to native `Id`s.
/// Missing key yields an empty list (a type with no impls of that class).
fn archived_ids(
    map: &rkyv::collections::swiss_table::ArchivedHashMap<
        ArchivedId,
        rkyv::vec::ArchivedVec<ArchivedId>,
    >,
    id: &Id,
) -> Vec<Id> {
    map.get(&ArchivedId(rkyv::rend::u32_le::from_native(id.0)))
        .map(|ids| ids.iter().map(|id| Id(id.0.to_native())).collect())
        .unwrap_or_default()
}

fn is_fresh(sidecar: &Path, json_path: &Path) -> bool {
    let mtime = |p: &Path| fs::metadata(p).and_then(|m| m.modified()).ok();
    match (mtime(sidecar), mtime(json_path)) {
        (Some(sidecar_mtime), Some(json_mtime)) => sidecar_mtime >= json_mtime,
        // JSON gone but archive present: trust the archive.
        (Some(_), None) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{RKYV_VERSION, remove_stale_siblings, sidecar_path};

    /// Stale-tag sidecars and old temp files are removed; the current sidecar,
    /// the JSON itself, fresh temp files, and unrelated siblings (search
    /// `.index`, other versions' files) survive.
    #[test]
    fn stale_sibling_cleanup() {
        let dir = std::env::temp_dir().join(format!(
            "ferritin-archive-cleanup-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();

        let json = dir.join("1.40.0.json");
        let current = sidecar_path(&json);
        let stale = dir.join("1.40.0.json.rkyv1-fmt59-x86_64-64.rkyv");
        let fresh_tmp = {
            let mut name = current.clone().into_os_string();
            name.push(".tmp.12345.0");
            std::path::PathBuf::from(name)
        };
        let index = dir.join("1.40.0.index");
        let other_version = dir.join("1.39.0.json.rkyv1-fmt59-x86_64-64.rkyv");
        for file in [&json, &current, &stale, &fresh_tmp, &index, &other_version] {
            std::fs::write(file, b"x").unwrap();
        }

        remove_stale_siblings(&json, &current);

        assert!(!stale.exists(), "stale-tag sidecar should be removed");
        assert!(current.exists(), "current sidecar must survive");
        assert!(json.exists(), "the JSON itself must survive");
        assert!(fresh_tmp.exists(), "a fresh temp file must survive");
        assert!(index.exists(), "the search index must survive");
        assert!(
            other_version.exists(),
            "another version's sidecar must survive (cleanup is per-JSON)"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// [`RKYV_VERSION`] must track Cargo.lock exactly. If this fails, rkyv was
    /// bumped: update the constant so the schema tag invalidates sidecars
    /// written with the previous version's layout. Shipping without the bump
    /// would let `access_unchecked` reinterpret old archives with the new
    /// layout — undefined behavior, not a clean cache miss.
    #[test]
    fn rkyv_version_constant_matches_cargo_lock() {
        let lock_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../Cargo.lock");
        let lock = std::fs::read_to_string(&lock_path).expect("read workspace Cargo.lock");
        let mut lines = lock.lines();
        let locked_version = loop {
            match lines.next() {
                Some("name = \"rkyv\"") => {
                    break lines
                        .next()
                        .and_then(|line| line.strip_prefix("version = \""))
                        .and_then(|rest| rest.strip_suffix('"'))
                        .expect("version line follows rkyv's name line");
                }
                Some(_) => {}
                None => panic!("no rkyv package in {}", lock_path.display()),
            }
        };
        assert_eq!(
            RKYV_VERSION, locked_version,
            "rkyv was bumped in Cargo.lock; update archive::RKYV_VERSION to match"
        );
    }
}
