use anyhow::{Result, anyhow};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    thread,
};

/// Documentation lookups recurse deeply enough to overflow a default 2 MiB
/// thread stack; this matches the big-stack pool `ferritin serve` uses.
const STACK_SIZE: usize = 16 * 1024 * 1024;

/// The rustdoc tools' shared state, cloned into every call.
///
/// There is no working directory to set. MCP has no sessions, so nothing a
/// call does may change how the next one resolves: each call names its
/// project, or falls back to the root this server was started with — `--root`,
/// else the process's working directory.
#[derive(Debug, Clone)]
pub(crate) struct RustdocTools {
    root: Arc<Path>,
}

impl RustdocTools {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self { root: root.into() }
    }

    /// The project directory a call asked for, resolved against the root, or
    /// the root itself.
    pub(crate) fn project_dir(&self, project: Option<&str>) -> Result<PathBuf> {
        let project = project.map(|project| PathBuf::from(&*shellexpand::tilde(project)));
        let path = match project {
            Some(project) => self.root.join(project),
            None => self.root.to_path_buf(),
        };
        fs::canonicalize(&path).map_err(|e| anyhow!("{}: {e}", path.display()))
    }

    /// Run a tool body on a thread of its own. The lookups are synchronous and
    /// block on docs.rs fetches internally, so they must stay off the executor
    /// the other calls share, and they need a big stack.
    pub(crate) async fn unblock<T: Send + 'static>(
        self,
        body: impl FnOnce(&Self) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let (sender, receiver) = smol::channel::bounded(1);
        thread::Builder::new()
            .name("rustdoc-mcp call".into())
            .stack_size(STACK_SIZE)
            .spawn(move || {
                let _ = sender.send_blocking(body(&self));
            })?;
        receiver
            .recv()
            .await
            .map_err(|_| anyhow!("the tool panicked"))?
    }
}
