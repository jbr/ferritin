//! A minimal test crate for rustdoc JSON testing

// Use statements for testing intra-doc link resolution
use std::collections::HashMap;
use std::result::Result as StdResult;
use std::vec::Vec as Vector;

/// A simple struct for testing basic functionality.
///
/// This struct demonstrates basic usage patterns and should show completely
/// since it only has one paragraph of documentation.
///
/// It uses [`Vector`] for testing intra-doc link resolution with renamed imports.
/// Also tests [`HashMap`] which is a non-renamed import.
#[derive(Debug, Clone)]
pub struct TestStruct {
    /// A public field
    pub field: String,
    /// Another public field
    pub count: u32,
    /// A private field
    private_field: bool,
}

/// A generic struct for testing multi-paragraph documentation.
///
/// This struct demonstrates how generics work with complex type bounds
/// and provides a comprehensive example of the generic system in Rust.
///
/// ## Usage Examples
///
/// You can create instances with different type parameters:
/// - `GenericStruct<i32>` for integer data
/// - `GenericStruct<String, CustomDisplay>` for custom types
///
/// ## Implementation Notes
///
/// The struct uses trait bounds to ensure type safety and provides
/// default type parameters for common use cases.
pub struct GenericStruct<T, U = String>
where
    T: Clone + Send,
    U: std::fmt::Display,
{
    /// Generic field
    pub data: T,
    /// Generic field with default
    pub metadata: U,
    /// Private generic field  
    inner: Vec<T>,
    /// Another private field
    secret: String,
}

/// A trait for testing extremely long documentation that exceeds line limits.
///
/// This trait provides a comprehensive interface for data processing operations.
/// It demonstrates various method signatures including mutable references,
/// error handling, and different return types. The trait is designed to be
/// flexible and extensible for different use cases in data processing pipelines.
/// Each method serves a specific purpose in the data transformation workflow.
/// The implementation should handle edge cases gracefully and provide meaningful
/// error messages when operations fail. This documentation intentionally spans
/// many lines to test the line-based truncation when paragraph truncation
/// doesn't apply. We want to see how the system handles documentation that
/// goes well beyond the 16-line limit and should trigger line-based truncation.
/// This continues for several more lines to ensure we exceed the limit.
/// Line 14 of this very long paragraph that should be truncated.
/// Line 15 of this extremely verbose documentation example.
/// Line 16 which should be the last line shown in brief mode.
/// Line 17 that should be hidden and show a truncation indicator.
/// Line 18 that definitely won't be visible in brief mode.
///
/// ## Additional sections after the long paragraph
///
/// This section should not be visible in brief mode since the first
/// paragraph already exceeded the line limit.
pub trait TestTrait {
    /// trait associated constant
    const ASSOCIATED_CONSTANT: ();
    /// trait associated type
    type T: Clone;

    /// A method
    fn test_method(&self) -> String;

    /// Another method with parameters
    fn process(&mut self, data: &str) -> Result<(), String>;
}

impl TestStruct {
    /// This is an associated constant for a struct
    pub const ASSOCIATED_CONST: () = ();

    /// Create a new TestStruct
    pub fn new(field: String, count: u32) -> Self {
        Self {
            field,
            count,
            private_field: false,
        }
    }

    /// Get the field value
    pub fn get_field(&self) -> &str {
        &self.field
    }

    /// Update the count
    pub fn increment_count(&mut self) {
        self.count += 1;
    }
}

impl TestTrait for TestStruct {
    const ASSOCIATED_CONSTANT: () = ();
    type T = String;
    fn test_method(&self) -> String {
        format!("{}: {}", self.field, self.count)
    }

    fn process(&mut self, data: &str) -> Result<(), String> {
        self.field = data.to_string();
        Ok(())
    }
}

/// A public function
pub fn test_function(input: &str) -> String {
    format!("processed: {}", input)
}

/// A generic function
pub fn generic_function<T, U>(data: T, transform: U) -> String
where
    T: std::fmt::Debug,
    U: Fn(T) -> String,
{
    transform(data)
}

/// An async function
pub async fn async_function(delay: u64) -> Result<String, Box<dyn std::error::Error>> {
    Ok(format!("waited {delay} ms"))
}

/// A private function  
fn private_function() -> i32 {
    42
}

/// A module with items
pub mod submodule {
    /// A struct in a submodule
    pub struct SubStruct {
        /// A value field
        pub value: i32,
    }

    impl SubStruct {
        /// Create a new SubStruct
        pub fn new(value: i32) -> Self {
            Self { value }
        }

        /// Get the value
        pub fn get_value(&self) -> i32 {
            self.value
        }

        /// Double the value
        pub fn double(&mut self) {
            self.value *= 2;
        }
    }

    /// A function in a submodule
    pub fn sub_function() -> &'static str {
        "from submodule"
    }

    /// An enum for testing
    ///
    /// This is like [`crate::GenericEnum`] but without the generic
    pub enum TestEnum {
        /// Variant A (see also [`crate::GenericEnum`])
        VariantA,
        /// Variant B with data
        VariantB(String),
        /// Variant C with struct data (`name` and `value`)
        VariantC {
            /// Documentation for the name field
            name: String,
            /// Documentation for value
            value: i32,
        },
    }

    pub use TestEnum::*;
}

/// A const for testing
pub const TEST_CONSTANT: i32 = 42;

/// A static for testing
pub static TEST_STATIC: &str = "hello world";

/// A unit struct for testing
pub struct UnitStruct;

/// A tuple struct for testing
pub struct TupleStruct(
    /// It's probably uncommon to add documentation for a tuple struct field
    pub String,
    u32,
);

/// A generic enum for testing
///
/// See also [`crate::TestEnum`]
pub enum GenericEnum<T, U = String>
where
    T: Clone + Send,
    U: std::fmt::Display,
{
    /// Simple variant
    Simple,
    /// Variant with generic data
    WithData(T),
    /// Variant with mixed generics
    Mixed {
        data: T,
        /// Info can be any U as long as it's [`std::fmt::Display`]
        info: U,
    },
}

/// A more complex trait demonstrating various features
pub trait ComplexTrait<T>
where
    T: Clone + Send,
{
    /// An associated type
    type Output: std::fmt::Display;

    /// An associated constant
    const MAX_SIZE: usize = 100;

    /// A simple method
    fn process(&self, input: T) -> Self::Output;

    /// A method with default implementation
    fn is_ready(&self) -> bool {
        true
    }

    /// A method with complex generics
    fn transform<U>(&self, data: U) -> Result<T, String>
    where
        U: Into<T>;
}

/// Module for testing intra-doc link resolution
pub mod link_resolution_tests {
    pub use super::TestStruct as RenamedTestStruct;
    pub use super::submodule::SubStruct;
    pub use std::collections::BTreeMap as Tree;
    pub use std::collections::HashSet;

    /// Struct in link test module
    pub struct LinkTestStruct {
        /// Field in link test struct
        pub data: String,
    }

    impl LinkTestStruct {
        /// Method for testing Self resolution
        pub fn new() -> Self {
            Self {
                data: String::new(),
            }
        }

        /// Another method
        pub fn get_data(&self) -> &str {
            &self.data
        }
    }

    /// Nested module for testing scoped resolution
    pub mod nested {
        pub use super::super::TestTrait;
        pub use std::string::String as Str;

        /// Struct in nested module
        pub struct NestedStruct {
            /// Field
            pub value: i32,
        }

        impl NestedStruct {
            /// Create new NestedStruct
            pub fn new(value: i32) -> Self {
                Self { value }
            }
        }

        /// Another nested module
        pub mod deeply_nested {
            use super::super::LinkTestStruct;

            /// Struct in deeply nested module
            pub struct DeepStruct;
        }
    }
}

pub use std::vec::Vec;
pub use submodule::*;
pub mod reexport_mod {
    pub use super::submodule::*;
}


pub mod markdown_test {
    #![doc=include_str!("./markdown_test.md")]
}

/// Module for testing namespace disambiguation with kind discriminators.
///
/// Contains a sub-module and a function that share the same name, creating a
/// genuine module-function collision in rustdoc's paths map.
pub mod namespace_collisions {
    /// A module sharing its name with [`both()`] below.
    pub mod both {
        /// An item inside the colliding module.
        pub struct Inside;
    }

    /// A function sharing its name with the [`both`] module above.
    pub fn both() {}
}

/// Private module whose items are accessible only via re-export.
///
/// Items here appear in rustdoc's `paths` map with a path that goes through this
/// private module (e.g. `fixture_crate::private_detail::ReachableViaPrivateModule`),
/// which tree traversal cannot follow since `private_detail` is not a public child.
/// They should be resolved via the `path_to_id` reverse index instead.
mod private_detail {
    /// A struct accessible only via re-export from a private module.
    pub struct ReachableViaPrivateModule;

    impl ReachableViaPrivateModule {
        /// A method on a struct whose module is private.
        ///
        /// This exercises the combined case: the method is absent from rustdoc's
        /// `paths` map (rust-lang/rust#152511), and the parent struct's
        /// `ItemSummary::path` passes through this private module, so tree
        /// traversal cannot anchor on the parent either.
        pub fn private_module_method(&self) {}
    }
}
pub use private_detail::ReachableViaPrivateModule;

/// Edge cases for path-prefix resolution in use-item sources and intra-doc links.
///
/// Rustdoc emits `Use::source` and intra-doc link targets verbatim from the Rust
/// source: `crate::`, `self::`, and `super::` prefixes are kept as-written. When
/// the corresponding id resolves to an item in the *local* crate's index (as is
/// the case for all the `pub use`s below), the iterator never needs to fall back
/// to resolving the source string. The cross-crate counterparts live in
/// `tests/test-workspace/crate-b` — those actually exercise the fallback path.
///
/// The intra-doc links in this module's docs and its members' docs exercise the
/// same prefix handling on the `extract_link_target` side (rendered in
/// snapshots).
pub mod prefix_tests {
    /// A sibling target for `self::` / `super::PrefixMarker` references.
    pub struct PrefixMarker;

    /// Intra-doc links from the outer module:
    /// - [`crate::TestStruct`] — absolute path from crate root.
    /// - [`self::PrefixMarker`] — sibling in the same module.
    /// - [`super::TestStruct`] — parent of this module is the crate root.
    /// - [`crate::submodule::TestEnum`] — deep absolute path.
    pub struct DocOuter;

    /// A nested module to exercise `self::`, `super::`, and `super::super::`.
    pub mod deep {
        /// A marker target inside `deep`.
        pub struct DeepMarker;

        /// Intra-doc links from a nested module:
        /// - [`self::DeepMarker`] — local sibling.
        /// - [`super::PrefixMarker`] — one module up.
        /// - [`super::super::TestStruct`] — two modules up (crate root).
        /// - [`crate::TestStruct`] — absolute path from crate root.
        pub struct DocDeep;

        // Same-crate prefixed uses. Each one's `use.id` is valid and present in
        // this crate's local index, so the iterator resolves via the id path and
        // never touches `source`. They exist to pin down what rustdoc emits for
        // each prefix form — inspected by tests that look at the raw JSON.
        pub use self::DeepMarker as SelfAliasDeep;
        pub use super::PrefixMarker as SuperAlias;
        pub use super::super::TestStruct as SuperSuperAlias;
        pub use crate::submodule::SubStruct as CrateAlias;

        /// Exercises the "glob brings a name into scope, then re-export that
        /// name" pattern, all within the same crate.
        pub mod glob_reexport {
            pub use super::super::*; // brings PrefixMarker, DocOuter into scope

            pub use PrefixMarker as GlobReexportedMarker;
        }
    }
}
