// Core library for rustdoc navigation and search
// Re-export rustdoc_types for convenience
pub use rustdoc_types;

mod archive;
pub mod conversions;
pub mod crate_name;
pub mod doc_ref;
mod indexes;
pub mod iterators;
mod navigator;
mod resolver;
mod rustdoc_data;
pub mod search;
pub mod sources;
pub mod string_utils;

// Re-export commonly used types
pub use crate_name::CrateName;
pub use doc_ref::DocRef;
pub use navigator::{CrateInfo, Navigator};
pub use resolver::{Resolver, Suggestion};
pub use rustdoc_data::RustdocData;
pub use sources::CrateProvenance;

#[cfg(test)]
mod tests;
