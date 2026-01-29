//! Navigator - orchestrates documentation lookup across multiple sources

use crate::CrateName;
use crate::DocRef;
use crate::RustdocData;
use crate::sources::{CrateProvenance, DocsRsSource, LocalSource, Source, StdSource};
use crate::string_utils::case_aware_jaro_winkler;
use elsa::FrozenMap;
use fieldwork::Fieldwork;
use rustdoc_types::{Id, Item, ItemEnum};
use semver::Version;
use semver::VersionReq;
use std::borrow::Cow;
use std::fmt;
use std::fmt::Debug;
use std::path::PathBuf;

// /// Key for identifying crates in the working set
// /// Version is None for workspace/local crates, Some(semver) for published crates
// type CrateKey = (String, Option<String>);

#[derive(Fieldwork)]
#[fieldwork(get)]
pub struct Suggestion<'a> {
    path: String,
    item: Option<DocRef<'a, Item>>,
    score: f64,
}

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
    name: String,
    /// The version this crate was built against
    version: Version,
}

#[derive(Debug, Clone, Fieldwork)]
#[fieldwork(get, rename_predicates)]
pub struct CrateInfo {
    #[field(copy)]
    pub(crate) provenance: CrateProvenance,
    pub(crate) version: Option<Version>,
    pub(crate) description: Option<String>,
    pub(crate) name: String,
    pub(crate) default_crate: bool,
    pub(crate) used_by: Vec<String>,
    pub(crate) json_path: Option<PathBuf>,
}

/// Navigator orchestrates documentation lookup across multiple sources
///
/// Sources are checked in this order:
/// 1. std (if crate name matches RUST_CRATES)
/// 2. local (if LocalSource is present and has the crate)
/// 3. docs.rs (if DocsRsSource is present)
#[derive(Fieldwork, Default)]
#[fieldwork(get, opt_in, with)]
pub struct Navigator {
    #[field]
    std_source: Option<StdSource>,
    #[field]
    docsrs_source: Option<DocsRsSource>,
    #[field]
    local_source: Option<LocalSource>,

    /// Cached docs.
    ///
    /// This is the only place in all of ferritin-common that stores RustdocData, and
    /// all references to &'a RustdocData or DocRef<'a> are borrowing from this map.
    ///
    /// A None value indicates permanent failure.
    working_set: FrozenMap<CrateName<'static>, Box<Option<RustdocData>>>,

    // Map from internal name (underscores) to real name/version from external_crates
    external_crate_names: FrozenMap<CrateName<'static>, Box<ExternalCrateInfo>>,
}

impl Debug for Navigator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Navigator")
            .field("std_source", &self.std_source)
            .field("docsrs_source", &self.docsrs_source)
            .field("local_source", &self.local_source)
            .finish()
    }
}
impl Navigator {
    /// List all available crate names from all sources
    /// Returns crate names from std library and local workspace/dependencies
    pub fn list_available_crates(&self) -> impl Iterator<Item = &CrateInfo> {
        std::iter::empty()
            .chain(self.std_source.iter().flat_map(|x| x.list_available()))
            .chain(self.local_source.iter().flat_map(|x| x.list_available()))
    }

    /// Look up a crate by name, returning canonical name and metadata
    /// Tries sources in priority order: std, local, docs.rs
    pub fn lookup_crate<'a>(
        &'a self,
        name: &str,
        version: &VersionReq,
    ) -> Option<Cow<'a, CrateInfo>> {
        self.std_source()
            .and_then(|s| s.lookup(name, version))
            .or_else(|| self.local_source().and_then(|s| s.lookup(name, version)))
            .or_else(|| self.docsrs_source().and_then(|s| s.lookup(name, version)))
    }

    /// Get the project root path if a local context exists
    pub fn project_root(&self) -> Option<&std::path::Path> {
        self.local_source.as_ref().map(|p| p.project_root())
    }

    /// Resolve a path like "std::vec::Vec" or "tokio::runtime::Runtime"
    /// or (custom format for this crate) "tokio@1::runtime::Runtime" or "serde@1.0.228::de"
    ///
    /// This is the primary string entrypoint for any user-generated crate or type specification
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

        let (crate_name, version_req) = if let Some(index) = crate_name.find("@") {
            (
                &crate_name[..index],
                VersionReq::parse(&crate_name[index + 1..]).unwrap_or(VersionReq::STAR),
            )
        } else {
            (crate_name, VersionReq::STAR)
        };

        let Some(crate_data) = self.load_crate(crate_name, &version_req) else {
            suggestions.extend(self.list_available_crates().map(|crate_info| Suggestion {
                path: crate_info.name.clone(),
                item: None,
                score: case_aware_jaro_winkler(&crate_info.name, crate_name),
            }));
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

    pub fn canonicalize(&self, name: &str) -> CrateName<'static> {
        self.std_source()
            .and_then(|s| s.canonicalize(name))
            .or_else(|| self.local_source().and_then(|s| s.canonicalize(name)))
            .or_else(|| self.docsrs_source().and_then(|s| s.canonicalize(name)))
            .unwrap_or_else(|| CrateName::from(String::from(name)))
    }

    /// Load a crate by name and optional version
    ///
    /// If version is None:
    /// - First checks external crate names from loaded crates
    /// - For local context crates: use the locked version from Cargo.lock
    /// - For arbitrary crates: use "latest"
    ///
    /// Returns None if the crate cannot be found in any source
    pub fn load_crate(&self, name: &str, version_req: &VersionReq) -> Option<&RustdocData> {
        let crate_name = self.canonicalize(name);
        if let Some(data) = self.working_set.get(&crate_name) {
            return data.as_ref();
        }

        let (resolved_name, resolved_version, provenance_hint) =
            if let Some(external_crate) = self.external_crate_names.get(&crate_name) {
                (
                    external_crate.name.to_string(),
                    Some(external_crate.version.clone()),
                    None,
                )
            } else {
                let lookup_result = self.lookup_crate(name, version_req)?;
                (
                    lookup_result.name.to_string(),
                    lookup_result.version.clone(),
                    Some(lookup_result.provenance),
                )
            };

        // Try loading from the appropriate source based on provenance
        if let Some(rv) = resolved_version.as_ref() {
            log::debug!("Loading {resolved_name}@{rv}",);
        } else {
            log::debug!("Loading {resolved_name}");
        }
        let start = std::time::Instant::now();
        let result = self.load(&resolved_name, resolved_version.as_ref(), provenance_hint);
        let elapsed = start.elapsed();
        log::info!("⏱️ Total load time for {}: {:?}", resolved_name, elapsed);

        match result {
            Some(data) => {
                // Index external crates for future lookups
                self.index_external_crates(&data);

                // Cache in working set
                self.working_set
                    .insert(CrateName::from(resolved_name), Box::new(Some(data)))
                    .as_ref()
            }
            None => {
                // // Mark as failed
                self.working_set
                    .insert(CrateName::from(resolved_name), Box::new(None));
                None
            }
        }
    }

    /// Try loading from the appropriate source based on lookup result
    fn load(
        &self,
        crate_name: &str,
        version: Option<&Version>,
        provenance_hint: Option<CrateProvenance>,
    ) -> Option<RustdocData> {
        match provenance_hint {
            Some(CrateProvenance::Std) => self.std_source()?.load(crate_name, version),
            Some(CrateProvenance::Workspace | CrateProvenance::LocalDependency) => {
                self.local_source()?.load(crate_name, version)
            }
            Some(CrateProvenance::DocsRs) => self.docsrs_source()?.load(crate_name, version),
            None => self
                .std_source()
                .and_then(|s| s.load(crate_name, version))
                .or_else(|| {
                    self.local_source()
                        .and_then(|s| s.load(crate_name, version))
                })
                .or_else(|| {
                    self.docsrs_source()
                        .and_then(|s| s.load(crate_name, version))
                }),
        }
    }

    /// Index external crates from a loaded crate
    fn index_external_crates(&self, crate_data: &RustdocData) {
        log::debug!("Indexing external crates from {}", crate_data.name());
        for external in crate_data.external_crates.values() {
            if let Some(url) = &external.html_root_url
                && let Some((real_name, version)) = parse_docsrs_url(url)
                && let Ok(version) = Version::parse(version)
            {
                log::debug!(
                    "  {} (internal: {}) -> {}@{}",
                    real_name,
                    external.name,
                    real_name,
                    version
                );
                let info = ExternalCrateInfo {
                    name: real_name.to_string(),
                    version,
                };
                self.external_crate_names
                    .insert(CrateName::from(external.name.clone()), Box::new(info));
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
        let crate_docs = self.load_crate(crate_name, &VersionReq::STAR)?;
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
