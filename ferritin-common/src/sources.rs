//! Documentation sources
//!
//! This module defines different sources for rustdoc JSON data:
//! - StdSource: rustup-managed std library docs
//! - LocalSource: workspace-local crates (built on demand)
//! - DocsRsSource: fetched from docs.rs and cached
use crate::{CrateName, RustdocData, store::CrateInfo};
use anyhow::Result;
use semver::{Version, VersionReq};
use serde::{Deserialize, Deserializer, Serialize};

mod docsrs;
mod local;
mod std;
mod workspace_metadata;

use ::std::borrow::Cow;
pub use docsrs::DocsRsSource;
pub use local::LocalSource;
pub use std::StdSource;

/// A cargo feature selection for a local documentation build.
///
/// Mirrors cargo's `--features`/`--all-features`/`--no-default-features` trio.
/// Only meaningful for [`LocalSource`]: docs.rs builds are not under our control.
///
/// This doubles as build provenance — the selection a cached rustdoc JSON was
/// built with is persisted in `<target-dir>/ferritin.json` so the cache can be
/// invalidated when the requested features change. See [`workspace_metadata`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSelection {
    /// Disable the `default` feature (`--no-default-features`).
    #[serde(default)]
    pub no_default: bool,
    /// Enable all features (`--all-features`).
    #[serde(default)]
    pub all: bool,
    /// Explicitly enabled features (`--features a,b`).
    #[serde(default)]
    pub list: Vec<String>,
}

impl FeatureSelection {
    /// The cargo arguments this selection expands to, in cargo's own order.
    fn cargo_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        if self.no_default {
            args.push("--no-default-features".to_string());
        }
        if self.all {
            args.push("--all-features".to_string());
        }
        if !self.list.is_empty() {
            args.push("--features".to_string());
            args.push(self.list.join(","));
        }
        args
    }
}

#[derive(Deserialize, Debug)]
struct RustdocVersion {
    format_version: u32,
    #[serde(deserialize_with = "option_semver_lenient")]
    crate_version: Option<Version>,
}

fn option_semver_lenient<'de, D>(deserializer: D) -> Result<Option<Version>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt = Option::<Cow<'de, str>>::deserialize(deserializer)?;
    Ok(opt.and_then(|s| Version::parse(&s).ok()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrateProvenance {
    Workspace,
    LocalDependency,
    Std,
    DocsRs,
}
impl CrateProvenance {
    pub fn is_workspace(&self) -> bool {
        matches!(self, Self::Workspace)
    }

    pub fn is_local_dependency(&self) -> bool {
        matches!(self, Self::LocalDependency)
    }

    pub fn is_std(&self) -> bool {
        matches!(self, Self::Std)
    }

    pub fn is_docs_rs(&self) -> bool {
        matches!(self, Self::DocsRs)
    }
}

/// Trait for documentation sources
///
/// Each source (std, local workspace, docs.rs) implements this trait to provide:
/// - Name lookup/normalization
/// - Crate loading
/// - Available crate listing (where applicable)
pub trait Source {
    /// Transform a crate name into an internal representation
    ///
    /// This should be cheap (local) and based on already-available information.
    /// Returning None indicates that this Source does not have any information with which to transform the provided name.
    fn canonicalize(&self, input_name: &str) -> Option<CrateName<'static>> {
        let _ = input_name;
        None
    }

    /// Look up a crate by name, returning canonical name and metadata if found.
    ///
    /// `Ok(None)` means this source definitively does not have the crate;
    /// `Err` means a transient failure (e.g. a network error reaching
    /// crates.io) that must not be cached as long-lived absence.
    fn lookup<'a>(
        &'a self,
        crate_name: &str,
        version: &VersionReq,
    ) -> Result<Option<Cow<'a, CrateInfo>>>;

    /// Load the rustdoc JSON data for a crate (by canonical name) at an exact
    /// version, as previously resolved by [`Source::lookup`].
    ///
    /// The same `Ok(None)`-versus-`Err` distinction as [`Source::lookup`]
    /// applies: `Ok(None)` is definitive absence (e.g. docs.rs has no rustdoc
    /// JSON for this release), `Err` is transient.
    fn load(&self, crate_name: &str, version: &Version) -> Result<Option<RustdocData>>;

    /// List all available crates from this source
    /// Returns None if this source doesn't support listing (e.g., DocsRsSource)
    fn list_available<'a>(&'a self) -> Box<dyn Iterator<Item = &'a CrateInfo> + '_> {
        Box::new(::std::iter::empty())
    }
}
