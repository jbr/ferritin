use std::vec::Vec as Vector;

/// This uses a renamed import: [`Vector`]
///
/// This references std directly: [`std::vec::Vec`]
///
/// This uses a relative path: [`MyStruct`]
///
/// Qualified path: [`link_test::MyStruct`]
///
/// This references a method: [`Vector::push`]
pub mod something {
    /// A simple struct with a link to [`super::MyStruct`]
    pub struct Inner;
}

/// A struct that links to [`Vector`] and [`something::Inner`]
///
/// Also tests without backticks: [Vector] and [MyStruct]
///
/// And with just the type name directly: Vector
pub struct MyStruct {
    /// Field that references [`Vector`]
    pub data: Vec<i32>,
}

impl MyStruct {
    /// Method that references [`Self`] and [`Vector`]
    pub fn new() -> Self {
        Self { data: Vec::new() }
    }
}
