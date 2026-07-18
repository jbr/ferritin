// Core library for rustdoc navigation and search
// Re-export rustdoc_types for convenience
pub use rustdoc_types;

mod archive;
pub mod conversions;
pub mod crate_name;
pub mod crate_names;
pub mod doc_ref;
pub mod ferritin_home;
mod indexes;
pub mod iterators;
mod navigator;
mod resolver;
mod rustdoc_data;
pub mod search;
pub mod sources;
mod store;
pub mod string_utils;

/// The `User-Agent` every ferritin http client identifies itself with.
pub const FERRITIN_USER_AGENT: &str = concat!("ferritin/", env!("CARGO_PKG_VERSION"));

/// Drive a future to completion on the current thread, using the same executor
/// this crate's async APIs — [`CrateIndex`] queries, the docs.rs source — are
/// built on.
///
/// Provided so a synchronous caller (the CLI's main thread, the serve worker
/// pool, the TUI request thread) can consume those async APIs without depending
/// on `trillium-smol` directly. It exists precisely so the block stays at the
/// caller's boundary rather than hiding inside the resolution path — never call
/// it from an async executor thread, where it would block that thread.
pub fn block_on<F: std::future::Future>(future: F) -> F::Output {
    trillium_smol::async_io::block_on(future)
}

// Re-export commonly used types
pub use crate_name::CrateName;
pub use crate_names::{CrateEntry, CrateIndex};
pub use doc_ref::DocRef;
pub use navigator::{MissingCrate, Navigator};
pub use resolver::{CratePath, Resolver, Suggestion};
pub use rustdoc_data::RustdocData;
pub use sources::CrateProvenance;
pub use store::{CrateInfo, Store};

#[cfg(test)]
mod tests;
