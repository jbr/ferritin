//! Implementor shapes for the trait-page "Implementors" section, mirroring the
//! `trillium::Handler` family of impls: bounds must surface in the rendering
//! whether they're declared inline or in `where` predicates, and wherever the
//! implementing type puts the param — a tuple element, an array element with a
//! const-generic length, a bare function-like generic, or nested too deep for
//! inline display.

/// The trait whose implementors exercise bound display.
pub trait Reactor {}

/// A concrete implementor, for the compact comma list.
pub struct Simple;

/// A wrapper whose `Reactor` impls place a bounded param one level down
/// (inside `Vec`, where inline merging follows it) and behind a reference
/// (where it can't merge and falls to the trailing where clause).
pub struct Nested<T>(pub T);

impl Reactor for Simple {}

impl Reactor for () {}

impl Reactor for &'static str {}

impl<A, B> Reactor for (A, B)
where
    A: Reactor,
    B: Reactor,
{
}

impl<R: Reactor, const N: usize> Reactor for [R; N] {}

impl<F, Out> Reactor for F
where
    F: Fn(u32) -> Out + Send + Sync + 'static,
    Out: Reactor,
{
}

impl<T: Reactor> Reactor for Nested<Vec<T>> {}

impl<T: Reactor> Reactor for &'static Nested<T> {}
