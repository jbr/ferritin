use super::CrateProvenance;
use super::FeatureSelection;
use super::workspace_metadata::WorkspaceMetadata;
use crate::RustdocData;
use crate::crate_name::CrateName;
use crate::navigator::CrateInfo;
use crate::sources::RustdocVersion;
use crate::sources::Source;
use anyhow::{Result, anyhow};
use cargo_metadata::MetadataCommand;
use fieldwork::Fieldwork;
use rustc_hash::FxHashMap;
use rustc_hash::FxHashSet;
use semver::Version;
use semver::VersionReq;
use std::borrow::Cow;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;
use walkdir::WalkDir;

#[derive(Debug, Fieldwork)]
#[field(get)]
pub struct LocalSource {
    manifest_path: PathBuf,
    target_dir: PathBuf,
    #[field = false]
    crates: FxHashMap<CrateName<'static>, CrateInfo>,
    root_crate: Option<CrateName<'static>>,
    can_rebuild: bool,
    /// One-shot flag: when set, the next crate loaded is rebuilt unconditionally,
    /// bypassing the freshness/version cache checks. Consumed on first use so that
    /// only the crate the user queried is forced — cross-crate dependencies loaded
    /// afterward fall back to normal caching.
    #[field = false]
    force_rebuild: AtomicBool,
    /// One-shot cargo feature selection for the next crate loaded (the queried
    /// one). `Some` means the user passed feature flags this invocation; consumed
    /// on first use, like [`Self::force_rebuild`], because features are per-crate
    /// — cross-crate dependencies loaded afterward use their own cached selection.
    #[field = false]
    requested_features: Mutex<Option<FeatureSelection>>,
}

impl LocalSource {
    pub fn load(path: &Path) -> Result<Self> {
        let metadata = if path.is_dir() {
            MetadataCommand::new().current_dir(path).exec()?
        } else if path.file_name().and_then(|n| n.to_str()) == Some("Cargo.toml") {
            if !path.exists() {
                return Err(anyhow!("Cargo.toml not found at {}", path.display()));
            }
            MetadataCommand::new().manifest_path(path).exec()?
        } else {
            return Err(anyhow!(
                "Path must be a directory or Cargo.toml file, got: {}",
                path.display()
            ));
        };

        let manifest_path: PathBuf = metadata.workspace_root.join("Cargo.toml").into();
        let mut reverse_deps: FxHashMap<&str, FxHashSet<&str>> = FxHashMap::default();

        let mut workspace_packages: FxHashSet<&str> = FxHashSet::default();

        for package in metadata.workspace_packages() {
            workspace_packages.insert(&package.name);
            for dep in &package.dependencies {
                reverse_deps
                    .entry(&dep.name)
                    .or_default()
                    .insert(&package.name);
            }
        }

        let target_dir = metadata.target_directory.clone().into_std_path_buf();
        let root_crate = metadata
            .root_package()
            .map(|p| CrateName::from(p.name.to_string()));

        let mut crates = FxHashMap::default();
        for package in &metadata.packages {
            // let is_crates_io = package
            //     .source
            //     .as_ref()
            //     .map(|s| s.repr.starts_with("registry+"))
            //     .unwrap_or(false);

            let provenance = if workspace_packages.contains(&**package.name) {
                CrateProvenance::Workspace
            } else {
                CrateProvenance::LocalDependency
            };

            let used_by = reverse_deps
                .get(&**package.name)
                .into_iter()
                .flatten()
                .map(|name| name.to_string())
                .collect();

            let doc_dir = target_dir.join("doc");
            let underscored = package.name.replace('-', "_");
            let json_path = doc_dir.join(format!("{underscored}.json"));

            crates.insert(
                package.name.to_string().into(),
                CrateInfo {
                    provenance,
                    version: Some(package.version.clone()),
                    description: package.description.clone(),
                    name: package.name.to_string(),
                    default_crate: root_crate
                        .as_ref()
                        .is_some_and(|dc| &CrateName::from(&**package.name) == dc),
                    used_by,
                    json_path: Some(json_path),
                },
            );
        }

        Ok(Self {
            manifest_path,
            target_dir,
            can_rebuild: true,
            force_rebuild: AtomicBool::new(false),
            requested_features: Mutex::new(None),
            crates,
            root_crate,
        })
    }

    /// Force the next loaded crate to be rebuilt, ignoring the cache.
    ///
    /// Only the first crate loaded is affected; see [`Self::force_rebuild`].
    pub fn with_force_rebuild(self, force: bool) -> Self {
        self.force_rebuild.store(force, Ordering::Relaxed);
        self
    }

    /// Request a cargo feature selection for the next crate loaded.
    ///
    /// `None` (no feature flags given) leaves the sticky cached selection in
    /// place; see [`Self::requested_features`] and [`Self::resolve_features`].
    pub fn with_features(self, features: Option<FeatureSelection>) -> Self {
        *self.requested_features.lock().unwrap() = features;
        self
    }

    /// Decide which feature selection to build with and whether the requested
    /// selection alone forces a rebuild (independent of source freshness).
    ///
    /// The model is *sticky*: a build's feature selection is persisted as
    /// provenance (`cached`) and inherited by later bare invocations, so you only
    /// type `--features` once and subsequent lookups ride the cache.
    ///
    /// - **`--rebuild`** → a clean build at the *requested* selection, or plain
    ///   default if none were given. This is the escape hatch back to default.
    /// - **explicit features** → build with them; rebuild only if they differ
    ///   from what the cache was last built with (`cached`).
    /// - **no features** → inherit `cached` (sticky); never rebuild on this
    ///   account. This is what survives `--features` across `src` edits: an
    ///   mtime-triggered rebuild still uses the inherited selection.
    fn resolve_features(
        requested: Option<FeatureSelection>,
        cached: Option<&FeatureSelection>,
        force_rebuild: bool,
    ) -> (FeatureSelection, bool) {
        match (force_rebuild, requested) {
            (true, requested) => (requested.unwrap_or_default(), true),
            (false, Some(requested)) => {
                let differs = cached != Some(&requested);
                (requested, differs)
            }
            (false, None) => (cached.cloned().unwrap_or_default(), false),
        }
    }

    /// Check if a crate name is a workspace package
    pub fn is_workspace_package(&self, crate_name: &str) -> bool {
        let crate_name = CrateName::from(crate_name);
        self.crates
            .get(&crate_name)
            .is_some_and(|crate_info| crate_info.provenance.is_workspace())
    }

    /// Get the resolved version for a dependency
    /// Returns None if not a dependency or if it's a path/workspace dep
    pub fn get_dependency_version<'a, 'b: 'a>(
        &'a self,
        crate_name: &'b str,
    ) -> Option<&'a Version> {
        let crate_name = CrateName::from(crate_name);
        self.crates
            .get(&crate_name)
            .and_then(|lsm| lsm.version.as_ref())
    }

    /// Get the project root
    pub fn project_root(&self) -> &Path {
        self.manifest_path.parent().unwrap_or(&self.manifest_path)
    }

    /// Check if this source can provide a given crate
    pub fn can_load(&self, crate_name: &str) -> bool {
        self.crates.contains_key(crate_name)
    }

    /// Get the JSON path for a crate
    fn json_path(&self, crate_name: &str) -> PathBuf {
        let doc_dir = self.target_dir.join("doc");
        let underscored = crate_name.replace('-', "_");
        doc_dir.join(format!("{underscored}.json"))
    }

    /// Load a workspace crate (may rebuild if needed)
    pub fn load_workspace_crate(&self, crate_name: CrateName<'_>) -> Option<RustdocData> {
        let json_path = self.json_path(crate_name.as_ref());
        let force_rebuild = self.force_rebuild.swap(false, Ordering::Relaxed);
        let requested = self.requested_features.lock().unwrap().take();

        let mut metadata = WorkspaceMetadata::load(&self.target_dir);
        let (features, mut feature_rebuild) = Self::resolve_features(
            requested,
            metadata.features(crate_name.as_ref()),
            force_rebuild,
        );

        let mut tried_rebuilding = false;
        loop {
            let needs_rebuild = feature_rebuild
                || json_path
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .is_none_or(|docs_updated| {
                        WalkDir::new(self.project_root().join("src"))
                            .into_iter()
                            .filter_map(|entry| -> Option<SystemTime> {
                                entry.ok()?.metadata().ok()?.modified().ok()
                            })
                            .any(|file_updated| file_updated > docs_updated)
                    });
            feature_rebuild = false;

            if !needs_rebuild
                && let Ok(content) = std::fs::read(&json_path)
                && let Ok(crate_data) = crate::conversions::load_and_normalize(&content, None)
            {
                let version = crate_data
                    .crate_version
                    .as_ref()
                    .and_then(|v| Version::parse(v).ok());

                break Some(RustdocData {
                    crate_data,
                    name: crate_name.to_string(),
                    provenance: CrateProvenance::Workspace,
                    fs_path: json_path,
                    version,
                    path_to_id: Default::default(),
                });
            } else if !tried_rebuilding && self.can_rebuild {
                tried_rebuilding = true;
                if self
                    .rebuild_docs(&crate_name, None, true, &features)
                    .is_ok()
                {
                    metadata.set_features(crate_name.as_ref(), features.clone());
                    continue;
                }
            }
            break None;
        }
    }

    /// Load a dependency crate (may rebuild if needed)
    pub fn load_dep(
        &self,
        crate_name: CrateName<'_>,
        version: Option<&Version>,
    ) -> Option<RustdocData> {
        let info = self.lookup(&crate_name, &VersionReq::STAR)?;
        let json_path = info.json_path.as_deref()?;
        let info_version = info.version.as_ref();

        if let Some(version) = version
            && let Some(info_version) = info_version
            && version != info_version
        {
            return None;
        }

        let force_rebuild = self.force_rebuild.swap(false, Ordering::Relaxed);
        let requested = self.requested_features.lock().unwrap().take();

        let mut metadata = WorkspaceMetadata::load(&self.target_dir);
        let (features, mut feature_rebuild) = Self::resolve_features(
            requested,
            metadata.features(crate_name.as_ref()),
            force_rebuild,
        );

        let mut tried_rebuilding = false;
        loop {
            if !feature_rebuild
                && let Ok(content) = std::fs::read(json_path)
                && let Ok(RustdocVersion { crate_version, .. }) =
                    sonic_rs::serde::from_slice(&content)
                && crate_version.as_ref() == version
                && let Ok(crate_data) = crate::conversions::load_and_normalize(&content, None)
            {
                let version = crate_data
                    .crate_version
                    .as_ref()
                    .and_then(|v| Version::parse(v).ok());

                break Some(RustdocData {
                    crate_data,
                    name: crate_name.to_string(),
                    provenance: CrateProvenance::LocalDependency,
                    fs_path: json_path.to_owned(),
                    version,
                    path_to_id: Default::default(),
                });
            } else if !tried_rebuilding && self.can_rebuild {
                tried_rebuilding = true;
                feature_rebuild = false;
                if self
                    .rebuild_docs(&crate_name, version, false, &features)
                    .is_ok()
                {
                    metadata.set_features(crate_name.as_ref(), features.clone());
                    continue;
                }
            }
            break None;
        }
    }

    /// Rebuild documentation for a crate.
    ///
    /// `document_private` enables `--document-private-items`, which is desirable for
    /// workspace crates (you're editing them) but not for local dependencies (you want
    /// their public API surface).
    fn rebuild_docs(
        &self,
        crate_name: &CrateName<'_>,
        version: Option<&Version>,
        document_private: bool,
        features: &FeatureSelection,
    ) -> Result<()> {
        let package_spec = match version {
            Some(v) => format!("{}@{}", crate_name, v),
            None => crate_name.to_string(),
        };

        let mut rustdocflags = String::from("-Z unstable-options --output-format=json");
        if document_private {
            rustdocflags.push_str(" --document-private-items");
        }

        let output = Command::new("rustup")
            .arg("run")
            .args([
                "nightly",
                "cargo",
                "doc",
                "--no-deps",
                "--package",
                &package_spec,
            ])
            .args(features.cargo_args())
            .env("RUSTDOCFLAGS", rustdocflags)
            .current_dir(self.project_root())
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("cargo doc failed: {}", stderr));
        }
        Ok(())
    }
}

impl Source for LocalSource {
    fn lookup<'a>(&'a self, name: &str, _version: &VersionReq) -> Option<Cow<'a, CrateInfo>> {
        // Handle "crate" alias for single-package workspaces
        let search_name = if name == "crate" {
            self.root_crate()?
        } else {
            &CrateName::from(name.to_owned())
        };

        self.crates.get(search_name).map(Cow::Borrowed)
    }

    fn load(&self, crate_name: &str, version: Option<&Version>) -> Option<RustdocData> {
        let crate_name = CrateName::from(crate_name);

        if self.is_workspace_package(&crate_name) {
            self.load_workspace_crate(crate_name)
        } else {
            self.load_dep(crate_name, version)
        }
    }

    fn list_available<'a>(&'a self) -> Box<dyn Iterator<Item = &'a CrateInfo> + '_> {
        Box::new(self.crates.values().filter(|crate_info| {
            crate_info.provenance.is_workspace()
                || match self.root_crate.as_ref() {
                    Some(rc) => crate_info
                        .used_by()
                        .iter()
                        .any(|u| &CrateName::from(&**u) == rc),
                    None => !crate_info.used_by().is_empty(),
                }
        }))
    }

    fn canonicalize(&self, input_name: &str) -> Option<CrateName<'static>> {
        self.crates
            .get_key_value(input_name)
            .map(|(k, _)| k.clone())
    }
}

// .filter(|c| {
//     root_crate.is_none_or(|rc| {
//         !c.provenance().is_local_dependency() || c.used_by().iter().any(|u| **u == **rc)
//     })
// })
