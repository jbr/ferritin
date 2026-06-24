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

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::JoinHandle;

use memmap2::Mmap;
use rkyv::rancor::Error;
use rustc_hash::FxHashMap;
use rustdoc_types::{ArchivedCrate, Crate, ExternalCrate, FORMAT_VERSION, Id, Item, ItemSummary};

/// Bumped whenever the on-disk archive layout could change incompatibly in a way
/// not already captured by [`FORMAT_VERSION`] or the target tag — e.g. a new rkyv
/// release or a change to which fields we archive.
const ARCHIVE_SCHEMA: u32 = 1;

/// A tag identifying the archive layout. An rkyv archive is not portable across
/// target architectures or pointer widths, and its contents depend on the rustdoc
/// format version, so all of those go in the sidecar filename: a foreign or stale
/// archive simply isn't found by name and is regenerated from JSON.
fn schema_tag() -> String {
    format!(
        "rkyv{ARCHIVE_SCHEMA}-fmt{FORMAT_VERSION}-{}-{}",
        std::env::consts::ARCH,
        usize::BITS,
    )
}

/// The rkyv sidecar path for a cached JSON file, e.g.
/// `…/1.40.0.json` → `…/1.40.0.json.rkyv1-fmt57-x86_64-64.rkyv`.
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
    let bytes =
        rkyv::to_bytes::<Error>(krate).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
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
    fs::rename(&tmp, &sidecar)
}

/// Spawn a background thread to serialize and write the sidecar, so a cold load
/// doesn't block the request on serialization + 60-odd MB of disk I/O. Returns
/// the join handle; the caller ([`RustdocData`](crate::rustdoc_data::RustdocData))
/// joins it on drop, so a short-lived CLI process still finishes the write before
/// exiting (the result has already been rendered by then). Returns `None` when
/// there is nothing to write (no backing path, e.g. synthetic test crates).
pub(crate) fn write_archive_async(krate: Arc<Crate>, json_path: PathBuf) -> Option<JoinHandle<()>> {
    if json_path.as_os_str().is_empty() {
        return None;
    }
    Some(std::thread::spawn(move || {
        if let Err(e) = write_archive(&krate, &json_path) {
            log::debug!("could not write rkyv sidecar: {e}");
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

    /// The zero-copy archived view. O(1): a pointer cast over the mapped bytes,
    /// with the OS paging in only the regions actually touched.
    pub(crate) fn krate(&self) -> &ArchivedCrate {
        // SAFETY: see `Archive::open` — the bytes are a validated-by-construction
        // archive produced by this same build.
        unsafe { rkyv::access_unchecked::<ArchivedCrate>(&self.mmap[..]) }
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

    /// Deserialize the entire item index (full materialization), used only for
    /// whole-crate impl-block scans.
    pub(crate) fn full_index(&self) -> FxHashMap<Id, Item> {
        rkyv::deserialize::<FxHashMap<Id, Item>, Error>(&self.krate().index).unwrap_or_default()
    }
}

/// The eagerly-materialized structural parts of a `Crate`.
pub(crate) struct EagerParts {
    pub(crate) root: Id,
    pub(crate) crate_version: Option<String>,
    pub(crate) paths: FxHashMap<Id, ItemSummary>,
    pub(crate) external_crates: FxHashMap<u32, ExternalCrate>,
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
