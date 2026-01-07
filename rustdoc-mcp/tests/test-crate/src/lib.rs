//! A minimal test crate for rustdoc JSON MCP testing

/// A simple struct for testing basic functionality.
///
/// This struct demonstrates basic usage patterns and should show completely
/// since it only has one paragraph of documentation.
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
    pub enum TestEnum {
        /// Variant A
        VariantA,
        /// Variant B with data
        VariantB(String),
        /// Variant C with struct data
        VariantC { name: String, value: i32 },
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
    Mixed { data: T, info: U },
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

pub use std::vec::Vec;
pub use submodule::*;
pub mod reexport_mod {
    pub use super::submodule::*;
}
