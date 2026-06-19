//! Workspace-local build provenance, persisted at `<target-dir>/ferritin.json`.
//!
//! Records the inputs that produced each cached rustdoc JSON so the cache can be
//! invalidated correctly. Currently only the cargo [`FeatureSelection`] is
//! tracked; content hashes and toolchain version are natural future additions
//! that slot in beside `features` without reshaping the file.
//!
//! This lives under `target/` deliberately: it describes artifacts in
//! `target/doc/` and should be discarded along with them by `cargo clean`. User
//! preferences (which should *survive* `cargo clean`) do not belong here.

use super::FeatureSelection;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Current on-disk format version for `ferritin.json`.
const FORMAT: u32 = 1;

/// The contents of `<target-dir>/ferritin.json`.
#[derive(Debug, Default, Serialize, Deserialize)]
pub(super) struct WorkspaceMetadata {
    #[serde(default)]
    format: u32,
    #[serde(default)]
    crates: FxHashMap<String, CrateRecord>,
    /// Absolute path the metadata was loaded from / will be written to.
    #[serde(skip)]
    path: PathBuf,
}

/// Provenance for a single cached crate.
#[derive(Debug, Default, Serialize, Deserialize)]
struct CrateRecord {
    #[serde(default)]
    features: FeatureSelection,
}

impl WorkspaceMetadata {
    /// Read provenance for the given target directory. A missing or unparseable
    /// file yields empty provenance (everything looks un-built), which is safe:
    /// the worst outcome is an unnecessary rebuild.
    pub(super) fn load(target_dir: &Path) -> Self {
        let path = target_dir.join("ferritin.json");
        let mut metadata: Self = std::fs::read(&path)
            .ok()
            .and_then(|bytes| sonic_rs::from_slice(&bytes).ok())
            .unwrap_or_default();
        metadata.path = path;
        metadata
    }

    /// The feature selection the cached docs for `crate_name` were built with, if known.
    pub(super) fn features(&self, crate_name: &str) -> Option<&FeatureSelection> {
        self.crates.get(crate_name).map(|record| &record.features)
    }

    /// Record the feature selection used to build `crate_name` and persist to disk.
    ///
    /// A write failure is logged but not propagated: the docs were still built;
    /// we just won't remember the selection, so the next run rebuilds.
    pub(super) fn set_features(&mut self, crate_name: &str, features: FeatureSelection) {
        self.format = FORMAT;
        self.crates
            .insert(crate_name.to_string(), CrateRecord { features });
        if let Err(error) = self.save() {
            log::warn!("failed to write {}: {error}", self.path.display());
        }
    }

    /// Atomically write the metadata file (temp + rename) so concurrent ferritin
    /// processes can't observe a half-written file. The temp name is
    /// process-unique to avoid two writers clobbering each other's temp; the
    /// final rename is last-writer-wins, whose worst case is a spurious rebuild.
    fn save(&self) -> std::io::Result<()> {
        let json = sonic_rs::to_string(self).map_err(std::io::Error::other)?;
        let tmp = self
            .path
            .with_file_name(format!("ferritin.json.{}.tmp", std::process::id()));
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, &self.path)
    }
}
