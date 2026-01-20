//! Documentation sources
//!
//! This module defines different sources for rustdoc JSON data:
//! - StdSource: rustup-managed std library docs
//! - LocalSource: workspace-local crates (built on demand)
//! - DocsRsSource: fetched from docs.rs and cached

use crate::crate_name::CrateName;
use crate::docsrs_client::DocsRsClient;
use crate::project::{CrateProvenance, LocalContext, RUST_CRATES, RustdocData};
use anyhow::{Result, anyhow};
use rustdoc_types::{Crate, FORMAT_VERSION};
use serde::Deserialize;
use std::path::PathBuf;
use std::process::Command;
use std::time::SystemTime;
use walkdir::WalkDir;

#[derive(Deserialize, Debug)]
struct RustdocVersion {
    format_version: u32,
    crate_version: Option<String>,
}

/// Source for std library documentation (rustup-managed)
#[derive(Debug, Clone, fieldwork::Fieldwork)]
#[field(get)]
pub struct StdSource {
    docs_path: PathBuf,
    rustc_version: String,
}

impl StdSource {
    /// Try to create a StdSource from the current rustup installation
    pub fn from_rustup() -> Option<Self> {
        let sysroot = Command::new("rustup")
            .args(["run", "nightly", "rustc", "--print", "sysroot"])
            .output()
            .ok()?;

        if !sysroot.status.success() {
            return None;
        }

        let s = std::str::from_utf8(&sysroot.stdout).ok()?;
        let docs_path = PathBuf::from(s.trim()).join("share/doc/rust/json/");

        let version = Command::new("rustup")
            .args(["run", "nightly", "rustc", "--version", "--verbose"])
            .output()
            .ok()?;

        if !version.status.success() {
            return None;
        }

        let rustc_version = std::str::from_utf8(&version.stdout)
            .ok()?
            .lines()
            .find_map(|line| line.strip_prefix("release: "))?
            .to_string();

        docs_path.exists().then_some(Self {
            docs_path,
            rustc_version,
        })
    }

    /// Check if this source can provide a given crate
    pub fn can_load(&self, crate_name: CrateName<'_>) -> bool {
        RUST_CRATES.contains(&crate_name)
    }

    /// Load a std library crate
    pub fn load(&self, crate_name: CrateName<'_>) -> Option<RustdocData> {
        if !self.can_load(crate_name) {
            return None;
        }

        let json_path = self.docs_path.join(format!("{crate_name}.json"));

        if let Ok(content) = std::fs::read_to_string(&json_path)
            && let Ok(RustdocVersion { format_version, .. }) = serde_json::from_str(&content)
            && format_version == FORMAT_VERSION
        {
            let crate_data: Crate = serde_json::from_str(&content).ok()?;

            Some(RustdocData {
                crate_data,
                name: crate_name.to_string(),
                crate_type: CrateProvenance::Rust,
                fs_path: json_path,
            })
        } else {
            None
        }
    }

    /// List all available crates from this source
    pub fn list_available(&self) -> impl Iterator<Item = CrateName<'static>> {
        RUST_CRATES.iter().copied()
    }
}

/// Source for locally-built workspace documentation
pub struct LocalSource {
    context: LocalContext,
    target_dir: PathBuf,
    can_rebuild: bool,
}

impl std::fmt::Debug for LocalSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalSource")
            .field("target_dir", &self.target_dir)
            .field("can_rebuild", &self.can_rebuild)
            .finish()
    }
}

impl LocalSource {
    /// Create a new LocalSource
    pub fn new(context: LocalContext, can_rebuild: bool) -> Self {
        let target_dir = context.project_root().join("target");
        Self {
            context,
            target_dir,
            can_rebuild,
        }
    }

    /// Check if this source can provide a given crate
    pub fn can_load(&self, crate_name: &str) -> bool {
        self.context.is_workspace_package(crate_name)
            || self.context.get_dependency_version(crate_name).is_some()
    }

    /// Get the JSON path for a crate
    fn json_path(&self, crate_name: &str) -> PathBuf {
        let doc_dir = self.target_dir.join("doc");
        let underscored = crate_name.replace('-', "_");
        doc_dir.join(format!("{underscored}.json"))
    }

    /// Load a workspace crate (may rebuild if needed)
    pub fn load_workspace(&self, crate_name: CrateName<'_>) -> Option<RustdocData> {
        let json_path = self.json_path(crate_name.as_ref());
        let mut tried_rebuilding = false;

        loop {
            let needs_rebuild = json_path
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .is_none_or(|docs_updated| {
                    WalkDir::new(self.context.project_root().join("src"))
                        .into_iter()
                        .filter_map(|entry| -> Option<SystemTime> {
                            entry.ok()?.metadata().ok()?.modified().ok()
                        })
                        .any(|file_updated| file_updated > docs_updated)
                });

            if !needs_rebuild
                && let Ok(content) = std::fs::read_to_string(&json_path)
                && let Ok(RustdocVersion { format_version, .. }) = serde_json::from_str(&content)
                && format_version == FORMAT_VERSION
            {
                let crate_data: Crate = serde_json::from_str(&content).ok()?;

                break Some(RustdocData {
                    crate_data,
                    name: crate_name.to_string(),
                    crate_type: CrateProvenance::Workspace,
                    fs_path: json_path,
                });
            } else if !tried_rebuilding && self.can_rebuild {
                tried_rebuilding = true;
                if self.rebuild_docs(crate_name).is_ok() {
                    continue;
                }
            }
            break None;
        }
    }

    /// Load a dependency crate (may rebuild if needed)
    pub fn load_dep(&self, crate_name: CrateName<'_>) -> Option<RustdocData> {
        let json_path = self.json_path(crate_name.as_ref());
        let mut tried_rebuilding = false;
        let expected_version = self.context.get_dependency_version(crate_name.as_ref());

        loop {
            if let Ok(content) = std::fs::read_to_string(&json_path)
                && let Ok(RustdocVersion {
                    format_version,
                    crate_version,
                }) = serde_json::from_str(&content)
                && format_version == FORMAT_VERSION
                && crate_version.as_deref() == expected_version
            {
                let crate_data: Crate = serde_json::from_str(&content).ok()?;

                break Some(RustdocData {
                    crate_data,
                    name: crate_name.to_string(),
                    crate_type: CrateProvenance::Library,
                    fs_path: json_path,
                });
            } else if !tried_rebuilding && self.can_rebuild {
                tried_rebuilding = true;
                if self.rebuild_docs(crate_name).is_ok() {
                    continue;
                }
            }
            break None;
        }
    }

    /// Rebuild documentation for a crate
    fn rebuild_docs(&self, crate_name: CrateName<'_>) -> Result<()> {
        let output = Command::new("rustup")
            .arg("run")
            .args([
                "nightly",
                "cargo",
                "doc",
                "--no-deps",
                "--package",
                &*crate_name,
            ])
            .env("RUSTDOCFLAGS", "-Z unstable-options --output-format=json")
            .current_dir(self.context.project_root())
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("cargo doc failed: {}", stderr));
        }
        Ok(())
    }

    /// List all available crates from this source
    pub fn list_available(&self) -> impl Iterator<Item = String> + '_ {
        self.context
            .workspace_packages()
            .iter()
            .cloned()
            .chain(self.context.dependencies().map(|(name, _)| name.clone()))
    }

    /// Get the local context
    pub fn context(&self) -> &LocalContext {
        &self.context
    }
}

/// Source for docs.rs documentation
pub struct DocsRsSource {
    client: DocsRsClient,
}

impl std::fmt::Debug for DocsRsSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DocsRsSource").finish()
    }
}

impl DocsRsSource {
    /// Create a new DocsRsSource with a cache directory
    pub fn new(cache_dir: PathBuf) -> Result<Self> {
        let client = DocsRsClient::new(cache_dir)?;
        Ok(Self { client })
    }

    /// Try to create from default cache location
    pub fn from_default_cache() -> Option<Self> {
        let cache_dir = home::cargo_home().ok()?.join("rustdoc-json");
        DocsRsClient::new(cache_dir)
            .ok()
            .map(|client| Self { client })
    }

    /// Load a crate from docs.rs
    pub async fn load(
        &self,
        crate_name: &str,
        version: Option<&str>,
    ) -> Result<Option<RustdocData>> {
        self.client.get_crate(crate_name, version).await
    }

    /// Load a crate from docs.rs (blocking version)
    pub fn load_blocking(
        &self,
        crate_name: &str,
        version: Option<&str>,
    ) -> Result<Option<RustdocData>> {
        trillium_smol::async_global_executor::block_on(async {
            self.load(crate_name, version).await
        })
    }

    /// Docs.rs has unbounded crates, so we don't provide a list
    /// This method exists for API consistency but always returns None
    pub fn list_available(&self) -> Option<std::iter::Empty<String>> {
        None
    }
}
