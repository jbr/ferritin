use std::sync::atomic::{AtomicBool, Ordering};

/// Context for formatting operations
///
/// This contains configuration that determines what content to include in Documents.
/// Separate from RenderContext (which controls how to display Documents).
#[derive(Debug)]
pub(crate) struct FormatContext {
    /// Whether to include source code snippets (toggled at runtime)
    include_source: AtomicBool,
    /// Whether to show recursive/nested content
    recursive: AtomicBool,
}

impl FormatContext {
    pub(crate) fn new() -> Self {
        Self {
            include_source: AtomicBool::new(false),
            recursive: AtomicBool::new(false),
        }
    }

    /// Check if source code should be included
    pub(crate) fn include_source(&self) -> bool {
        self.include_source.load(Ordering::Relaxed)
    }

    /// Set source code inclusion (thread-safe)
    pub(crate) fn set_include_source(&self, value: bool) -> &Self {
        self.include_source.store(value, Ordering::Relaxed);
        self // For chaining
    }

    /// Check if recursive display is enabled
    pub(crate) fn is_recursive(&self) -> bool {
        self.recursive.load(Ordering::Relaxed)
    }

    /// Set recursive display (thread-safe)
    pub(crate) fn set_recursive(&self, value: bool) -> &Self {
        self.recursive.store(value, Ordering::Relaxed);
        self // For chaining
    }

    /// Builder method for recursive
    pub(crate) fn with_recursion(self, value: bool) -> Self {
        self.set_recursive(value);
        self
    }
}
