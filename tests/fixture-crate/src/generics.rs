//! Generic-syntax edge cases, gathered here so signature rendering has a
//! fixture for every shape rustdoc JSON can produce.
//!
//! Several of these are shapes where the JSON is *not* a transcription of the
//! source: `impl Trait` in argument position becomes a synthetic generic
//! parameter whose name is the whole `impl` type, higher-ranked bounds live in
//! a `generic_params` list beside the trait rather than inside its path, and a
//! `dyn` object's own lifetime is a field rather than one of its bounds.

use std::borrow::Cow;
use std::fmt::{Debug, Display};

/// `impl Trait` in argument position — the whole reason this module exists.
///
/// Rustdoc lowers this to a *synthetic* generic parameter (`is_synthetic`)
/// literally named `impl Into<Cow<'static, str>>`, bounded by the `impl`'s own
/// bounds. Rendering the parameter list verbatim produces
/// `fn set_data<impl Into<Cow<'static, str>>: Into<Cow<'static, str>>>`, which
/// is not Rust.
pub fn set_data(data: impl Into<Cow<'static, str>>) -> String {
    data.into().into_owned()
}

/// A real parameter and a synthetic one in the same signature: the `<T>` list
/// must survive while the synthetic entry drops out.
pub fn mixed_params<T: Clone>(first: T, second: impl Into<String>) -> (T, String) {
    (first, second.into())
}

/// Two synthetic parameters, so eliding them cannot leave a stray separator.
pub fn two_impls(a: impl Debug, b: impl Display) -> String {
    format!("{a:?}{b}")
}

/// An `impl Trait` argument nested inside another type's generic arguments.
pub fn nested_impl_trait(items: Vec<impl Into<String>>) -> usize {
    items.len()
}

/// An `impl Trait` argument with several bounds, alongside a real parameter
/// whose bound is in a `where` clause — so a synthetic parameter and a `where`
/// clause coexist.
pub fn impl_and_where<T>(value: T, extra: impl Debug + Clone + Send) -> String
where
    T: Display,
{
    format!("{value}{extra:?}")
}

/// `impl Trait` in *return* position, which is a real `Type::ImplTrait` rather
/// than a synthetic parameter.
pub fn returns_impl_trait() -> impl Iterator<Item = u32> {
    0..10
}

/// Return-position `impl Trait` with a precise-capturing `use<..>` bound.
pub fn precise_capturing<T: Clone>(value: &T) -> impl Clone + use<'_, T> {
    value.clone()
}

/// Return-position `impl Trait` carrying a lifetime bound alongside a trait
/// bound, so `GenericBound::Outlives` appears among an `impl`'s bounds.
pub fn impl_trait_outlives<'a>(text: &'a str) -> impl Iterator<Item = &'a str> + 'a {
    text.split(' ')
}

/// A higher-ranked trait bound written inline in the parameter list.
pub fn hrtb_inline<F: for<'a> Fn(&'a str) -> &'a str>(f: F) -> usize {
    f("x").len()
}

/// The same bound moved into a `where` clause, where rustdoc records the
/// `for<'a>` on the predicate instead of on the parameter.
pub fn hrtb_where<F>(f: F) -> usize
where
    F: for<'a> Fn(&'a str) -> &'a str,
{
    f("x").len()
}

/// A higher-ranked bound on a `dyn` object. The `for<'a>` lives in
/// `PolyTrait::generic_params`, beside the trait path rather than inside it.
pub fn hrtb_dyn(f: &dyn for<'a> Fn(&'a str) -> bool) -> bool {
    f("x")
}

/// A higher-ranked bound on a bare function pointer.
pub fn hrtb_fn_pointer(f: for<'a> fn(&'a str) -> &'a str) -> usize {
    f("x").len()
}

/// A `dyn` object carrying an explicit lifetime bound, which rustdoc stores in
/// `DynTrait::lifetime` rather than among the traits.
pub fn dyn_with_lifetime(e: Box<dyn std::error::Error + Send + 'static>) -> String {
    e.to_string()
}

/// A multi-trait `dyn` object behind a reference: valid Rust only with the
/// parentheses, so the rendering has to supply them.
pub fn dyn_needs_parens(e: &(dyn std::error::Error + Send)) -> String {
    e.to_string()
}

/// A `dyn` trait whose generic arguments are parenthesized (`Fn` sugar) rather
/// than angle-bracketed.
pub fn dyn_parenthesized_args(f: Box<dyn Fn(u32, &str) -> String>) -> String {
    f(0, "")
}

/// A `?Sized` relaxed bound, one of the two `TraitBoundModifier`s.
pub fn maybe_sized<T: ?Sized + Debug>(value: &T) -> String {
    format!("{value:?}")
}

/// Lifetime parameters with an outlives relationship between them.
pub fn lifetime_outlives<'long: 'short, 'short>(long: &'long str, short: &'short str) -> &'short str {
    if long.len() > short.len() { short } else { long }
}

/// An associated-item *equality* constraint inside generic arguments.
pub fn assoc_equality<I>(iter: I) -> usize
where
    I: Iterator<Item = u8>,
{
    iter.count()
}

/// An associated-item *bound* constraint (`Item: Clone`) inside generic
/// arguments — the other `AssocItemConstraintKind`.
pub fn assoc_bound<I: Iterator<Item: Clone + Debug>>(iter: I) -> usize {
    iter.count()
}

/// A qualified path (`<I as Iterator>::Item`) in return position.
pub fn qualified_return<I: Iterator>(mut iter: I) -> Option<<I as Iterator>::Item> {
    iter.next()
}

/// A const generic with a default, alongside a type parameter with a default.
pub struct ConstGeneric<T = u8, const N: usize = 3> {
    /// The backing array.
    pub items: [T; N],
}

impl<T: Copy + Default, const N: usize> ConstGeneric<T, N> {
    /// A method whose signature mentions the const parameter.
    pub fn filled(value: T) -> Self {
        Self { items: [value; N] }
    }
}

/// A struct with a where clause that has several predicates, so the multi-line
/// `where` rendering is exercised alongside the single-predicate form.
pub struct ManyPredicates<T, U, F>
where
    T: Clone + Send + 'static,
    U: Display,
    F: for<'a> Fn(&'a T) -> U,
{
    /// A value.
    pub value: T,
    /// Another value.
    pub other: U,
    /// A callback.
    pub callback: F,
}

/// A tuple struct with a `where` clause, which Rust writes *after* the field
/// list (`struct T(..) where ..;`) rather than before it.
pub struct TupleWithWhere<T>(pub T)
where
    T: Display;

/// An enum with a multi-predicate `where` clause, the enum counterpart to
/// [`ManyPredicates`].
pub enum EnumManyPredicates<T, U>
where
    T: Clone + Send,
    U: Display,
{
    /// Holds a `T`.
    First(T),
    /// Holds a `U`.
    Second(U),
}

/// A trait with a generic method taking `impl Trait`, so the synthetic
/// parameter also appears on an associated item rather than a free function.
pub trait ImplTraitMethods {
    /// Takes `impl Trait` in argument position.
    fn accept(&self, value: impl Into<String>) -> String;

    /// Returns `impl Trait` from a trait method (RPITIT).
    fn produce(&self) -> impl Debug;
}

impl ImplTraitMethods for ConstGeneric<u8, 1> {
    fn accept(&self, value: impl Into<String>) -> String {
        value.into()
    }

    fn produce(&self) -> impl Debug {
        self.items
    }
}
