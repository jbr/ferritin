use ferritin_common::sources::{DocsRsSource, LocalSource, StdSource};
use ferritin_common::{Navigator, Resolver};
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;

/// MCP-specific wrapper around a [`Resolver`] that adds formatting capabilities.
///
/// Lifetime `'a` is the [`Navigator`]'s lifetime — both the resolver and any
/// [`ferritin_common::DocRef`] it returns borrow from there.
pub(crate) struct Request<'a> {
    resolver: Resolver<'a>,
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
    pub(crate) fn new(navigator: &'a Navigator) -> Self {
        Self {
            resolver: Resolver::new(navigator),
        }
    }
}

/// Build a [`Navigator`] configured with all sources for the given manifest path.
pub(crate) fn build_navigator(manifest_path: PathBuf) -> Navigator {
    Navigator::default()
        .with_std_source(StdSource::from_rustup())
        .with_local_source(LocalSource::load(&manifest_path).ok())
        .with_docsrs_source(DocsRsSource::from_default_cache())
}
