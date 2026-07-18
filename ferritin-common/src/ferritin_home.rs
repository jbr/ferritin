//! The ferritin cache root (`FERRITIN_HOME`) and one-shot migration of the
//! legacy `$CARGO_HOME/rustdoc-json` cache into it.
//!
//! Everything under the root is reconstructible cache. Layout:
//!
//! ```text
//! $FERRITIN_HOME/
//!   docs/{crate}/{version}/{format}.json   # raw rustdoc JSON + derived
//!                                          # sidecars/search indexes beside it
//!   crate-names/                           # crates.io namespace artifacts
//!   crates-io-versions/                    # per-crate version-resolution cache
//! ```
//!
//! The legacy layout was `$CARGO_HOME/rustdoc-json/{format}/{crate}/{version}.json`,
//! sharded by source format version — load-bearing when reading was
//! format-careful, vestigial now that `conversions` normalizes any supported
//! format transparently. Inverting to `{crate}/{version}/{format}` gives each
//! release's derived family one directory and each crate *name* one directory
//! (where the cross-crate xref index will live).

use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// Resolve the ferritin cache root: `$FERRITIN_HOME` if set, else
/// `$XDG_CACHE_HOME/ferritin` (ignored if relative, per the XDG spec), else
/// `~/.cache/ferritin` — on every platform, including macOS, where a dotdir
/// beats `~/Library/Caches` for a dev tool people will `ls` into.
///
/// `None` only when no home directory can be determined at all.
pub fn resolve() -> Option<PathBuf> {
    if let Some(home) = env_path("FERRITIN_HOME") {
        return Some(home);
    }
    if let Some(xdg) = env_path("XDG_CACHE_HOME").filter(|path| path.is_absolute()) {
        return Some(xdg.join("ferritin"));
    }
    home::home_dir().map(|home| home.join(".cache").join("ferritin"))
}

/// A non-empty environment variable as a path.
fn env_path(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// Migrate a legacy `$CARGO_HOME/rustdoc-json` cache into `new_root`, if one
/// exists. Callers resolving the *default* cache location run this once per
/// process; explicitly-constructed cache dirs (tests) never do.
///
/// The old root's presence is the trigger — it is removed once emptied, so a
/// completed migration costs one `stat` per process thereafter. Unrecognized
/// files are left in place (and logged), which leaves the old root standing;
/// the re-walk on later starts then finds nothing to move.
pub fn migrate_legacy_cache(new_root: &Path) {
    let Ok(cargo_home) = home::cargo_home() else {
        return;
    };
    let old_root = cargo_home.join("rustdoc-json");
    if !old_root.is_dir() || old_root == new_root {
        return;
    }
    let stats = migrate(&old_root, new_root);
    if stats.moved > 0 || stats.cleaned > 0 {
        log::info!(
            "migrated docs cache from {} to {} ({} files moved, {} redundant/stale removed)",
            old_root.display(),
            new_root.display(),
            stats.moved,
            stats.cleaned,
        );
    }
    if stats.left > 0 {
        log::warn!(
            "left {} unrecognized files behind in {}; remove the directory manually to stop \
             ferritin from re-checking it",
            stats.left,
            old_root.display(),
        );
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct MigrationStats {
    /// Files renamed (or copied, across filesystems) into the new layout.
    moved: u64,
    /// Files deleted rather than moved: already present at the destination
    /// (a concurrent fetch won the race — contents are identical) or an
    /// orphaned temp file old enough to be from a dead process.
    cleaned: u64,
    /// Files we did not recognize and left in place.
    left: u64,
}

/// How old an orphaned temp file must be before the migration removes it
/// instead of leaving it for its (possibly still-running) writer. Mirrors
/// `archive::TMP_ORPHAN_AGE`.
const TMP_ORPHAN_AGE: std::time::Duration = std::time::Duration::from_secs(60 * 60);

fn migrate(old_root: &Path, new_root: &Path) -> MigrationStats {
    let mut stats = MigrationStats::default();
    let Ok(entries) = fs::read_dir(old_root) else {
        return stats;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            stats.left += 1;
            continue;
        };
        let path = entry.path();
        match name {
            // Self-contained sibling caches: move wholesale.
            "crate-names" | "crates-io-versions" => {
                migrate_flat_dir(&path, &new_root.join(name), &mut stats);
            }
            // A format shard: {format}/{crate}/{version}.json + derived files.
            _ if name.parse::<u32>().is_ok() && path.is_dir() => {
                migrate_format_shard(&path, name, &new_root.join("docs"), &mut stats);
            }
            _ => stats.left += 1,
        }
    }
    let _ = fs::remove_dir(old_root);
    stats
}

/// Move a directory of plain files (`crate-names/`, `crates-io-versions/`).
/// Fast path: a single rename when the destination doesn't exist yet. Slow
/// path (destination present, or rename refused): per-file, skipping files the
/// destination already has.
fn migrate_flat_dir(old_dir: &Path, new_dir: &Path, stats: &mut MigrationStats) {
    if !new_dir.exists() && fs::rename(old_dir, new_dir).is_ok() {
        // Whole-directory rename: contents uncounted, but not silent.
        stats.moved += 1;
        return;
    }
    let Ok(entries) = fs::read_dir(old_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            stats.left += 1;
            continue;
        };
        if is_temp(name) {
            remove_orphaned_temp(&entry, stats);
            continue;
        }
        move_file(&entry.path(), &new_dir.join(name), stats);
    }
    let _ = fs::remove_dir(old_dir);
}

/// Migrate one legacy format shard: `{format}/{crate}/{version}.json` (and the
/// sidecar/index files beside it) → `docs/{crate}/{version}/{format}.json`.
fn migrate_format_shard(shard: &Path, format: &str, docs_root: &Path, stats: &mut MigrationStats) {
    let Ok(crates) = fs::read_dir(shard) else {
        return;
    };
    for crate_dir in crates.flatten() {
        let crate_name = crate_dir.file_name();
        let Some(crate_name) = crate_name.to_str() else {
            stats.left += 1;
            continue;
        };
        if !crate_dir.path().is_dir() {
            stats.left += 1;
            continue;
        }
        migrate_crate_dir(
            &crate_dir.path(),
            format,
            &docs_root.join(crate_name),
            stats,
        );
    }
    let _ = fs::remove_dir(shard);
}

fn migrate_crate_dir(
    crate_dir: &Path,
    format: &str,
    new_crate_dir: &Path,
    stats: &mut MigrationStats,
) {
    let Ok(entries) = fs::read_dir(crate_dir) else {
        return;
    };
    let mut files: Vec<(String, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| Some((entry.file_name().to_str()?.to_owned(), entry.path())))
        .collect();
    // JSON first: the copy fallback stamps fresh mtimes, and a sidecar copied
    // *before* its JSON would land older than it and read as stale.
    files.sort_by_key(|(name, _)| !name.ends_with(".json"));
    for (name, path) in files {
        if is_temp(&name) {
            if let Ok(entry) = fs::metadata(&path) {
                remove_orphaned_temp_meta(&path, entry, stats);
            }
            continue;
        }
        let Some((version, new_name)) = classify(&name, format) else {
            stats.left += 1;
            continue;
        };
        move_file(&path, &new_crate_dir.join(version).join(new_name), stats);
    }
    let _ = fs::remove_dir(crate_dir);
}

/// Map a legacy filename to `(version dir, new filename)`, or `None` for
/// anything this layout never produced. The version must parse as semver —
/// that's what proves the filename is ours.
fn classify<'a>(name: &'a str, format: &str) -> Option<(&'a str, String)> {
    if let Some(version) = name.strip_suffix(".json") {
        valid_version(version)?;
        Some((version, format!("{format}.json")))
    } else if let Some(version) = name.strip_suffix(".index") {
        valid_version(version)?;
        Some((version, format!("{format}.index")))
    } else if let Some(pos) = name.find(".json.") {
        // Sidecars: "{version}.json.{schema-tag}.rkyv" → "{format}.json.{tag}.rkyv".
        let version = &name[..pos];
        valid_version(version)?;
        Some((version, format!("{format}{}", &name[pos..])))
    } else {
        None
    }
}

fn valid_version(version: &str) -> Option<semver::Version> {
    semver::Version::parse(version).ok()
}

/// Temp files from interrupted atomic writes: `archive` uses `.tmp.{pid}.{seq}`
/// suffixes, the version cache and crate-names use `.{pid}[.{seq}].tmp`
/// extensions.
fn is_temp(name: &str) -> bool {
    name.ends_with(".tmp") || name.contains(".tmp.")
}

fn remove_orphaned_temp(entry: &fs::DirEntry, stats: &mut MigrationStats) {
    if let Ok(metadata) = entry.metadata() {
        remove_orphaned_temp_meta(&entry.path(), metadata, stats);
    }
}

/// Remove a temp file old enough that its writer is certainly gone; leave a
/// fresh one for the (possibly still-running) process that owns it. A left
/// temp keeps its directory — and therefore the old root — in place, so the
/// next process re-runs the sweep and removes it once it ages out.
fn remove_orphaned_temp_meta(path: &Path, metadata: fs::Metadata, stats: &mut MigrationStats) {
    let age = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.elapsed().ok());
    if age.is_some_and(|age| age > TMP_ORPHAN_AGE) {
        let _ = fs::remove_file(path);
        stats.cleaned += 1;
    } else {
        stats.left += 1;
    }
}

/// Move one file into the new layout. If the destination already exists, the
/// old file is redundant (an atomic-rename cache write is complete by
/// construction, and cached content for a given key is immutable) and is
/// removed instead. Falls back to copy + delete across filesystems.
fn move_file(old: &Path, new: &Path, stats: &mut MigrationStats) {
    match try_move_file(old, new) {
        Ok(true) => stats.moved += 1,
        Ok(false) => stats.cleaned += 1,
        Err(error) => {
            log::warn!(
                "could not migrate {} to {}: {error}",
                old.display(),
                new.display()
            );
            stats.left += 1;
        }
    }
}

fn try_move_file(old: &Path, new: &Path) -> io::Result<bool> {
    if new.exists() {
        fs::remove_file(old)?;
        return Ok(false);
    }
    if let Some(parent) = new.parent() {
        fs::create_dir_all(parent)?;
    }
    match fs::rename(old, new) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::CrossesDevices => {
            // Preserve the destination's atomicity even when copying: a
            // concurrent reader must never see a half-copied file.
            let mut tmp = new.as_os_str().to_owned();
            tmp.push(format!(".migrate.{}.tmp", std::process::id()));
            let tmp = PathBuf::from(tmp);
            fs::copy(old, &tmp)?;
            fs::rename(&tmp, new)?;
            fs::remove_file(old)?;
            Ok(true)
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("ferritin-home-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: &Path, contents: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn migrates_shards_and_sibling_dirs() {
        let root = temp_root("full");
        let old = root.join("old");
        let new = root.join("new");

        let sidecar = "1.0.0.json.rkyv4.0.8.17-fmt60-x86_64-64.rkyv";
        touch(&old.join("57/serde/1.0.0.json"), "json");
        touch(&old.join(format!("57/serde/{sidecar}")), "rkyv");
        touch(&old.join("57/serde/1.0.0.index"), "index");
        touch(&old.join("60/serde/1.2.3.json"), "json");
        touch(&old.join("57/tokio/1.40.0.json"), "json");
        touch(&old.join("crate-names/names-v2.tsv.zst"), "names");
        touch(&old.join("crates-io-versions/serde.json"), "versions");

        let stats = migrate(&old, &new);

        assert!(new.join("docs/serde/1.0.0/57.json").exists());
        assert!(
            new.join("docs/serde/1.0.0")
                .join("57.json.rkyv4.0.8.17-fmt60-x86_64-64.rkyv")
                .exists()
        );
        assert!(new.join("docs/serde/1.0.0/57.index").exists());
        assert!(new.join("docs/serde/1.2.3/60.json").exists());
        assert!(new.join("docs/tokio/1.40.0/57.json").exists());
        assert!(new.join("crate-names/names-v2.tsv.zst").exists());
        assert!(new.join("crates-io-versions/serde.json").exists());
        assert!(!old.exists(), "emptied old root should be removed");
        // 5 shard files + 2 wholesale dir renames
        assert_eq!(
            stats,
            MigrationStats {
                moved: 7,
                cleaned: 0,
                left: 0
            }
        );

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn existing_destination_wins_and_old_copy_is_cleaned() {
        let root = temp_root("existing");
        let old = root.join("old");
        let new = root.join("new");

        touch(&old.join("57/serde/1.0.0.json"), "stale");
        touch(&new.join("docs/serde/1.0.0/57.json"), "fresh");

        let stats = migrate(&old, &new);

        assert_eq!(
            fs::read_to_string(new.join("docs/serde/1.0.0/57.json")).unwrap(),
            "fresh"
        );
        assert!(!old.exists());
        assert_eq!(
            stats,
            MigrationStats {
                moved: 0,
                cleaned: 1,
                left: 0
            }
        );

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn unrecognized_files_are_left_and_keep_the_old_root() {
        let root = temp_root("unrecognized");
        let old = root.join("old");
        let new = root.join("new");

        touch(&old.join("57/serde/1.0.0.json"), "json");
        touch(&old.join("57/serde/notes.txt"), "not ours");
        touch(&old.join("README"), "not ours either");
        // Not semver → not a file the legacy layout produced.
        touch(&old.join("57/serde/latest.json"), "not ours");
        // A fresh temp file: a concurrent writer may still own it.
        touch(&old.join("57/tokio/1.0.0.json.tag.rkyv.tmp.1.0"), "tmp");

        let stats = migrate(&old, &new);

        assert!(new.join("docs/serde/1.0.0/57.json").exists());
        assert!(old.join("57/serde/notes.txt").exists());
        assert!(old.join("README").exists());
        assert!(old.join("57/serde/latest.json").exists());
        assert!(old.join("57/tokio/1.0.0.json.tag.rkyv.tmp.1.0").exists());
        assert_eq!(
            stats,
            MigrationStats {
                moved: 1,
                cleaned: 0,
                left: 4
            }
        );

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn flat_dir_merges_into_existing_destination() {
        let root = temp_root("flat-merge");
        let old = root.join("old");
        let new = root.join("new");

        touch(&old.join("crates-io-versions/serde.json"), "old");
        touch(&old.join("crates-io-versions/tokio.json"), "old");
        touch(&new.join("crates-io-versions/serde.json"), "new");

        let stats = migrate(&old, &new);

        assert_eq!(
            fs::read_to_string(new.join("crates-io-versions/serde.json")).unwrap(),
            "new"
        );
        assert_eq!(
            fs::read_to_string(new.join("crates-io-versions/tokio.json")).unwrap(),
            "old"
        );
        assert!(!old.exists());
        assert_eq!(
            stats,
            MigrationStats {
                moved: 1,
                cleaned: 1,
                left: 0
            }
        );

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn classify_maps_legacy_names() {
        assert_eq!(
            classify("1.40.0.json", "57"),
            Some(("1.40.0", "57.json".into()))
        );
        assert_eq!(
            classify("1.40.0.index", "57"),
            Some(("1.40.0", "57.index".into()))
        );
        assert_eq!(
            classify("1.40.0.json.rkyv4.0.8.17-fmt60-aarch64-64.rkyv", "57"),
            Some((
                "1.40.0",
                "57.json.rkyv4.0.8.17-fmt60-aarch64-64.rkyv".into()
            ))
        );
        assert_eq!(classify("latest.json", "57"), None);
        assert_eq!(classify("notes.txt", "57"), None);
    }
}
