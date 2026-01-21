use anyhow::{Result, anyhow};
use cargo_metadata::{DependencyKind, Metadata, MetadataCommand};
use cargo_toml::Manifest;
use fieldwork::Fieldwork;
use rustdoc_types::{Crate, ExternalCrate, FORMAT_VERSION, Id, Item};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt::{self, Debug, Formatter};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;
use walkdir::WalkDir;

use crate::crate_name::CrateName;
use crate::doc_ref::{self, DocRef};
use crate::docsrs_client::DocsRsClient;
use crate::navigator::{Navigator, parse_docsrs_url};

pub const RUST_CRATES: [CrateName<'_>; 5] = [
    CrateName("std"),
    CrateName("alloc"),
    CrateName("core"),
    CrateName("proc_macro"),
    CrateName("test"),
];

/// Key for identifying crates in the working set
/// Version is None for workspace/local crates, Some(semver) for published crates
pub type CrateKey = (String, Option<String>);

/// Metadata about the local workspace - doesn't own documentation data
/// This can be cached to disk with content hash over Cargo.lock/Cargo.toml
#[derive(Debug, Clone)]
pub struct LocalContext {
    manifest_path: PathBuf,
    // commented out because currently unused
    //    metadata: Metadata,
    workspace_packages: Vec<String>,
    /// Resolved dependencies from Cargo.lock
    /// Maps crate name to (version, is_crates_io)
    /// version is None for path/workspace deps
    resolved_deps: BTreeMap<String, (Option<String>, bool)>,
}

impl LocalContext {
    /// Create a new LocalContext from a manifest path
    pub fn load(path: PathBuf) -> Result<Self> {
        let metadata = if path.is_dir() {
            MetadataCommand::new().current_dir(&path).exec()?
        } else if path.file_name().and_then(|n| n.to_str()) == Some("Cargo.toml") {
            if !path.exists() {
                return Err(anyhow!("Cargo.toml not found at {}", path.display()));
            }
            MetadataCommand::new().manifest_path(&path).exec()?
        } else {
            return Err(anyhow!(
                "Path must be a directory or Cargo.toml file, got: {}",
                path.display()
            ));
        };

        let manifest_path: PathBuf = metadata.workspace_root.join("Cargo.toml").into();

        let workspace_packages: Vec<String> = metadata
            .workspace_packages()
            .iter()
            .map(|package| package.name.to_string())
            .collect();

        // Build resolved_deps from metadata
        let mut resolved_deps = BTreeMap::new();
        for package in &metadata.packages {
            // Only include dependencies, not workspace members
            if !workspace_packages.contains(&package.name) {
                // Determine if it's from crates.io (has no path, no git)
                let is_crates_io = package
                    .source
                    .as_ref()
                    .map(|s| s.repr.starts_with("registry+"))
                    .unwrap_or(false);

                let version = if is_crates_io {
                    Some(package.version.to_string())
                } else {
                    None
                };

                resolved_deps.insert(package.name.to_string(), (version, is_crates_io));
            }
        }

        Ok(Self {
            manifest_path,
            //            metadata,
            workspace_packages,
            resolved_deps,
        })
    }

    /// Check if a crate name is a workspace package
    pub fn is_workspace_package(&self, crate_name: &str) -> bool {
        self.workspace_packages
            .iter()
            .any(|c| eq_ignoring_dash_underscore(c, crate_name))
    }

    /// Get the resolved version for a dependency
    /// Returns None if not a dependency or if it's a path/workspace dep
    pub fn get_dependency_version(&self, crate_name: &str) -> Option<&str> {
        self.resolved_deps
            .iter()
            .find(|(name, _)| eq_ignoring_dash_underscore(name, crate_name))
            .and_then(|(_, (version, _))| version.as_deref())
    }

    /// Get all workspace package names
    pub fn workspace_packages(&self) -> &[String] {
        &self.workspace_packages
    }

    /// Get the manifest path
    pub fn manifest_path(&self) -> &Path {
        &self.manifest_path
    }

    /// Get the project root
    pub fn project_root(&self) -> &Path {
        self.manifest_path.parent().unwrap_or(&self.manifest_path)
    }

    /// Get an iterator over resolved dependencies
    pub fn dependencies(&self) -> impl Iterator<Item = (&String, &(Option<String>, bool))> {
        self.resolved_deps.iter()
    }
}

/// Get the cache directory for rustdoc JSON files
///
/// Uses `{CARGO_HOME}/rustdoc-json/` as the cache location
fn docsrs_cache_dir() -> Result<PathBuf> {
    Ok(home::cargo_home()?.join("rustdoc-json"))
}

/// Manages a Cargo project and its rustdoc JSON files
#[derive(Fieldwork)]
#[fieldwork(get)]
pub struct RustdocProject {
    manifest_path: PathBuf,
    target_dir: PathBuf,
    manifest: Manifest,
    metadata: Metadata,
    #[field = false]
    crate_info: Vec<CrateInfo>,
    workspace_packages: Box<[String]>,
    #[field = false]
    available_crates: Vec<String>,
    rustc_docs: Option<(PathBuf, String)>,
    #[field = false]
    docsrs_client: Option<DocsRsClient>,
}

impl Debug for RustdocProject {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("RustdocProject")
            .field("manifest_path", &self.manifest_path)
            .field("target_dir", &self.target_dir)
            .field("crate_info", &self.crate_info)
            .finish_non_exhaustive()
    }
}

pub(crate) fn rustc_docs() -> Option<(PathBuf, String)> {
    let sysroot = Command::new("rustup")
        .args(["run", "nightly", "rustc", "--print", "sysroot"])
        .output()
        .ok()?;

    if !sysroot.status.success() {
        return None;
    }

    let s = str::from_utf8(&sysroot.stdout).ok()?;

    let path = PathBuf::from(s.trim()).join("share/doc/rust/json/");

    let version = Command::new("rustup")
        .args(["run", "nightly", "rustc", "--version", "--verbose"])
        .arg("run")
        .output()
        .ok()?;

    if !version.status.success() {
        return None;
    }

    let version = str::from_utf8(&version.stdout)
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix("release: "))?
        .to_string();

    path.exists().then_some((path, version))
}

fn eq_ignoring_dash_underscore(a: &str, b: &str) -> bool {
    let mut a = a.chars();
    let mut b = b.chars();
    loop {
        match (a.next(), b.next()) {
            (Some('_'), Some('-')) | (Some('-'), Some('_')) => {}
            (Some(a), Some(b)) if a == b => {}
            (None, None) => break true,
            _ => break false,
        }
    }
}

impl RustdocProject {
    /// Create a new project from a path (directory or Cargo.toml)
    ///
    /// If given a directory, walks up to find the workspace root like cargo does.
    /// If given a Cargo.toml path, uses that directly.
    pub fn load(path: PathBuf) -> Result<Self> {
        // Run MetadataCommand once, using either current_dir or manifest_path
        let metadata = if path.is_dir() {
            // Use cargo_metadata to find the manifest, which will walk up from current_dir
            MetadataCommand::new().current_dir(&path).exec()?
        } else if path.file_name().and_then(|n| n.to_str()) == Some("Cargo.toml") {
            // It's already a Cargo.toml path
            if !path.exists() {
                return Err(anyhow!("Cargo.toml not found at {}", path.display()));
            }
            MetadataCommand::new().manifest_path(&path).exec()?
        } else {
            return Err(anyhow!(
                "Path must be a directory or Cargo.toml file, got: {}",
                path.display()
            ));
        };

        // Get the manifest path from metadata (workspace root)
        let manifest_path: PathBuf = metadata.workspace_root.join("Cargo.toml").into();

        let manifest = Manifest::from_path(&manifest_path)?;
        let project_root = manifest_path
            .parent()
            .ok_or_else(|| anyhow!("Invalid manifest path"))?;

        let target_dir: PathBuf = project_root.join("target");

        let workspace_packages: Vec<String> = metadata
            .workspace_packages()
            .iter()
            .map(|package| package.name.to_string())
            .collect();

        let rustc_docs = rustc_docs();

        // Initialize DocsRsClient for fetching from docs.rs
        // This is optional - if it fails, we just won't have remote fetching capability
        let docsrs_client = docsrs_cache_dir()
            .and_then(|cache_dir| {
                log::debug!(
                    "Initializing DocsRsClient with cache dir: {}",
                    cache_dir.display()
                );
                DocsRsClient::new(cache_dir)
            })
            .ok();

        if docsrs_client.is_some() {
            log::debug!("DocsRsClient initialized successfully");
        } else {
            log::warn!("Failed to initialize docs.rs client, remote fetching will be disabled");
        }

        let mut project = Self {
            manifest_path,
            manifest,
            target_dir,
            metadata,
            crate_info: vec![],
            workspace_packages: workspace_packages.into(),
            available_crates: vec![],
            rustc_docs,
            docsrs_client,
        };

        project.crate_info = project.generate_crate_info();
        project.available_crates = project
            .crate_info(None)
            .map(|c| c.name().to_owned())
            .collect();
        Ok(project)
    }

    pub(crate) fn resolve_json_path<'a>(
        &'a self,
        crate_name: CrateName<'a>,
    ) -> Option<(PathBuf, CrateProvenance)> {
        let doc_dir = self.target_dir.join("doc");

        if RUST_CRATES.contains(&crate_name)
            && let Some((rustc_docs, _)) = &self.rustc_docs
        {
            Some((
                rustc_docs.join(format!("{crate_name}.json")),
                CrateProvenance::Rust,
            ))
        } else if self
            .available_crates()
            .any(|name| eq_ignoring_dash_underscore(&name, &crate_name))
        {
            let underscored = crate_name.replace('-', "_");
            Some((
                doc_dir.join(format!("{underscored}.json")),
                if self.is_workspace_package(crate_name) {
                    CrateProvenance::Workspace
                } else {
                    CrateProvenance::Library
                },
            ))
        } else {
            None
        }
    }

    pub fn is_workspace_package(&self, crate_name: CrateName<'_>) -> bool {
        self.workspace_packages
            .iter()
            .any(|c| eq_ignoring_dash_underscore(c, &crate_name))
    }

    /// Generate documentation for the project or a specific package
    pub(crate) fn rebuild_docs(&self, crate_name: CrateName<'_>) -> Result<()> {
        let project_root = self.project_root();

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
            .current_dir(project_root)
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("cargo doc failed: {}", stderr));
        }
        Ok(())
    }

    /// Get available crate names and optional descriptions
    /// Always generates full workspace view with used_by tracking
    fn generate_crate_info(&self) -> Vec<CrateInfo> {
        let mut crates = vec![];
        let default_crate = self.default_crate_name();

        // In workspace contexts (>1 package), never alias any crate as "crate"
        let workspace_packages = self.metadata.workspace_packages();
        let is_workspace = workspace_packages.len() > 1;

        // Add workspace members
        for package in &workspace_packages {
            crates.push(CrateInfo {
                crate_type: CrateProvenance::Workspace,
                name: package.name.to_string(),
                description: package.description.clone(),
                version: Some(package.version.to_string()),
                dev_dep: false,
                default_crate: !is_workspace
                    && default_crate
                        .is_some_and(|dc| eq_ignoring_dash_underscore(&dc, &package.name)),
                used_by: vec![], // Workspace members aren't "used by" anyone
            });
        }

        // Collect all dependencies with tracking of which workspace members use them
        let mut dep_usage: BTreeMap<String, Vec<String>> = BTreeMap::new(); // dep_name -> vec of workspace members
        let mut dep_dev_status: BTreeMap<String, bool> = BTreeMap::new(); // dep_name -> is any usage a dev dep

        if workspace_packages.len() > 1 {
            // Multi-crate workspace - collect from all members
            for package in &workspace_packages {
                for dep in &package.dependencies {
                    // Skip workspace-internal dependencies
                    if dep.path.is_some() || self.workspace_packages.contains(&dep.name) {
                        continue;
                    }

                    let is_dev_dep = matches!(dep.kind, DependencyKind::Development);
                    dep_usage
                        .entry(dep.name.clone())
                        .or_default()
                        .push(package.name.to_string());

                    // Mark as dev_dep if ANY usage is dev (we could be more nuanced here)
                    let current_dev_status =
                        dep_dev_status.get(&dep.name).copied().unwrap_or(false);
                    dep_dev_status.insert(dep.name.clone(), current_dev_status || is_dev_dep);
                }
            }
        } else {
            // Single crate - use manifest dependencies
            let single_crate_name = workspace_packages
                .first()
                .map(|p| p.name.to_string())
                .unwrap_or_default();
            for (crate_names, dev_dep) in [
                (self.manifest.dependencies.keys(), false),
                (self.manifest.dev_dependencies.keys(), true),
            ] {
                for crate_name in crate_names {
                    dep_usage
                        .entry(crate_name.clone())
                        .or_default()
                        .push(single_crate_name.clone());
                    dep_dev_status.insert(crate_name.clone(), dev_dep);
                }
            }
        }

        // Convert dependencies to CrateInfo with used_by tracking
        for (dep_name, using_crates) in dep_usage {
            let dev_dep = dep_dev_status.get(&dep_name).copied().unwrap_or(false);
            let metadata = self
                .metadata
                .packages
                .iter()
                .find(|package| eq_ignoring_dash_underscore(&package.name, &dep_name));

            crates.push(CrateInfo {
                crate_type: CrateProvenance::Library,
                version: metadata.map(|p| p.version.to_string()),
                description: metadata.and_then(|p| p.description.clone()),
                dev_dep,
                name: dep_name,
                default_crate: false,
                used_by: using_crates,
            });
        }

        // Add standard library crates
        if let Some((_, rustc_version)) = self.rustc_docs() {
            crates.extend([
                ("std", "The Rust Standard Library"),
                ("alloc","The Rust core allocation and collections library"),
                ("core", "The Rust Core Library"),
                ("proc_macro", "A support library for macro authors when defining new macros"),
                ("test", "Support code for rustc's built in unit-test and micro-benchmarking framework")
            ].map(|(name, description)|{
                CrateInfo {
                    crate_type: CrateProvenance::Rust,
                    version: Some(rustc_version.to_string()),
                    description: Some(description.to_string()),
                    dev_dep: false,
                    name: name.to_string(),
                    default_crate: false,
                    used_by: vec![], // Standard library not tracked by workspace usage
                }})
            );
        }

        crates
    }

    /// Get available crate names and optional descriptions
    pub(crate) fn available_crates(&self) -> impl Iterator<Item = CrateName<'_>> {
        self.available_crates
            .iter()
            .filter_map(|x| CrateName::new(x))
    }

    pub fn project_root(&self) -> &Path {
        self.manifest_path.parent().unwrap_or(&self.manifest_path)
    }

    pub(crate) fn default_crate_name(&self) -> Option<CrateName<'_>> {
        if let Some(root) = self.metadata.root_package() {
            CrateName::new(&root.name)
        } else {
            self.metadata
                .workspace_default_packages()
                .first()
                .and_then(|p| CrateName::new(p.name.as_str()))
        }
    }
    /// Get crate info, optionally scoped to a specific workspace member
    pub fn crate_info<'a>(
        &'a self,
        member_name: Option<&str>,
    ) -> impl Iterator<Item = &'a CrateInfo> {
        let filter_member = member_name.or_else(|| self.detect_subcrate_context());
        let member_string = filter_member.map(|s| s.to_string());

        self.crate_info.iter().filter(move |info| {
            match &member_string {
                Some(member) => {
                    // Include: workspace members + deps used by this member + standard library
                    info.crate_type().is_workspace()
                        || info.used_by().contains(member)
                        || matches!(info.crate_type(), CrateProvenance::Rust)
                }
                None => true, // Include all for workspace view
            }
        })
    }

    /// Detect if we're in a subcrate context based on working directory
    pub fn detect_subcrate_context(&self) -> Option<&str> {
        let root_package = self.metadata.root_package()?;
        let workspace_packages = self.metadata.workspace_packages();

        // Check if we're in a subcrate context (working directory set to a specific workspace member)
        if workspace_packages.len() > 1
            && workspace_packages
                .iter()
                .any(|pkg| pkg.name == root_package.name)
        {
            Some(&root_package.name)
        } else {
            None
        }
    }

    pub fn normalize_crate_name<'a>(&'a self, crate_name: &'a str) -> Option<CrateName<'a>> {
        match crate_name {
            "crate" => {
                // In workspace contexts (>1 package), don't allow "crate" alias
                if self.metadata.workspace_packages().len() > 1 {
                    None
                } else {
                    self.default_crate_name()
                }
            }

            // rustdoc placeholders
            "alloc" | "alloc_crate" => Some(CrateName("alloc")),
            "core" | "core_crate" => Some(CrateName("core")),
            "proc_macro" | "proc_macro_crate" => Some(CrateName("proc_macro")),
            "test" | "test_crate" => Some(CrateName("test")),
            "std" | "std_crate" => Some(CrateName("std")),
            "std_detect" | "rustc_literal_escaper" => None,

            // future-proof: skip internal rustc crates
            name if name.starts_with("rustc_") => None,
            name => {
                // First try to find in available crates
                self.available_crates()
                    .find(|correct_name| eq_ignoring_dash_underscore(correct_name, name))
                    .or({
                        // If not found in available crates, still return the name so
                        // load_crate can attempt to fetch from docs.rs
                        Some(CrateName(name))
                    })
            }
        }
    }

    /// Load rustdoc data for a specific crate
    pub fn load_crate(&self, crate_name: CrateName<'_>) -> Option<RustdocData> {
        // Parse crate_name@version syntax
        let (crate_name, version) = if let Some(at_index) = crate_name.find('@') {
            let name = &crate_name[..at_index];
            let version = &crate_name[at_index + 1..];
            (CrateName(name), Some(version))
        } else {
            (crate_name, None)
        };

        // Try loading from local files first
        if let Some((json_path, crate_type)) = self.resolve_json_path(crate_name) {
            return match crate_type {
                CrateProvenance::Workspace => self.load_workspace(crate_name, json_path),
                CrateProvenance::Library => self.load_dep(crate_name, json_path),
                CrateProvenance::Rust => self.load_rustc(crate_name, json_path),
                CrateProvenance::DocsRs => {
                    unreachable!("resolve_json_path should never return DocsRs")
                }
            };
        }

        // Fallback: try fetching from docs.rs
        self.load_from_docsrs(crate_name, version)
    }

    pub(crate) fn load_dep(
        &self,
        crate_name: CrateName<'_>,
        json_path: PathBuf,
    ) -> Option<RustdocData> {
        let mut tried_rebuilding = false;
        let expected_version = self
            .metadata
            .packages
            .iter()
            .find(|x| **x.name == *crate_name)
            .map(|x| x.version.to_string());

        loop {
            if let Ok(content) = std::fs::read_to_string(&json_path)
                && let Ok(RustdocVersion {
                    format_version,
                    crate_version,
                }) = serde_json::from_str(&content)
                && format_version == FORMAT_VERSION
                && crate_version == expected_version
            {
                let crate_data: Crate = serde_json::from_str(&content).ok()?;

                break Some(RustdocData {
                    crate_data,
                    name: crate_name.to_string(),
                    crate_type: CrateProvenance::Library,
                    fs_path: json_path,
                });
            } else if !tried_rebuilding {
                tried_rebuilding = true;
                if self.rebuild_docs(crate_name).is_ok() {
                    continue;
                }
            }
            break None;
        }
    }

    fn load_rustc(&self, crate_name: CrateName<'_>, json_path: PathBuf) -> Option<RustdocData> {
        if let Ok(content) = std::fs::read_to_string(&json_path)
            && let Ok(RustdocVersion { format_version, .. }) = serde_json::from_str(&content)
            && format_version == FORMAT_VERSION
        {
            let crate_data: Crate = serde_json::from_str(&content).ok()?;

            Some(RustdocData {
                crate_data,
                name: crate_name.to_string(),
                crate_type: CrateProvenance::Library,
                fs_path: json_path,
            })
        } else {
            None
        }
    }

    /// Load from docs.rs
    ///
    /// This is called when the crate is not found locally. It uses the DocsRsClient
    /// to fetch the crate from docs.rs and cache it.
    fn load_from_docsrs(
        &self,
        crate_name: CrateName<'_>,
        version: Option<&str>,
    ) -> Option<RustdocData> {
        let client = self.docsrs_client.as_ref()?;

        log::info!(
            "Crate '{}' not found locally, fetching from docs.rs",
            crate_name
        );

        // DocsRsClient requires blocking on async
        let result = trillium_smol::async_global_executor::block_on(async {
            client.get_crate(crate_name.as_ref(), version).await
        });

        match result {
            Ok(Some(data)) => {
                log::info!("Successfully fetched '{}' from docs.rs", crate_name);
                Some(data)
            }
            Ok(None) => {
                log::info!("Crate '{}' not found on docs.rs", crate_name);
                None
            }
            Err(e) => {
                log::warn!("Failed to fetch '{}' from docs.rs: {}", crate_name, e);
                None
            }
        }
    }

    fn load_workspace(&self, crate_name: CrateName<'_>, json_path: PathBuf) -> Option<RustdocData> {
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
                && let Ok(content) = std::fs::read_to_string(&json_path)
                && let Ok(RustdocVersion { format_version, .. }) = serde_json::from_str(&content)
                && format_version == FORMAT_VERSION
            {
                let crate_data: Crate = serde_json::from_str(&content).ok()?;

                break Some(RustdocData {
                    crate_data,
                    name: crate_name.to_string(),
                    crate_type: CrateProvenance::Library,
                    fs_path: json_path,
                });
            } else if !tried_rebuilding {
                tried_rebuilding = true;
                if self.rebuild_docs(crate_name).is_ok() {
                    continue;
                }
            }
            break None;
        }
    }
}

#[derive(Debug, Clone, Fieldwork)]
#[fieldwork(get, rename_predicates)]
pub struct CrateInfo {
    crate_type: CrateProvenance,
    version: Option<String>,
    description: Option<String>,
    dev_dep: bool,
    name: String,
    default_crate: bool,
    used_by: Vec<String>,
}

#[derive(Deserialize, Debug)]
struct RustdocVersion {
    format_version: u32,
    crate_version: Option<String>,
}

#[derive(Debug, Clone)]
pub enum CrateProvenance {
    Workspace,
    Library,
    Rust,
    DocsRs,
}
impl CrateProvenance {
    pub fn is_workspace(&self) -> bool {
        matches!(self, Self::Workspace)
    }
}

/// Wrapper around rustdoc JSON data that provides convenient query methods
#[derive(Clone, Fieldwork)]
#[fieldwork(get, rename_predicates)]
pub struct RustdocData {
    pub(crate) crate_data: Crate,
    pub(crate) name: String,
    pub(crate) crate_type: CrateProvenance,
    pub(crate) fs_path: PathBuf,
}

impl Debug for RustdocData {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("RustdocData")
            .field("name", &self.name)
            .field("crate_type", &self.crate_type)
            .field("fs_path", &self.fs_path)
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

        let (name, version) = html_root_url
            .as_deref()
            .and_then(parse_docsrs_url)
            .map_or((&**name, None), |(name, version)| (name, Some(version)));

        navigator.load_crate(name, version)
    }

    pub(crate) fn get_path<'a>(
        &'a self,
        navigator: &'a Navigator,
        id: Id,
    ) -> Option<DocRef<'a, Item>> {
        let item_summary = self.paths.get(&id)?;
        let crate_ = self.traverse_to_crate_by_id(navigator, item_summary.crate_id)?;

        crate_
            .root_item(navigator)
            .find_by_path(item_summary.path.iter().skip(1))
    }
}
