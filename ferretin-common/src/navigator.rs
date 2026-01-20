//! Navigator - orchestrates documentation lookup across multiple sources

use crate::crate_name::CrateName;
use crate::doc_ref::DocRef;
use crate::project::{CrateKey, LocalContext, RUST_CRATES, RustdocData};
use crate::sources::{DocsRsSource, LocalSource, StdSource};
use crate::string_utils::case_aware_jaro_winkler;
use anyhow::Result;
use elsa::FrozenMap;
use fieldwork::Fieldwork;
use rustdoc_types::{Id, Item, ItemEnum};
use std::cell::OnceCell;
use std::fmt;
use std::path::PathBuf;

/// Parse a docs.rs URL to extract crate name and version
///
/// Examples:
/// - "https://docs.rs/tokio-macros/2.6.0/x86_64-unknown-linux-gnu/" -> ("tokio-macros", "2.6.0")
/// - "https://docs.rs/serde/1.0.228" -> ("serde", "1.0.228")
pub(crate) fn parse_docsrs_url(url: &str) -> Option<(&str, &str)> {
    let url = url
        .strip_prefix("https://docs.rs/")
        .or_else(|| url.strip_prefix("http://docs.rs/"))?;

    // Split by '/' to get parts
    let parts: Vec<&str> = url.split('/').collect();
    if parts.len() >= 2 {
        Some((parts[0], parts[1]))
    } else {
        None
    }
}

/// External crate info extracted from html_root_url
#[derive(Debug, Clone)]
struct ExternalCrateInfo {
    /// The real crate name (with dashes, as it appears on crates.io)
    real_name: String,
    /// The version this crate was built against
    version: String,
}

/// Navigator orchestrates documentation lookup across multiple sources
///
/// Sources are checked in this order:
/// 1. std (if crate name matches RUST_CRATES)
/// 2. local (if LocalSource is present and has the crate)
/// 3. docs.rs (if DocsRsSource is present)
pub struct Navigator {
    // Optional documentation sources
    std_source: Option<StdSource>,
    docsrs_source: Option<DocsRsSource>,

    // Lazy local source - only loaded when needed
    local_context_path: Option<PathBuf>,
    can_rebuild: bool,
    local_source: OnceCell<Option<LocalSource>>,

    // Default crate (for "crate" alias in single-package workspaces)
    default_crate: OnceCell<Option<String>>,

    // Working set cache: CrateKey -> RustdocData
    // CrateKey is (name, version) where version is None for local/workspace crates
    working_set: FrozenMap<CrateKey, Box<RustdocData>>,

    // tracks versions in the working set
    versions: FrozenMap<String, Box<Option<String>>>,

    // Map from internal name (underscores) to real name/version from external_crates
    external_crate_names: FrozenMap<String, Box<ExternalCrateInfo>>,

    // Track failed load attempts to avoid retries
    failed_loads: FrozenMap<CrateKey, Box<()>>,
}

impl fmt::Debug for Navigator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Navigator")
            .field("has_std_source", &self.std_source.is_some())
            .field("has_local_context_path", &self.local_context_path.is_some())
            .field("local_source_loaded", &self.local_source.get().is_some())
            .field("has_docsrs_source", &self.docsrs_source.is_some())
            .field("working_set_size", &self.working_set.len())
            .finish()
    }
}

impl Navigator {
    /// Create a new Navigator builder
    pub fn builder() -> NavigatorBuilder {
        NavigatorBuilder {
            std_source: None,
            local_context_path: None,
            can_rebuild: false,
            docsrs_source: None,
        }
    }

    /// Create a Navigator with all default sources
    pub fn with_defaults() -> Result<Self> {
        Self::builder()
            .with_std_source_if_available()
            .with_docsrs_source_if_available()
            .build()
    }

    /// Get or lazily create the local source
    /// Returns None if no local context path was provided or if loading failed
    fn local_source(&self) -> Option<&LocalSource> {
        let path = self.local_context_path.as_ref()?;

        self.local_source
            .get_or_init(|| match LocalContext::load(path.clone()) {
                Ok(context) => {
                    log::debug!("Loaded local context from {}", path.display());
                    Some(LocalSource::new(context, self.can_rebuild))
                }
                Err(e) => {
                    log::warn!(
                        "Failed to load local context from {}: {}",
                        path.display(),
                        e
                    );
                    None
                }
            })
            .as_ref()
    }

    /// Get the default crate (for "crate" alias in single-package workspaces)
    /// Returns None if no local source or if multi-package workspace
    fn default_crate(&self) -> Option<&str> {
        self.default_crate
            .get_or_init(|| {
                let local = self.local_source()?;
                let packages = local.context().workspace_packages();

                // Only allow "crate" alias in single-package workspaces
                if packages.len() == 1 {
                    log::debug!("Setting default crate to: {}", packages[0]);
                    Some(packages[0].clone())
                } else {
                    log::debug!(
                        "No default crate (multi-package workspace with {} packages)",
                        packages.len()
                    );
                    None
                }
            })
            .as_deref()
    }

    /// Resolve a path like "std::vec::Vec" or "tokio::runtime::Runtime"
    pub fn resolve_path<'a>(
        &'a self,
        path: &str,
        suggestions: &mut Vec<Suggestion<'a>>,
    ) -> Option<DocRef<'a, Item>> {
        let (crate_name, index) = if let Some(index) = path.find("::") {
            (&path[..index], Some(index + 2))
        } else {
            (path, None)
        };

        let Some(crate_data) = self.load_crate(crate_name, None) else {
            // Generate suggestions from available crates
            if let Some(local) = self.local_source() {
                suggestions.extend(local.list_available().map(|name| {
                    let version = self.versions.get(&name);
                    let item = version
                        .and_then(|v| self.working_set.get(&(name.clone(), v.clone())))
                        .map(|x| x.root_item(self));

                    Suggestion {
                        path: name.clone(),
                        item,
                        score: case_aware_jaro_winkler(&name, crate_name),
                    }
                }));
            }
            if let Some(std) = &self.std_source {
                suggestions.extend(std.list_available().map(|name| Suggestion {
                    path: name.to_string(),
                    item: None,
                    score: case_aware_jaro_winkler(&name.to_string(), crate_name),
                }));
            }
            return None;
        };

        // Start from crate root
        let item = crate_data.get(self, &crate_data.root)?;
        if let Some(index) = index {
            self.find_children_recursive(item, path, index, suggestions)
        } else {
            Some(item)
        }
    }

    /// Load a crate by name and optional version
    ///
    /// If version is None:
    /// - First checks external crate names from loaded crates
    /// - For local context crates: use the locked version from Cargo.lock
    /// - For arbitrary crates: use "latest"
    ///
    /// Returns None if the crate cannot be found in any source
    pub fn load_crate(&self, name: &str, version: Option<&str>) -> Option<&RustdocData> {
        // Handle "crate" alias (only in single-package workspaces)
        let name = if name == "crate" {
            self.default_crate()?
        } else {
            name
        };

        // Determine the version BEFORE normalizing (external_crate_names is keyed by internal names)
        let resolved_version = version.or_else(|| self.resolve_version(name));

        // Normalize the name (handle underscores vs dashes, filter invalid names)
        let Some(name_str) = self.normalize_name(name) else {
            // normalize_name returned None - this is a filtered/invalid crate name
            return None;
        };

        // Create cache key
        let key = (name_str.clone(), resolved_version.map(String::from));

        // Check if already in working set
        if let Some(data) = self.working_set.get(&key) {
            return Some(data);
        }

        // Check if we already tried and failed
        if self.failed_loads.get(&key).is_some() {
            return None;
        }

        // Try loading from sources in order
        log::debug!("Loading '{}' with version {:?}", name_str, resolved_version);
        let result = self.try_load_from_sources(&name_str, resolved_version);

        match result {
            Some(data) => {
                // Index external crates for future lookups
                self.index_external_crates(&data);

                self.versions
                    .insert(name_str, Box::new(resolved_version.map(String::from)));
                // Cache in working set
                Some(self.working_set.insert(key.clone(), Box::new(data)))
            }
            None => {
                // Mark as failed
                self.failed_loads.insert(key, Box::new(()));
                None
            }
        }
    }

    /// Try loading from sources in priority order
    fn try_load_from_sources(&self, name: &str, version: Option<&str>) -> Option<RustdocData> {
        // 1. Try std source (if name matches)
        if let Some(crate_name) = CrateName::new(name)
            && RUST_CRATES.contains(&crate_name)
            && let Some(std) = &self.std_source
            && let Some(data) = std.load(crate_name)
        {
            return Some(data);
        }

        // 2. Try local source
        if let Some(local) = self.local_source()
            && local.can_load(name)
            && let Some(crate_name) = CrateName::new(name)
        {
            // Check if it's a workspace package
            if local.context().is_workspace_package(name) {
                if let Some(data) = local.load_workspace(crate_name) {
                    return Some(data);
                }
            } else {
                // It's a dependency
                if let Some(data) = local.load_dep(crate_name) {
                    return Some(data);
                }
            }
        }

        // 3. Try docs.rs source
        // Only fetch from docs.rs if:
        // - This is the first crate we're loading (external_crate_names is empty), OR
        // - This crate is referenced in a loaded crate's external_crates
        // (prevents fetching modules like "unix", "windows" as if they were crates)
        if let Some(docsrs) = &self.docsrs_source {
            let is_initial_load = self.external_crate_names.is_empty();
            let is_referenced = self.external_crate_names.get(name).is_some()
                || self
                    .external_crate_names
                    .get(&name.replace('-', "_"))
                    .is_some()
                || self
                    .external_crate_names
                    .get(&name.replace('_', "-"))
                    .is_some();

            if !is_initial_load && !is_referenced {
                log::debug!(
                    "Skipping docs.rs lookup for '{}' - not in any loaded crate's external_crates",
                    name
                );
                return None;
            }

            match docsrs.load_blocking(name, version) {
                Ok(Some(data)) => {
                    log::info!("Successfully loaded '{}' from docs.rs", name);
                    return Some(data);
                }
                Ok(None) => {
                    log::debug!("Crate '{}' not found on docs.rs", name);
                }
                Err(e) => {
                    log::warn!("Failed to load '{}' from docs.rs: {}", name, e);
                }
            }
        }

        None
    }

    /// Resolve version for a crate name
    /// Checks external_crate_names first, then local context
    /// Returns None if not found (will use "latest" for docs.rs)
    fn resolve_version(&self, name: &str) -> Option<&str> {
        // First check external_crate_names map (from loaded crates' dependencies)
        // Try both the name as-is and with dash/underscore swapped
        if let Some(info) = self.external_crate_names.get(name) {
            log::debug!(
                "Using version {} from external_crate_names for {}",
                info.version,
                name
            );
            return Some(&info.version);
        }

        // Try with dash/underscore swapped
        let alt_name = if name.contains('-') {
            name.replace('-', "_")
        } else {
            name.replace('_', "-")
        };
        if let Some(info) = self.external_crate_names.get(&alt_name) {
            log::debug!(
                "Using version {} from external_crate_names for {} (via {})",
                info.version,
                name,
                alt_name
            );
            return Some(&info.version);
        }

        // Then check local context (for workspace dependencies)
        if let Some(local) = self.local_source()
            && let Some(version) = local.context().get_dependency_version(name)
        {
            log::debug!("Using version {} from local context for {}", version, name);
            return Some(version);
        }

        None
    }

    /// Normalize a crate name (handle dash/underscore variants)
    fn normalize_name(&self, name: &str) -> Option<String> {
        // Filter out rustc-internal pseudo-crates
        if matches!(name, "std_detect" | "rustc_literal_escaper") || name.starts_with("rustc_") {
            log::debug!("Rejecting rustc-internal crate: {}", name);
            return None;
        }

        // Check if local source has this name (with dash/underscore normalization)
        if let Some(local) = self.local_source()
            && local.can_load(name)
        {
            return Some(name.to_string());
        }

        // Check external crate names map
        if let Some(info) = self.external_crate_names.get(name) {
            return Some(info.real_name.clone());
        }

        // Return as-is
        Some(name.to_string())
    }

    /// Index external crates from a loaded crate
    fn index_external_crates(&self, crate_data: &RustdocData) {
        log::debug!("Indexing external crates from {}", crate_data.name());
        for external in crate_data.external_crates.values() {
            if let Some(url) = &external.html_root_url
                && let Some((real_name, version)) = parse_docsrs_url(url)
            {
                log::debug!(
                    "  {} (internal: {}) -> {}@{}",
                    real_name,
                    external.name,
                    real_name,
                    version
                );
                let info = ExternalCrateInfo {
                    real_name: real_name.to_string(),
                    version: version.to_string(),
                };
                self.external_crate_names
                    .insert(external.name.clone(), Box::new(info));
            }
        }
    }

    /// Get item from ID path
    pub fn get_item_from_id_path<'a>(
        &'a self,
        crate_name: &str,
        ids: &[u32],
    ) -> Option<(DocRef<'a, Item>, Vec<&'a str>)> {
        let mut path = vec![];
        let crate_docs = self.load_crate(crate_name, None)?;
        let mut item = crate_docs.get(self, &crate_docs.root)?;
        path.push(item.crate_docs().name());
        for id in ids {
            item = item.get(&Id(*id))?;
            if let ItemEnum::Use(use_item) = item.inner() {
                item = use_item
                    .id
                    .and_then(|id| item.get(&id))
                    .or_else(|| item.navigator().resolve_path(&use_item.source, &mut vec![]))?;
                if !use_item.is_glob {
                    item.set_name(&use_item.name);
                }
            } else if let Some(name) = item.name() {
                path.push(name);
            }
        }

        Some((item, path))
    }

    fn find_children_recursive<'a>(
        &'a self,
        item: DocRef<'a, Item>,
        path: &str,
        index: usize,
        suggestions: &mut Vec<Suggestion<'a>>,
    ) -> Option<DocRef<'a, Item>> {
        let remaining = &path[path.len().min(index)..];
        if remaining.is_empty() {
            return Some(item);
        }
        let segment_end = remaining
            .find("::")
            .map(|x| index + x)
            .unwrap_or(path.len());
        let segment = &path[index..segment_end];
        let next_segment_start = path.len().min(segment_end + 2);

        log::trace!(
            "🔎 searching for {segment} in {} ({:?}) (remaining {})",
            &path[..index],
            item.kind(),
            &path[next_segment_start..]
        );

        for child in item.child_items() {
            if let Some(name) = child.name()
                && name == segment
                && let Some(child) =
                    self.find_children_recursive(child, path, next_segment_start, suggestions)
            {
                return Some(child);
            }
        }

        suggestions.extend(self.generate_suggestions(item, path, index));
        None
    }

    fn generate_suggestions<'a>(
        &'a self,
        item: DocRef<'a, Item>,
        path: &str,
        index: usize,
    ) -> impl Iterator<Item = Suggestion<'a>> {
        item.child_items().filter_map(move |item| {
            item.name().and_then(|name| {
                let full_path = format!("{}{name}", &path[..index]);
                if path.starts_with(&full_path) {
                    None
                } else {
                    let score = case_aware_jaro_winkler(path, &full_path);
                    Some(Suggestion {
                        path: full_path,
                        score,
                        item: Some(item),
                    })
                }
            })
        })
    }
}

/// Builder for Navigator
pub struct NavigatorBuilder {
    std_source: Option<StdSource>,
    local_context_path: Option<PathBuf>,
    can_rebuild: bool,
    docsrs_source: Option<DocsRsSource>,
}

impl NavigatorBuilder {
    /// Add std source from rustup
    pub fn with_std_source(mut self, source: StdSource) -> Self {
        self.std_source = Some(source);
        self
    }

    /// Try to add std source from rustup (doesn't fail if not available)
    pub fn with_std_source_if_available(mut self) -> Self {
        self.std_source = StdSource::from_rustup();
        if self.std_source.is_some() {
            log::debug!("Initialized std source from rustup");
        } else {
            log::debug!("std source not available");
        }
        self
    }

    /// Add local context path (will be loaded lazily when needed)
    pub fn with_local_context(mut self, path: PathBuf, can_rebuild: bool) -> Self {
        self.local_context_path = Some(path);
        self.can_rebuild = can_rebuild;
        self
    }

    /// Add docs.rs source
    pub fn with_docsrs_source(mut self, source: DocsRsSource) -> Self {
        self.docsrs_source = Some(source);
        self
    }

    /// Try to add docs.rs source from default cache location
    pub fn with_docsrs_source_if_available(mut self) -> Self {
        self.docsrs_source = DocsRsSource::from_default_cache();
        if self.docsrs_source.is_some() {
            log::debug!("Initialized docs.rs source");
        } else {
            log::debug!("docs.rs source not available");
        }
        self
    }

    /// Build the Navigator
    pub fn build(self) -> Result<Navigator> {
        Ok(Navigator {
            std_source: self.std_source,
            local_context_path: self.local_context_path,
            can_rebuild: self.can_rebuild,
            local_source: OnceCell::new(),
            default_crate: OnceCell::new(),
            docsrs_source: self.docsrs_source,
            working_set: FrozenMap::new(),
            external_crate_names: FrozenMap::new(),
            failed_loads: FrozenMap::new(),
            versions: FrozenMap::new(),
        })
    }
}

#[derive(Fieldwork)]
#[fieldwork(get)]
pub struct Suggestion<'a> {
    path: String,
    item: Option<DocRef<'a, Item>>,
    score: f64,
}
