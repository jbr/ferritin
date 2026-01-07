use rustdoc_core::project::RustdocProject;
use anyhow::{Result, anyhow};
use fieldwork::Fieldwork;
use mcplease::session::SessionStore;
use serde::{Deserialize, Serialize};
use std::rc::Rc;
use std::{fmt::Debug, fs, path::PathBuf};

/// Shared context data that can be used across multiple MCP servers
#[derive(Debug, Clone, Serialize, Deserialize, Default, Eq, PartialEq)]
pub(crate) struct SharedContextData {
    /// Current working context path
    context_path: Option<PathBuf>,
}

/// Rustdoc tools with session support and multi-crate capabilities
#[derive(Fieldwork, Debug)]
#[fieldwork(get)]
pub(crate) struct RustdocTools {
    /// Shared context store for cross-server communication (working directory)
    shared_context_store: SessionStore<SharedContextData>,

    #[field(set, with)]
    default_session_id: &'static str,

    /// Cached rustdoc project (lazily loaded)
    cached_project: Option<Rc<RustdocProject>>,
}

impl RustdocTools {
    /// Create a new RustdocTools instance
    pub(crate) fn new(storage_path: Option<PathBuf>) -> Result<Self> {
        let shared_context_store = SessionStore::new(storage_path)?;

        Ok(Self {
            shared_context_store,
            default_session_id: "default",
            cached_project: None,
        })
    }

    /// Get context (working directory) for a session
    pub(crate) fn get_context(&mut self, session_id: Option<&str>) -> Result<Option<PathBuf>> {
        let session_id = session_id.unwrap_or_else(|| self.default_session_id());
        let shared_data = self.shared_context_store.get_or_create(session_id)?;
        Ok(shared_data.context_path.clone())
    }

    /// Set working directory for a session (shared across MCP servers)
    pub(crate) fn set_working_directory(
        &mut self,
        path: PathBuf,
        session_id: Option<&str>,
    ) -> Result<()> {
        let session_id = session_id.unwrap_or_else(|| self.default_session_id());

        // Clear cached project when working directory changes
        self.cached_project = None;

        self.shared_context_store.update(session_id, |data| {
            data.context_path = Some(path);
        })
    }

    /// Get or load the rustdoc project for the current working directory
    pub(crate) fn project_context(
        &mut self,
        session_id: Option<&str>,
    ) -> Result<Rc<RustdocProject>> {
        let manifest_path = self.resolve_path("Cargo.toml", session_id)?;
        log::trace!("using manifest path {}", manifest_path.display());

        // If we already have a cached project, return it
        if let Some(project) = self.cached_project.clone()
            && project.manifest_path() == &*manifest_path
        {
            return Ok(project);
        };

        // Load the project
        let project = Rc::new(RustdocProject::load(manifest_path)?);

        self.cached_project = Some(Rc::clone(&project));

        Ok(project)
    }

    pub(crate) fn resolve_path(
        &mut self,
        path_str: &str,
        session_id: Option<&str>,
    ) -> Result<PathBuf> {
        let path = PathBuf::from(&*shellexpand::tilde(path_str));

        if path.is_absolute() {
            return fs::canonicalize(path).map_err(Into::into);
        }

        let session_id = session_id.unwrap_or_else(|| self.default_session_id());
        match self.get_context(Some(session_id))? {
            Some(context) => fs::canonicalize(context.join(path_str)).map_err(Into::into),
            None => Err(anyhow!(
                "Use set_working_directory first or provide an absolute path.",
            )),
        }
    }
}
