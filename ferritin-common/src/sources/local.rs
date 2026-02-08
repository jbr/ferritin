use super::CrateProvenance;
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
use rustdoc_types::{Crate, FORMAT_VERSION};
use semver::Version;
use semver::VersionReq;
use std::borrow::Cow;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
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
            crates,
            root_crate,
        })
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
        let mut tried_rebuilding = false;

        loop {
            let needs_rebuild = json_path
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

            if !needs_rebuild
                && let Ok(content) = std::fs::read(&json_path)
                && let Ok(format_version) = sonic_rs::get_from_slice(&content, &["format_version"])
                && let Ok(FORMAT_VERSION) = format_version.as_raw_str().parse()
            {
                let crate_data: Crate = sonic_rs::serde::from_slice(&content).ok()?;
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
                });
            } else if !tried_rebuilding && self.can_rebuild {
                tried_rebuilding = true;
                if self.rebuild_docs(&crate_name).is_ok() {
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

        let mut tried_rebuilding = false;

        loop {
            if let Ok(content) = std::fs::read(json_path)
                && let Ok(RustdocVersion {
                    format_version,
                    crate_version,
                }) = sonic_rs::serde::from_slice(&content)
                && format_version == FORMAT_VERSION
                && crate_version.as_ref() == version
            {
                let crate_data: Crate = sonic_rs::serde::from_slice(&content).ok()?;
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
                });
            } else if !tried_rebuilding && self.can_rebuild {
                tried_rebuilding = true;
                if self.rebuild_docs(&crate_name).is_ok() {
                    continue;
                }
            }
            break None;
        }
    }

    /// Rebuild documentation for a crate
    fn rebuild_docs(&self, crate_name: &CrateName<'_>) -> Result<()> {
        let output = Command::new("rustup")
            .arg("run")
            .args([
                "nightly",
                "cargo",
                "doc",
                "--no-deps",
                "--package",
                crate_name,
            ])
            .env("RUSTDOCFLAGS", "-Z unstable-options --output-format=json")
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
