use crate::styled_string::TruncationLevel;
use ferritin_common::DocRef;
use rustdoc_types::Item;
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

/// An erased "should this item be displayed?" predicate.
///
/// Higher-ranked over the borrow lifetime so storing one doesn't tie
/// `FormatContext` to the `Navigator` borrow, and `Send + Sync` so the context
/// can be shared across the TUI's scoped threads. The `DocRef` it receives
/// carries the `Navigator`, so even cross-crate predicates (e.g. "implements
/// some trait") are expressible without changing this signature.
pub(crate) type DisplayPredicate = Box<dyn for<'a> Fn(DocRef<'a, Item>) -> bool + Send + Sync>;

/// How much of the resolved item's own documentation prose to render before its
/// body (e.g. a module's listing). `None` is the pure-listing case; `Brief`
/// shows just the leading paragraph — usually the sweet spot for getting the
/// gist without the essay.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, clap::ValueEnum)]
pub(crate) enum DocLevel {
    #[default]
    Full,
    Brief,
    None,
}

impl DocLevel {
    /// The truncation level to render docs at, or `None` to omit them entirely.
    fn truncation(self) -> Option<TruncationLevel> {
        match self {
            DocLevel::Full => Some(TruncationLevel::Full),
            DocLevel::Brief => Some(TruncationLevel::Brief),
            DocLevel::None => Option::None,
        }
    }

    fn as_u8(self) -> u8 {
        match self {
            DocLevel::Full => 0,
            DocLevel::Brief => 1,
            DocLevel::None => 2,
        }
    }

    fn from_u8(value: u8) -> Self {
        match value {
            1 => DocLevel::Brief,
            2 => DocLevel::None,
            _ => DocLevel::Full,
        }
    }
}

/// Context for formatting operations
///
/// This contains configuration that determines what content to include in Documents.
/// Separate from RenderContext (which controls how to display Documents).
pub(crate) struct FormatContext {
    /// Whether to include source code snippets (toggled at runtime)
    include_source: AtomicBool,
    /// Whether to show recursive/nested content
    recursive: AtomicBool,
    /// Whether to hide non-public items (private fields, methods, module items)
    public: AtomicBool,
    /// Optional predicate narrowing which items appear in listings (e.g. the
    /// `--kind` filter). `None` means show everything. Behind a lock rather
    /// than an atomic because a closure can't be atomic, and we still need
    /// `Sync` for the TUI; `RwLock` (not `OnceLock`) so TUI re-navigation can
    /// replace it.
    filter: RwLock<Option<DisplayPredicate>>,
    /// How much of the resolved item's own doc prose to show (`--docs`).
    /// Stored as a `u8` so it stays an atomic like the other prefs.
    doc_level: AtomicU8,
}

impl std::fmt::Debug for FormatContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FormatContext")
            .field("include_source", &self.include_source())
            .field("recursive", &self.is_recursive())
            .field("public", &self.public())
            .field(
                "filter",
                &self.filter.read().map(|g| g.is_some()).unwrap_or(false),
            )
            .field("doc_level", &self.doc_level())
            .finish()
    }
}

impl FormatContext {
    pub(crate) fn new() -> Self {
        Self {
            include_source: AtomicBool::new(false),
            recursive: AtomicBool::new(false),
            public: AtomicBool::new(false),
            filter: RwLock::new(None),
            doc_level: AtomicU8::new(DocLevel::default().as_u8()),
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

    /// Check if non-public items should be hidden
    pub(crate) fn public(&self) -> bool {
        self.public.load(Ordering::Relaxed)
    }

    /// Set hiding of non-public items (thread-safe)
    pub(crate) fn set_public(&self, value: bool) -> &Self {
        self.public.store(value, Ordering::Relaxed);
        self // For chaining
    }

    /// Builder method for hiding non-public items
    pub(crate) fn with_public(self, value: bool) -> Self {
        self.set_public(value);
        self
    }

    /// Replace the display filter (thread-safe). `None` shows everything.
    pub(crate) fn set_filter(&self, predicate: Option<DisplayPredicate>) -> &Self {
        *self.filter.write().unwrap() = predicate;
        self
    }

    /// How much of the resolved item's own doc prose to show.
    pub(crate) fn doc_level(&self) -> DocLevel {
        DocLevel::from_u8(self.doc_level.load(Ordering::Relaxed))
    }

    /// Set how much of the resolved item's own doc prose to show (thread-safe).
    pub(crate) fn set_doc_level(&self, level: DocLevel) -> &Self {
        self.doc_level.store(level.as_u8(), Ordering::Relaxed);
        self
    }

    /// The truncation level to render the item's own docs at, or `None` to omit
    /// them. Wraps [`DocLevel::truncation`].
    pub(crate) fn doc_truncation(&self) -> Option<TruncationLevel> {
        self.doc_level().truncation()
    }

    /// Whether `item` passes the current display filter. `true` when no filter
    /// is set.
    pub(crate) fn should_display(&self, item: DocRef<'_, Item>) -> bool {
        match self.filter.read().unwrap().as_ref() {
            Some(predicate) => predicate(item),
            None => true,
        }
    }
}
