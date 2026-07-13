//! Recognizing rendered-documentation URLs and recovering the item path they name.
//!
//! Plenty of crates link to docs.rs (or doc.rust-lang.org) by hand instead of
//! writing an intra-doc link, both across crates and within one. Those arrive here
//! as ordinary external URLs; parsing them back into a lookup path is what lets
//! them navigate in-app rather than kicking the reader out to a browser.

use super::kind_for_sigil;
use rustdoc_types::ItemKind;
use semver::{Version, VersionReq};
use std::fmt;

/// Std-library crates documented at a doc.rust-lang.org channel root. Everything
/// else served there (`/book/`, `/reference/`, `/nightly/nightly-rustc/`) is prose
/// or a different rustdoc tree, and names no item we can resolve.
const STD_CRATES: &[&str] = &["std", "core", "alloc", "proc_macro", "test"];

/// docs.rs paths whose leading segment is a site route, not a crate name.
/// `crate` fronts the metadata and source-browsing pages; `-` fronts static assets.
const DOCS_RS_ROUTES: &[&str] = &["crate", "releases", "about", "search", "-"];

/// rustdoc anchor prefixes that name a child item our resolver can reach, and that
/// child's kind.
///
/// `tymethod` is a required trait method, `method` a provided or inherent one;
/// both are `Function` to us.
///
/// `structfield` is pointedly absent. `resolve_path` cannot reach a struct field, so
/// folding `#structfield.x` into the path would produce a link that resolves to
/// nothing; left unfolded, the path stops at the struct — the page the URL names
/// anyway — and the anchor survives in [`DocsRsLink::fragment`]. `Span::nav_path`
/// makes the same choice from the other direction.
const FRAGMENT_KINDS: &[(&str, ItemKind)] = &[
    ("method", ItemKind::Function),
    ("tymethod", ItemKind::Function),
    ("variant", ItemKind::Variant),
    ("associatedtype", ItemKind::AssocType),
    ("associatedconstant", ItemKind::AssocConst),
];

/// An item addressed by a docs.rs or doc.rust-lang.org URL.
///
/// [`Display`](fmt::Display) renders the path our resolver accepts —
/// `tokio@1.40.0::runtime::Runtime::spawn`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DocsRsLink<'a> {
    /// The docs.rs slug (i.e. the crates.io name), or the std crate name.
    pub(crate) krate: &'a str,
    /// `None` when the URL said `latest`, or named no version at all — both mean
    /// the same thing to the resolver as an absent version requirement.
    pub(crate) version: Option<&'a str>,
    /// The item's path relative to the crate root.
    pub(crate) path: Vec<&'a str>,
    /// The kind of the item `path` names — taken from the page sigil, or from the
    /// anchor when one named a child item.
    pub(crate) kind: ItemKind,
    /// An anchor naming no item (`impl-Display-for-Foo`, `examples`), kept verbatim
    /// so the original URL can be reconstructed. Item-naming anchors are folded
    /// into `path` instead, and leave this `None`.
    pub(crate) fragment: Option<&'a str>,
}

impl fmt::Display for DocsRsLink<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.krate)?;
        if let Some(version) = self.version {
            write!(f, "@{version}")?;
        }
        for segment in &self.path {
            write!(f, "::{segment}")?;
        }
        Ok(())
    }
}

impl<'a> DocsRsLink<'a> {
    /// Parse a rendered-documentation URL, or `None` if it isn't one — or is one
    /// that addresses no single item (`all.html`, a source listing, the Rust book).
    pub(crate) fn parse(url: &'a str) -> Option<Self> {
        let rest = url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"))?;
        let (host, rest) = rest.split_once('/').unwrap_or((rest, ""));

        // A query string precedes the fragment, so peel the fragment off first.
        let (rest, fragment) = rest.split_once('#').unwrap_or((rest, ""));
        let (rest, _query) = rest.split_once('?').unwrap_or((rest, ""));

        let segments: Vec<&'a str> = rest.split('/').filter(|s| !s.is_empty()).collect();

        let mut link = match host {
            "docs.rs" => Self::parse_docs_rs(&segments)?,
            // `docs.rust-lang.org` is a legacy alias that redirects to `doc.`.
            "doc.rust-lang.org" | "docs.rust-lang.org" => Self::parse_rust_lang(&segments)?,
            _ => return None,
        };

        link.apply_fragment(fragment);
        Some(link)
    }

    /// `docs.rs/{slug}/{version}/{target}?/{lib_name}/{module…}/{page}`
    ///
    /// The segment before the module path is the crate's *library* name, which can
    /// differ from the docs.rs slug by more than dash/underscore folding —
    /// `docs.rs/sha-1/latest/sha1/struct.Sha1.html`. We drop it and keep the slug,
    /// because the slug is what the version qualifies and what the resolver looks
    /// up; `sha1` is an unrelated crate on crates.io.
    ///
    /// A target triple may precede the library name, for crates whose docs were
    /// built for several targets (`docs.rs/bitflags/2.9.1/x86_64-unknown-linux-gnu/
    /// bitflags/struct.Flags.html`). It names no item, so it too is dropped.
    fn parse_docs_rs(segments: &[&'a str]) -> Option<Self> {
        let (&krate, rest) = segments.split_first()?;
        if DOCS_RS_ROUTES.contains(&krate) {
            return None;
        }

        let Some((&version, rest)) = rest.split_first() else {
            return Some(Self::crate_root(krate, None));
        };
        let version = parse_version(version)?;

        // A target triple always contains a hyphen; a library name, being a Rust
        // identifier, never can — so this tells the two apart unambiguously.
        let rest = match rest.split_first() {
            Some((first, tail)) if first.contains('-') => tail,
            _ => rest,
        };

        // Drop the library-name segment; the path we build is crate-relative.
        let rest = rest.split_first().map_or(&[][..], |(_, rest)| rest);

        let (path, kind) = parse_item_page(rest)?;
        Some(Self {
            krate,
            version,
            path,
            kind,
            fragment: None,
        })
    }

    /// `doc.rust-lang.org/{channel}?/{crate}/{module…}/{page}`
    ///
    /// The channel (`nightly`, `stable`, `1.75.0`) is optional and says nothing
    /// about a *crate* version, so it is discarded rather than becoming one.
    fn parse_rust_lang(segments: &[&'a str]) -> Option<Self> {
        let (&first, rest) = segments.split_first()?;

        let (krate, rest) = if STD_CRATES.contains(&first) {
            (first, rest)
        } else if is_channel(first) {
            let (&krate, rest) = rest.split_first()?;
            if !STD_CRATES.contains(&krate) {
                return None;
            }
            (krate, rest)
        } else {
            return None;
        };

        let (path, kind) = parse_item_page(rest)?;
        Some(Self {
            krate,
            version: None,
            path,
            kind,
            fragment: None,
        })
    }

    fn crate_root(krate: &'a str, version: Option<&'a str>) -> Self {
        Self {
            krate,
            version,
            path: Vec::new(),
            kind: ItemKind::Module,
            fragment: None,
        }
    }

    /// Fold an anchor into the path when it names a child item, so that
    /// `struct.Runtime.html#method.spawn` becomes the single lookup path
    /// `…::Runtime::spawn`. The kind then describes the *child*, since that is what
    /// `path` names. Anchors naming no item are kept in [`Self::fragment`].
    fn apply_fragment(&mut self, fragment: &'a str) {
        if fragment.is_empty() {
            return;
        }

        if let Some((prefix, name)) = fragment.split_once('.')
            && is_ident(name)
            && let Some((_, kind)) = FRAGMENT_KINDS.iter().find(|(p, _)| *p == prefix)
        {
            self.path.push(name);
            self.kind = *kind;
            return;
        }

        self.fragment = Some(fragment);
    }
}

/// Resolve a relative link against the page it appears on, yielding an absolute URL.
///
/// `page` is a full documentation URL; any fragment on it is dropped, and the link is
/// resolved against the *directory* containing it, per ordinary URL semantics. A
/// fragment on `relative` is carried through untouched, for [`DocsRsLink::parse`] to
/// interpret.
///
/// `None` if `relative` is not relative, or walks above the host.
pub(crate) fn resolve_relative(page: &str, relative: &str) -> Option<String> {
    if relative.starts_with('/') || relative.contains("://") {
        return None;
    }

    let page = page.split_once('#').map_or(page, |(before, _)| before);
    let (scheme, rest) = page.split_once("://")?;

    // `segments[0]` is the host. Dropping the last segment leaves the page's
    // directory — and for a URL already naming a directory (a trailing slash), that
    // last segment is empty, so the same step is correct.
    let mut segments: Vec<&str> = rest.split('/').collect();
    segments.pop()?;
    if segments.is_empty() {
        return None;
    }

    for part in relative.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if segments.len() <= 1 {
                    return None; // would escape above the host
                }
                segments.pop();
            }
            part => segments.push(part),
        }
    }

    Some(format!("{scheme}://{}", segments.join("/")))
}

/// The module path and page filename following the crate root:
/// `net/struct.TcpStream.html` → (`["net", "TcpStream"]`, `Struct`).
///
/// `None` for pages addressing no single item — `all.html`, and any filename whose
/// sigil rustdoc doesn't emit.
fn parse_item_page<'a>(segments: &[&'a str]) -> Option<(Vec<&'a str>, ItemKind)> {
    let Some((&last, mods)) = segments.split_last() else {
        return Some((Vec::new(), ItemKind::Module));
    };

    // A bare trailing directory (`…/net/`) is a module, as is its index page.
    if !last.ends_with(".html") {
        return Some((segments.to_vec(), ItemKind::Module));
    }
    if last == "index.html" {
        return Some((mods.to_vec(), ItemKind::Module));
    }

    let (sigil, name) = last.strip_suffix(".html")?.split_once('.')?;
    let kind = kind_for_sigil(sigil)?;

    let mut path = mods.to_vec();
    path.push(name);
    Some((path, kind))
}

/// docs.rs version segments are `latest` or a semver requirement (`1.40.0`, `1`,
/// `~1.40`). `latest` carries no constraint, so it maps to `None` — the same thing
/// an absent version means. A segment that parses as neither was never a version,
/// which means the URL was never a crate documentation URL.
fn parse_version(segment: &str) -> Option<Option<&str>> {
    if segment == "latest" {
        Some(None)
    } else if VersionReq::parse(segment).is_ok() {
        Some(Some(segment))
    } else {
        None
    }
}

fn is_channel(segment: &str) -> bool {
    matches!(segment, "nightly" | "beta" | "stable") || Version::parse(segment).is_ok()
}

/// A bare Rust identifier — the shape of every anchor that names an item.
/// Disambiguated anchors (`method.spawn-1`) and nested ones (`variant.A.field.0`)
/// deliberately fail this test and stay opaque fragments.
fn is_ident(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with(|c: char| c.is_ascii_digit())
        && s.chars().all(|c| c.is_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Assert a URL parses to the given lookup path and kind.
    #[track_caller]
    fn assert_link(url: &str, expected: &str, kind: ItemKind) {
        let link = DocsRsLink::parse(url).unwrap_or_else(|| panic!("failed to parse {url}"));
        assert_eq!(link.to_string(), expected, "path mismatch for {url}");
        assert_eq!(link.kind, kind, "kind mismatch for {url}");
    }

    #[track_caller]
    fn assert_unparsed(url: &str) {
        assert_eq!(
            DocsRsLink::parse(url),
            None,
            "expected {url} to be unparsed"
        );
    }

    #[test]
    fn docs_rs_item_pages() {
        assert_link(
            "https://docs.rs/tokio/latest/tokio/net/struct.TcpStream.html",
            "tokio::net::TcpStream",
            ItemKind::Struct,
        );
        assert_link(
            "https://docs.rs/tokio/1.40.0/tokio/runtime/struct.Runtime.html",
            "tokio@1.40.0::runtime::Runtime",
            ItemKind::Struct,
        );
        assert_link(
            "https://docs.rs/serde/latest/serde/trait.Serialize.html",
            "serde::Serialize",
            ItemKind::Trait,
        );
        assert_link(
            "https://docs.rs/serde/latest/serde/derive.Serialize.html",
            "serde::Serialize",
            ItemKind::ProcDerive,
        );
        assert_link(
            "https://docs.rs/tokio/latest/tokio/attr.main.html",
            "tokio::main",
            ItemKind::ProcAttribute,
        );
        assert_link(
            "https://docs.rs/tokio/latest/tokio/macro.select.html",
            "tokio::select",
            ItemKind::Macro,
        );
    }

    #[test]
    fn docs_rs_modules_and_roots() {
        assert_link("https://docs.rs/tokio", "tokio", ItemKind::Module);
        assert_link(
            "https://docs.rs/tokio/1.40.0",
            "tokio@1.40.0",
            ItemKind::Module,
        );
        assert_link(
            "https://docs.rs/tokio/latest/tokio/",
            "tokio",
            ItemKind::Module,
        );
        assert_link(
            "https://docs.rs/tokio/latest/tokio/index.html",
            "tokio",
            ItemKind::Module,
        );
        assert_link(
            "https://docs.rs/tokio/latest/tokio/task/index.html",
            "tokio::task",
            ItemKind::Module,
        );
        assert_link(
            "https://docs.rs/tokio/latest/tokio/sync/mpsc/",
            "tokio::sync::mpsc",
            ItemKind::Module,
        );
    }

    /// The version segment accepts any semver requirement docs.rs does.
    #[test]
    fn version_requirements() {
        assert_link(
            "https://docs.rs/tokio/1/tokio/struct.Foo.html",
            "tokio@1::Foo",
            ItemKind::Struct,
        );
        assert_link(
            "https://docs.rs/tokio/~1.40/tokio/struct.Foo.html",
            "tokio@~1.40::Foo",
            ItemKind::Struct,
        );
    }

    /// The slug is kept and the library-name segment dropped: `sha1` is a *different*
    /// crate on crates.io than `sha-1`, so emitting the library name would resolve
    /// the wrong crate.
    #[test]
    fn library_name_differing_from_slug() {
        assert_link(
            "https://docs.rs/sha-1/0.10.1/sha1/struct.Sha1.html",
            "sha-1@0.10.1::Sha1",
            ItemKind::Struct,
        );
    }

    /// docs.rs interposes a target triple for crates documented on several targets.
    #[test]
    fn target_triple_segment_is_dropped() {
        assert_link(
            "https://docs.rs/bitflags/2.9.1/x86_64-unknown-linux-gnu/bitflags/struct.Flags.html",
            "bitflags@2.9.1::Flags",
            ItemKind::Struct,
        );
        assert_link(
            "https://docs.rs/async-lock/3.4.0/x86_64-unknown-linux-gnu/",
            "async-lock@3.4.0",
            ItemKind::Module,
        );
        assert_link(
            "https://docs.rs/wasm-bindgen/0.2.100/wasm32-unknown-unknown/wasm_bindgen/fn.throw_str.html",
            "wasm-bindgen@0.2.100::throw_str",
            ItemKind::Function,
        );
    }

    /// Shapes taken verbatim from doc comments and `html_root_url`s in cached
    /// rustdoc JSON, which is where the target-triple form turned up.
    #[test]
    fn shapes_found_in_the_wild() {
        assert_link(
            "https://docs.rs/async-channel/latest/async_channel/struct.Receiver.html",
            "async-channel::Receiver",
            ItemKind::Struct,
        );
        assert_link(
            "https://docs.rs/futures-io/latest/futures_io/trait.AsyncRead.html",
            "futures-io::AsyncRead",
            ItemKind::Trait,
        );
        assert_link("https://docs.rs/async-net", "async-net", ItemKind::Module);
        assert_link("https://docs.rs/cfg-if/", "cfg-if", ItemKind::Module);
        assert_link(
            "https://docs.rs/adler2/2.0.0/",
            "adler2@2.0.0",
            ItemKind::Module,
        );
    }

    #[test]
    fn item_naming_fragments_fold_into_path() {
        assert_link(
            "https://docs.rs/tokio/latest/tokio/runtime/struct.Runtime.html#method.spawn",
            "tokio::runtime::Runtime::spawn",
            ItemKind::Function,
        );
        assert_link(
            "https://docs.rs/serde/latest/serde/trait.Serialize.html#tymethod.serialize",
            "serde::Serialize::serialize",
            ItemKind::Function,
        );
        assert_link(
            "https://docs.rs/tokio/latest/tokio/sync/enum.TryLockError.html#variant.Poisoned",
            "tokio::sync::TryLockError::Poisoned",
            ItemKind::Variant,
        );
        assert_link(
            "https://doc.rust-lang.org/std/ops/trait.Deref.html#associatedtype.Target",
            "std::ops::Deref::Target",
            ItemKind::AssocType,
        );
    }

    /// Anchors that name no item leave the path at the page's own item, and are kept
    /// verbatim so the URL stays reconstructible.
    #[test]
    fn non_item_fragments_are_kept_raw() {
        for (url, fragment) in [
            (
                "https://docs.rs/tokio/latest/tokio/struct.Foo.html#impl-Display-for-Foo",
                "impl-Display-for-Foo",
            ),
            (
                "https://docs.rs/tokio/latest/tokio/struct.Foo.html#implementations",
                "implementations",
            ),
            // A disambiguated method anchor: `spawn-1` is not an identifier, and we
            // have no way to tell which overload it meant.
            (
                "https://docs.rs/tokio/latest/tokio/struct.Foo.html#method.spawn-1",
                "method.spawn-1",
            ),
            // A field names an item, but not one `resolve_path` can reach, so the
            // path stops at the struct that owns it.
            (
                "https://docs.rs/tokio/latest/tokio/struct.Foo.html#structfield.inner",
                "structfield.inner",
            ),
        ] {
            let link = DocsRsLink::parse(url).unwrap();
            assert_eq!(link.to_string(), "tokio::Foo");
            assert_eq!(link.kind, ItemKind::Struct);
            assert_eq!(link.fragment, Some(fragment));
        }
    }

    #[test]
    fn rust_lang_std_pages() {
        for url in [
            "https://doc.rust-lang.org/std/vec/struct.Vec.html",
            "https://doc.rust-lang.org/nightly/std/vec/struct.Vec.html",
            "https://doc.rust-lang.org/stable/std/vec/struct.Vec.html",
            "https://doc.rust-lang.org/1.75.0/std/vec/struct.Vec.html",
            // The legacy host that redirects to `doc.`. We no longer emit it, but
            // documentation in the wild still links to it.
            "http://docs.rust-lang.org/nightly/std/vec/struct.Vec.html",
        ] {
            assert_link(url, "std::vec::Vec", ItemKind::Struct);
        }

        assert_link("https://doc.rust-lang.org/std/", "std", ItemKind::Module);
        assert_link(
            "https://doc.rust-lang.org/core/primitive.usize.html",
            "core::usize",
            ItemKind::Primitive,
        );
        assert_link(
            "https://doc.rust-lang.org/nightly/alloc/vec/struct.Vec.html",
            "alloc::vec::Vec",
            ItemKind::Struct,
        );
    }

    /// A query string is dropped, and does not swallow the fragment after it.
    #[test]
    fn query_strings_are_dropped() {
        assert_link(
            "https://docs.rs/tokio/latest/tokio/index.html?search=spawn",
            "tokio",
            ItemKind::Module,
        );
        let link =
            DocsRsLink::parse("https://docs.rs/tokio/latest/tokio/struct.Foo.html?a=b#method.bar")
                .unwrap();
        assert_eq!(link.to_string(), "tokio::Foo::bar");
    }

    /// Relative links resolve against the *directory* of the page they appear on, so
    /// an item's own page name is discarded first.
    #[test]
    fn relative_links_resolve_against_the_page_directory() {
        let module = "https://docs.rs/tokio/1.52.3/tokio/runtime/index.html";
        let method =
            "https://docs.rs/tokio/1.52.3/tokio/runtime/struct.Runtime.html#method.block_on";
        let root = "https://docs.rs/tokio/1.52.3/tokio/index.html";

        // Both a module and a method inside it sit in `.../tokio/runtime/`.
        for page in [module, method] {
            assert_eq!(
                resolve_relative(page, "../attr.main.html").as_deref(),
                Some("https://docs.rs/tokio/1.52.3/tokio/attr.main.html"),
            );
            assert_eq!(
                resolve_relative(page, "index.html").as_deref(),
                Some("https://docs.rs/tokio/1.52.3/tokio/runtime/index.html"),
            );
        }

        assert_eq!(
            resolve_relative(root, "attr.main.html").as_deref(),
            Some("https://docs.rs/tokio/1.52.3/tokio/attr.main.html"),
        );
        assert_eq!(
            resolve_relative(root, "./task/index.html").as_deref(),
            Some("https://docs.rs/tokio/1.52.3/tokio/task/index.html"),
        );

        // A fragment on the link rides along for `parse` to interpret; one on the page
        // is dropped, being no part of its directory.
        assert_eq!(
            resolve_relative(module, "../index.html#cpu-bound-tasks").as_deref(),
            Some("https://docs.rs/tokio/1.52.3/tokio/index.html#cpu-bound-tasks"),
        );

        // A URL already naming a directory has an empty final segment.
        assert_eq!(
            resolve_relative("https://docs.rs/tokio/1.52.3/tokio/", "attr.main.html").as_deref(),
            Some("https://docs.rs/tokio/1.52.3/tokio/attr.main.html"),
        );
    }

    #[test]
    fn relative_links_that_do_not_resolve() {
        let page = "https://docs.rs/tokio/1.52.3/tokio/runtime/index.html";
        // Absolute, so not ours to join.
        assert_eq!(resolve_relative(page, "/tokio/index.html"), None);
        assert_eq!(resolve_relative(page, "https://example.com/x.html"), None);
        // Walks above the host.
        assert_eq!(resolve_relative(page, "../../../../../../x.html"), None);
    }

    /// The end-to-end shape of the `tokio::runtime` regression: `[`tokio::main`]:
    /// ../attr.main.html` must reach the `main` attribute macro, not `tokio::./attr.main`.
    #[test]
    fn relative_link_round_trips_through_parse() {
        let page = "https://docs.rs/tokio/1.52.3/tokio/runtime/index.html";
        let absolute = resolve_relative(page, "../attr.main.html").unwrap();
        let link = DocsRsLink::parse(&absolute).unwrap();
        assert_eq!(link.to_string(), "tokio@1.52.3::main");
        assert_eq!(link.kind, ItemKind::ProcAttribute);
    }

    #[test]
    fn non_item_urls() {
        for url in [
            // Not a documentation host at all.
            "https://github.com/tokio-rs/tokio",
            "https://example.com/docs.rs/tokio/latest/tokio/",
            // Relative and fragment-only links never reach us, but must not panic.
            "task/index.html",
            "#method.spawn",
            // docs.rs site routes rather than crate documentation.
            "https://docs.rs/crate/tokio/latest",
            "https://docs.rs/crate/tokio/1.40.0/source/src/lib.rs",
            "https://docs.rs/releases",
            "https://docs.rs/about/badges",
            // Addresses no single item.
            "https://docs.rs/tokio/latest/tokio/all.html",
            // Second segment is not a version, so this was never an item URL.
            "https://docs.rs/tokio/not-a-version/tokio/struct.Foo.html",
            // A sigil rustdoc never emits.
            "https://docs.rs/tokio/latest/tokio/widget.Foo.html",
            // doc.rust-lang.org prose, and the compiler's own docs.
            "https://doc.rust-lang.org/book/ch01-00-getting-started.html",
            "https://doc.rust-lang.org/nightly/nomicon/index.html",
            "https://doc.rust-lang.org/nightly/nightly-rustc/rustc_middle/index.html",
            "https://doc.rust-lang.org/cargo/reference/manifest.html",
        ] {
            assert_unparsed(url);
        }
    }
}
