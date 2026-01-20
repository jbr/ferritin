//! Intra-doc link resolution
//!
//! This module handles resolving rustdoc's intra-doc link syntax to concrete items.
//! Intra-doc links can take several forms:
//! - Simple paths: `Vec::push`, `std::vec::Vec`
//! - Disambiguated: `type@str`, `fn@clone`, `macro@vec`
//! - Relative: `method_name` (relative to current item)
//! - With anchors: `Vec#capacity-and-reallocation`
//!
//! See: https://doc.rust-lang.org/rustdoc/write-documentation/linking-to-items-by-name.html

use crate::{DocRef, Navigator};
use rustdoc_types::Item;

/// The result of resolving an intra-doc link
#[derive(Debug, Clone)]
pub enum ResolvedLink<'a> {
    /// Link resolved to a concrete item
    Item(DocRef<'a, Item>),
    /// Link is a fragment/anchor (e.g., `#heading`)
    Fragment(String),
    /// Link is external (http/https)
    External(String),
    /// Link could not be resolved
    Unresolved,
}

/// Disambiguator prefix for intra-doc links
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disambiguator {
    /// `type@` - type (struct, enum, union, type alias)
    Type,
    /// `fn@` or `function@` - function or method
    Function,
    /// `struct@` - struct
    Struct,
    /// `enum@` - enum
    Enum,
    /// `trait@` - trait
    Trait,
    /// `mod@` or `module@` - module
    Module,
    /// `const@` or `constant@` - constant
    Constant,
    /// `static@` - static
    Static,
    /// `macro@` - macro
    Macro,
    /// `union@` - union
    Union,
    /// `primitive@` - primitive type
    Primitive,
    /// `method@` - method
    Method,
    /// `field@` - field
    Field,
    /// `variant@` - enum variant
    Variant,
}

impl Disambiguator {
    /// Parse a disambiguator from a string prefix
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "type" => Some(Self::Type),
            "fn" | "function" => Some(Self::Function),
            "struct" => Some(Self::Struct),
            "enum" => Some(Self::Enum),
            "trait" => Some(Self::Trait),
            "mod" | "module" => Some(Self::Module),
            "const" | "constant" => Some(Self::Constant),
            "static" => Some(Self::Static),
            "macro" => Some(Self::Macro),
            "union" => Some(Self::Union),
            "primitive" => Some(Self::Primitive),
            "method" => Some(Self::Method),
            "field" => Some(Self::Field),
            "variant" => Some(Self::Variant),
            _ => None,
        }
    }
}

/// Parse and resolve an intra-doc link
///
/// # Arguments
/// * `navigator` - The navigator for resolving paths
/// * `origin` - The item where the link appears (required for scope-aware resolution)
/// * `link` - The link string (e.g., "Vec::push", "type@str", "#heading")
///
/// # Returns
/// The resolved link, or `Unresolved` if it couldn't be found
pub fn resolve_link<'a>(
    navigator: &'a Navigator,
    origin: DocRef<'a, Item>,
    link: &str,
) -> ResolvedLink<'a> {
    // Handle fragment-only links
    if link.starts_with('#') {
        return ResolvedLink::Fragment(link.to_string());
    }

    // Handle external URLs
    if link.starts_with("http://") || link.starts_with("https://") {
        return ResolvedLink::External(link.to_string());
    }

    // Split off any fragment/anchor
    let (path, _fragment) = link.split_once('#').unwrap_or((link, ""));

    // Parse disambiguator if present
    let (disambiguator, path) = if let Some((prefix, rest)) = path.split_once('@') {
        (Disambiguator::from_str(prefix), rest)
    } else {
        (None, path)
    };

    // Try to resolve the path
    let resolved_path = resolve_path(navigator, origin, path, disambiguator);

    match resolved_path {
        Some(item) => ResolvedLink::Item(item),
        None => ResolvedLink::Unresolved,
    }
}

/// Resolve a path to an item, considering the origin for scope-aware resolution
///
/// This uses rustdoc's pre-resolved `links` field from the origin item, which contains
/// all intra-doc links that rustdoc successfully resolved during compilation.
fn resolve_path<'a>(
    navigator: &'a Navigator,
    origin: DocRef<'a, Item>,
    path: &str,
    _disambiguator: Option<Disambiguator>,
) -> Option<DocRef<'a, Item>> {
    // IMPORTANT: Rustdoc stores links with backticks like "`Vector`" in the links map
    // We need to try both with and without backticks
    let link_key_with_backticks = format!("`{}`", path);

    // Check if the origin item has this link pre-resolved by rustdoc
    if let Some(link_id) = origin.links.get(&link_key_with_backticks) {
        // Try to get the item from the same crate first
        if let Some(item) = origin.get(link_id) {
            return Some(item);
        }

        // If not in same crate, look in paths (for external crates)
        if let Some(item_summary) = origin.crate_docs().paths.get(link_id) {
            // Try to resolve the external path
            let full_path = item_summary.path.join("::");
            let mut suggestions = vec![];
            return navigator.resolve_path(&full_path, &mut suggestions);
        }
    }

    // Fallback: Try direct path resolution (for absolute paths like "std::vec::Vec")
    // Handle "crate::", "self::", "super::" and unqualified paths
    let mut suggestions = vec![];
    let qualified_path = if let Some(rest) = path.strip_prefix("crate::") {
        // crate::Type -> current_crate::Type
        format!("{}::{}", origin.crate_docs().name, rest)
    } else if let Some(rest) = path.strip_prefix("self::") {
        // self::Type -> current_crate::Type (within the same module)
        // TODO: This should really be current_module::Type but we don't have that context
        format!("{}::{}", origin.crate_docs().name, rest)
    } else if path.contains("::") {
        // Already qualified (e.g., "std::vec::Vec")
        // Note: super:: is tricky - would need module path context
        path.to_string()
    } else {
        // Relative path (e.g., "Runtime"), qualify it with the current crate
        format!("{}::{}", origin.crate_docs().name, path)
    };

    navigator.resolve_path(&qualified_path, &mut suggestions)
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn test_disambiguator_parsing() {
        assert_eq!(Disambiguator::from_str("type"), Some(Disambiguator::Type));
        assert_eq!(Disambiguator::from_str("fn"), Some(Disambiguator::Function));
        assert_eq!(
            Disambiguator::from_str("function"),
            Some(Disambiguator::Function)
        );
        assert_eq!(
            Disambiguator::from_str("struct"),
            Some(Disambiguator::Struct)
        );
        assert_eq!(Disambiguator::from_str("invalid"), None);
    }
}

#[cfg(test)]
mod tests;
