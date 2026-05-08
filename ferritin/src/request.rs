use ferritin_common::{Navigator, Resolver};
use std::ops::{Deref, DerefMut};

use crate::format_context::FormatContext;

/// Per-operation wrapper around a [`Resolver`] plus formatting state.
///
/// One `Request` corresponds to one user-initiated operation (a CLI command
/// invocation, or one TUI command roundtrip). The owned `Resolver` lives for
/// the same scope; its frame stack drains to empty between sub-lookups, so
/// reusing a single `Resolver` across all the formatter's `find_child` /
/// `resolve_path` calls is correct and gives `current_path()` something to
/// report when wired up.
///
/// Lifetime `'a` is the `Navigator`'s lifetime — both the resolver and any
/// `DocRef` it returns borrow from there.
pub(crate) struct Request<'a> {
    resolver: Resolver<'a>,
    format_context: FormatContext,
}

impl<'a> Deref for Request<'a> {
    type Target = Resolver<'a>;

    fn deref(&self) -> &Self::Target {
        &self.resolver
    }
}

impl<'a> DerefMut for Request<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.resolver
    }
}

impl<'a> Request<'a> {
    pub(crate) fn new(navigator: &'a Navigator, format_context: FormatContext) -> Self {
        Self {
            resolver: Resolver::new(navigator),
            format_context,
        }
    }

    pub(crate) fn format_context(&self) -> &FormatContext {
        &self.format_context
    }

    pub(crate) fn resolver_mut(&mut self) -> &mut Resolver<'a> {
        &mut self.resolver
    }
}
