// Core library for rustdoc navigation and search
// Re-export rustdoc_types for convenience
pub use rustdoc_types;

pub mod conversions;
pub mod crate_name;
pub mod doc_ref;
pub mod docsrs_client;
pub mod intra_doc_links;
pub mod iterators;
pub mod navigator;
pub mod project;
pub mod search;
pub mod sources;
pub mod string_utils;

// Re-export commonly used types
pub use crate_name::CrateName;
pub use doc_ref::DocRef;
pub use navigator::Navigator;
pub use project::{CrateInfo, RustdocData, RustdocProject};
