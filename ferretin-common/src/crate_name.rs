use std::{fmt::Display, ops::Deref};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CrateName<'a>(
    // private so the only way to construct is through new
    pub(crate) &'a str,
);

impl Display for CrateName<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self)
    }
}

impl<'a> Deref for CrateName<'a> {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.0
    }
}

impl<'a> CrateName<'a> {
    pub fn new(name: &'a str) -> Option<Self> {
        match name {
            // rustdoc placeholders
            "alloc_crate" => Some("alloc"),
            "core_crate" => Some("core"),
            "proc_macro_crate" => Some("proc_macro"),
            "test_crate" => Some("test"),
            "std_crate" => Some("std"),

            // known unresolved pseudo-crates
            "std_detect" | "rustc_literal_escaper" => None,

            // future-proof: skip internal rustc crates
            name if name.starts_with("rustc_") => None,

            // default case: treat as real crate name
            name => Some(name),
        }
        .map(Self)
    }
}
