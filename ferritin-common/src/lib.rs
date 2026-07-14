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
mod store;
pub mod string_utils;

// Re-export commonly used types
pub use crate_name::CrateName;
pub use doc_ref::DocRef;
pub use navigator::Navigator;
pub use resolver::{CratePath, Resolver, Suggestion};
pub use rustdoc_data::RustdocData;
pub use sources::CrateProvenance;
pub use store::{CrateInfo, Store};

#[cfg(test)]
mod tests;
