use std::{
    borrow::Cow,
    cmp::Ordering,
    fmt::Display,
    hash::{Hash, Hasher},
    ops::Deref,
};

#[derive(Clone, Eq)]
pub struct CrateName<'a>(Cow<'a, str>);

impl CrateName<'_> {
    pub fn to_static(&self) -> CrateName<'static> {
        self.0.to_string().into()
    }
}

impl<'a> std::fmt::Debug for CrateName<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl Hash for CrateName<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        for byte in self.0.bytes() {
            if byte == b'-' {
                state.write_u8(b'_');
            } else {
                state.write_u8(byte);
            }
        }
    }
}

impl<'a> From<Cow<'a, str>> for CrateName<'a> {
    fn from(value: Cow<'a, str>) -> Self {
        Self(value)
    }
}

impl<'a> From<&'a str> for CrateName<'a> {
    fn from(value: &'a str) -> Self {
        Self(Cow::Borrowed(value))
    }
}

impl From<String> for CrateName<'_> {
    fn from(value: String) -> Self {
        Self(Cow::Owned(value))
    }
}

impl Display for CrateName<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self)
    }
}

impl Deref for CrateName<'_> {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl PartialEq for CrateName<'_> {
    fn eq(&self, other: &Self) -> bool {
        cmp_ignoring_dash_underscore(&self.0, &other.0).is_eq()
    }
}

impl Ord for CrateName<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        cmp_ignoring_dash_underscore(&self.0, &other.0)
    }
}

impl PartialOrd for CrateName<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Compare strings treating `-` and `_` as equivalent.
///
/// Folds `-` to `_` on both sides, then compares byte-by-byte. This matches
/// the `Hash` impl (which folds `-` to `_`) so `Eq`, `Ord`, and `Hash` agree.
fn cmp_ignoring_dash_underscore(a: &str, b: &str) -> Ordering {
    let normalize = |c: u8| if c == b'-' { b'_' } else { c };
    a.bytes().map(normalize).cmp(b.bytes().map(normalize))
}
