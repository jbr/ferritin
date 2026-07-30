//! What the SPA index says about itself, and what status it carries.
//!
//! The client renders every item page, so the HTML the server sends is one
//! fixed shell — which means every shared link unfurls identically ("ferritin",
//! no description) and every path that names nothing is answered `200`. Both are
//! the same omission: the server knows something about the requested path and
//! declines to say it.
//!
//! Crawlers are the reason this can't be left to the client. Slack, Discord,
//! iMessage, Mastodon and the search engines fetch a URL once and parse the
//! HTML; none of them run JavaScript, so anything React fills in afterward is
//! invisible to them. The `<meta>` tags have to be in the bytes we send.
//!
//! ## What this is allowed to cost
//!
//! Nothing. Every answer here comes from the crate-names artifact, which is
//! already resident and refreshed out of band by a detached task — so this is a
//! binary search over memory, not a crate load, and emphatically not a docs.rs
//! download. That bound is deliberate rather than incidental: the index is on
//! the critical path to first byte, and it is reachable by anyone, so making it
//! able to trigger a fetch would hand every scanner a way to make us do real
//! work. A crate's own documentation is loaded when the client asks for it over
//! `/api`, exactly as before.
//!
//! The cost of that bound is that the metadata is crate-level: `serde::Serialize`
//! and `serde::Deserialize` describe themselves with serde's description. Item
//! detail needs the crate loaded, which is a separate decision from this one.
//!
//! ## The 404
//!
//! A path whose crate segment names no crate is answered `404` — with the index
//! as its body, so the client still boots and renders its own not-found page.
//! This is worth more than tidiness: it stops us telling search engines that
//! every typo is a page, and it gives fail2ban a signal to count, since scanner
//! traffic is precisely the traffic that names crates which do not exist.
//!
//! Absence has to be *proven* to be acted on, which is why [`CrateLookup`]
//! separates "the artifact says no" from "there is no artifact". A server whose
//! artifact never loaded answers `200` for everything, as it did before.

use crate::{
    crate_search::{CrateLookup, CrateSearchService},
    serve::{caching, spa_route},
};
use ferritin_common::CratePath;
use percent_encoding::percent_decode_str;
use std::{borrow::Cow, sync::Arc};
use trillium::{Conn, Handler, KnownHeaderName, Status};
use trillium_caching_headers::CachingHeadersExt;
use trillium_html_rewriter::{
    HtmlRewriter, Settings,
    html::{element, html_content::ContentType},
};

/// The `og:site_name`, and the title of the pages that aren't a crate.
pub(super) const SITE_NAME: &str = "ferritin";

/// The description for the root, the `~` pages, and any crate the artifact
/// knows but has no description for.
pub(super) const SITE_DESCRIPTION: &str = "Rust documentation for humans and agents.";

/// The longest description we will emit. Crate descriptions are short by
/// convention, but nothing enforces that, and every crawler truncates somewhere
/// — better to cut on a character boundary ourselves than to be cut mid-entity.
const MAX_DESCRIPTION: usize = 300;

/// What the index should say about this request, resolved during `run` and read
/// back in `before_send`.
///
/// Absent from the conn entirely when the path is not an app route, which is
/// what keeps this off every asset and API response.
#[derive(Debug, Clone, Hash)]
struct PageMeta {
    title: Cow<'static, str>,
    description: Cow<'static, str>,
    /// The absolute URL of this page, when the request said what host it was
    /// for. `og:url` must be absolute, so it is omitted rather than guessed.
    url: Option<String>,
    /// The absolute URL of this page's card image, served by [`super::og`].
    /// Absolute for the same reason as `url`, and absent under the same
    /// condition.
    image: Option<String>,
    /// Whether the path named something we can serve. `false` is the only thing
    /// that produces a `404`, and only [`CrateLookup::Unknown`] produces it.
    found: bool,
}

impl PageMeta {
    /// The tags, as an HTML fragment for the head.
    ///
    /// Everything interpolated here is escaped: descriptions come from
    /// crates.io and the title contains a caller-supplied path, so both are
    /// untrusted text being written into attribute values.
    fn tags(&self) -> String {
        let title = escape(&self.title);
        let description = escape(&self.description);

        // `summary_large_image` is a claim about the image, so it is only made
        // when there is one to fetch.
        let twitter_card = if self.image.is_some() {
            "summary_large_image"
        } else {
            "summary"
        };

        let mut tags = format!(
            r#"<meta name="description" content="{description}">
<meta property="og:site_name" content="{SITE_NAME}">
<meta property="og:type" content="website">
<meta property="og:title" content="{title}">
<meta property="og:description" content="{description}">
<meta name="twitter:card" content="{twitter_card}">
"#
        );

        if let Some(url) = &self.url {
            tags.push_str(&format!(
                r#"<meta property="og:url" content="{}">
"#,
                escape(url)
            ));
        }

        if let Some(image) = &self.image {
            tags.push_str(&format!(
                r#"<meta property="og:image" content="{image}">
<meta property="og:image:width" content="{width}">
<meta property="og:image:height" content="{height}">
<meta property="og:image:type" content="image/png">
<meta property="og:image:alt" content="{title}">
"#,
                image = escape(image),
                width = super::og::WIDTH,
                height = super::og::HEIGHT,
            ));
        }

        tags
    }
}

/// Resolve the index's metadata and status for app routes.
///
/// Split across the two phases because neither alone can do it: the path is only
/// intact during `run` (the router consumes prefixes), while the status can only
/// be set in `before_send`, after `frontend()` has served the index with its
/// `200`.
pub(super) struct AppPage;

impl Handler for AppPage {
    async fn run(&self, conn: Conn) -> Conn {
        if !spa_route::is_app_route(conn.path()) {
            return conn;
        }

        let Some(service) = conn.state::<Arc<CrateSearchService>>().cloned() else {
            return conn;
        };

        let meta = resolve(&conn, &service).await;
        conn.with_state(Arc::new(meta))
    }

    async fn before_send(&self, conn: Conn) -> Conn {
        let Some(meta) = conn.state::<Arc<PageMeta>>().cloned() else {
            return conn;
        };

        // Re-tag before `CachingHeaders` compares anything: the frontend's etag
        // names the file, which is identical for every route. See
        // [`caching::app_page`].
        let file_etag = conn.response_headers().get_str(KnownHeaderName::Etag);
        let etag = caching::app_page(file_etag, &meta);

        let mut conn = conn.with_etag(&etag);

        // The file's mtime is the same for every route too, so it would let an
        // `If-Modified-Since` win a `304` that the etag correctly denied.
        conn.response_headers_mut()
            .remove(KnownHeaderName::LastModified);

        // The body is left alone: the client boots from the same index and
        // renders its own not-found page, so this is a 404 that still works.
        if meta.found {
            conn
        } else {
            conn.with_status(Status::NotFound)
        }
    }
}

/// The rewriter that writes [`PageMeta`] into the head.
///
/// Mount this **last** in the handler tuple. `before_send` runs in reverse tuple
/// order, so the last handler's runs first — which is what puts the rewrite
/// ahead of `CachingHeaders` (so the etag describes what we actually sent) and
/// ahead of `Compression` (so the rewritten stream is what gets compressed).
pub(super) fn rewriter() -> HtmlRewriter {
    HtmlRewriter::new_with_conn(|conn| {
        let Some(meta) = conn.state::<Arc<PageMeta>>().cloned() else {
            // Not an app route, so there is nothing to say about it. The
            // rewriter still runs, and passes the body through untouched.
            return Settings::new_send();
        };

        let title = escape(&meta.title);

        Settings::new_send()
            .append_element_content_handler(element!("head", move |el| {
                el.prepend(&meta.tags(), ContentType::Html);
                Ok(())
            }))
            // The shell's `<title>` is the static site name; a crawler that
            // ignores og tags (and the browser tab, before React boots) should
            // still see the page it asked for.
            .append_element_content_handler(element!("title", move |el| {
                el.set_inner_content(&title, ContentType::Html);
                Ok(())
            }))
    })
}

/// What the server can say about one app path, from resident data alone.
///
/// The single resolution behind both surfaces a shared link presents: [`resolve`]
/// dresses it as the head's meta tags, and [`super::og`] draws it as the card
/// image those tags point at. One origin is what keeps the two claims from ever
/// disagreeing.
pub(super) enum PageContent {
    /// The root and the `~` pages: the site describing itself.
    Site,
    /// A crate the artifact knows, described by [`describe`].
    Known {
        item_path: String,
        /// The crate's name as the artifact spells it — without any `@` version
        /// requirement the path carried.
        crate_name: String,
        description: String,
    },
    /// The artifact is loaded and the path's crate segment is not in it.
    NotFound,
    /// No artifact loaded, so absence proves nothing; echo the path.
    Indeterminate { item_path: String },
}

/// Resolve one decoded app path (no leading `/`) against the resident artifact.
pub(super) async fn content(service: &CrateSearchService, item_path: &str) -> PageContent {
    // The root and the `~` pages name no crate, so there is nothing to look up
    // and nothing that could be missing.
    if item_path.is_empty() || item_path.starts_with('~') {
        return PageContent::Site;
    }

    // Parsed with the same type the resolver uses, so the crate we ask about is
    // the crate a later `/api` request would load.
    let CratePath { name, .. } = CratePath::parse(item_path);

    match service.lookup(name).await {
        CrateLookup::Known { name, description } => PageContent::Known {
            item_path: item_path.to_string(),
            description: describe(&name, description.as_deref()),
            crate_name: name,
        },
        CrateLookup::Unknown => PageContent::NotFound,
        CrateLookup::Indeterminate => PageContent::Indeterminate {
            item_path: item_path.to_string(),
        },
    }
}

/// Derive the metadata for one app route from resident data alone.
async fn resolve(conn: &Conn, service: &CrateSearchService) -> PageMeta {
    let url = absolute_url(conn);
    let image = image_url(conn);
    let path = percent_decode_str(conn.path()).decode_utf8_lossy();
    let item_path = path.trim_start_matches('/');

    match content(service, item_path).await {
        PageContent::Site => PageMeta {
            title: Cow::Borrowed(SITE_NAME),
            description: Cow::Borrowed(SITE_DESCRIPTION),
            url,
            image,
            found: true,
        },

        PageContent::Known {
            item_path,
            crate_name: _,
            description,
        } => PageMeta {
            title: format!("{item_path} on ferritin.rs").into(),
            description: description.into(),
            url,
            image,
            found: true,
        },

        PageContent::NotFound => PageMeta {
            title: Cow::Borrowed("Not Found on ferritin.rs"),
            description: Cow::Borrowed(SITE_DESCRIPTION),
            url,
            image,
            found: false,
        },

        // No artifact loaded: we cannot say this crate is missing, so we don't.
        PageContent::Indeterminate { item_path } => PageMeta {
            title: item_path.into(),
            description: Cow::Borrowed(SITE_DESCRIPTION),
            url,
            image,
            found: true,
        },
    }
}

/// The description for a crate we know: what crates.io says about it, and
/// nothing else.
///
/// No version, deliberately — see [`CrateLookup::Known`]. Crates with no
/// description on crates.io get a bare statement of what the page is, which is
/// still more than the shell's static text.
fn describe(name: &str, description: Option<&str>) -> String {
    match description {
        Some(description) => truncate(description),
        None => format!("Rust documentation for the {name} crate."),
    }
}

/// Cut to [`MAX_DESCRIPTION`] characters on a character boundary, with an
/// ellipsis when anything was dropped.
fn truncate(value: &str) -> String {
    match value.char_indices().nth(MAX_DESCRIPTION) {
        Some((index, _)) => format!("{}…", &value[..index]),
        None => value.to_string(),
    }
}

/// The absolute URL of this request, or `None` when it named no host.
///
/// `og:url` and `og:image` are required to be absolute, and the only authority
/// we can honestly report is the one the client asked for.
fn absolute_url(conn: &Conn) -> Option<String> {
    let host = conn.host()?;
    let scheme = if conn.is_secure() { "https" } else { "http" };
    Some(format!("{scheme}://{host}{}", conn.path()))
}

/// The absolute URL of this page's card image: the page's own path under
/// [`super::og`]'s prefix, so the image is derived from the URL by prefixing
/// alone. `None` without a host, exactly like [`absolute_url`].
fn image_url(conn: &Conn) -> Option<String> {
    let host = conn.host()?;
    let scheme = if conn.is_secure() { "https" } else { "http" };
    Some(format!(
        "{scheme}://{host}{}{}",
        super::og::PREFIX,
        conn.path()
    ))
}

/// Escape text for interpolation into an HTML (or SVG) attribute value.
///
/// The tags are inserted as [`ContentType::Html`] — they have to be, or they
/// would be text rather than markup — so nothing escapes them for us. Also used
/// by [`super::og`] for text interpolated into the card's SVG, where the same
/// five entities are the reserved characters.
pub(super) fn escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::{MAX_DESCRIPTION, escape, truncate};

    #[test]
    fn escapes_attribute_syntax() {
        assert_eq!(escape(r#"a & b"#), "a &amp; b");
        assert_eq!(escape(r#""><script>"#), "&quot;&gt;&lt;script&gt;");
        assert_eq!(escape("plain"), "plain");
    }

    /// Crate descriptions are arbitrary text from crates.io, so the cut has to
    /// land on a character boundary rather than a byte one.
    #[test]
    fn truncates_on_character_boundaries() {
        let multibyte = "é".repeat(MAX_DESCRIPTION + 10);
        let truncated = truncate(&multibyte);
        assert!(truncated.ends_with('…'));
        assert_eq!(truncated.chars().count(), MAX_DESCRIPTION + 1);

        let short = "serde 1.0.0 — a serialization framework";
        assert_eq!(truncate(short), short);
    }
}
