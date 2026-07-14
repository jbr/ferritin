//! `robots.txt` and `sitemap.xml`.
//!
//! These are served rather than shipped as static assets in the client's `dist`
//! because a sitemap's `<loc>` must be an absolute URL, and the origin is not
//! known when the client is built: the same binary serves the public deployment,
//! a staging domain, and `ferritin serve` on someone's laptop. Deriving the
//! origin from the request is what lets one build be correct on all of them.
//!
//! They also keep well-behaved crawlers out of the scanner jail. Both paths
//! contain a `.`, so [`is_app_route`](super::spa_route::is_app_route) rejects
//! them and they would otherwise 404 — and a 404 outside `/api` is exactly what
//! fail2ban counts as a probe. A crawler that politely asks for `robots.txt`
//! should not accrue strikes for doing so.

use trillium::{Conn, KnownHeaderName, Status};

/// The origin to advertise: the authority the request arrived on, with a scheme
/// matching its transport. Falls back to the ACME domain over https when a
/// request carries no `Host` (HTTP/1.0, or a bare-IP client), since that is the
/// canonical name a crawler should be told about.
fn origin(conn: &Conn) -> Option<String> {
    let scheme = if conn.is_secure() { "https" } else { "http" };
    match conn.host() {
        Some(host) => Some(format!("{scheme}://{host}")),
        None => std::env::var("FERRITIN_ACME_DOMAIN")
            .ok()?
            .split(',')
            .next()
            .filter(|domain| !domain.is_empty())
            .map(|domain| format!("https://{domain}")),
    }
}

/// Ask crawlers for the root and nothing else.
///
/// This is not an SEO position, it is a load one. An item page is a live lookup
/// that may pull and parse a large rustdoc JSON from docs.rs, so a crawler
/// walking the crate graph costs far more than crawling an ordinary static site
/// of the same page count — and what it would index is a rendering of docs.rs,
/// which is already indexed.
///
/// `Allow: /$` relies on the `$` anchor, which RFC 9309 leaves optional. A
/// crawler that does not implement it reads the `Allow` as a literal path,
/// matches nothing, and falls back to `Disallow: /` — declining to crawl at all.
/// Failing closed is the right direction here.
pub(super) async fn robots(conn: Conn) -> Conn {
    let mut body = String::from("User-agent: *\nAllow: /$\nDisallow: /\n");

    if let Some(origin) = origin(&conn) {
        body.push_str(&format!("\nSitemap: {origin}/sitemap.xml\n"));
    }

    conn.with_status(Status::Ok)
        .with_response_header(KnownHeaderName::ContentType, "text/plain; charset=utf-8")
        .with_body(body)
        .halt()
}

/// A sitemap listing the root, and only the root — the one page worth crawling,
/// for the reasons in [`robots`].
pub(super) async fn sitemap(conn: Conn) -> Conn {
    let Some(origin) = origin(&conn) else {
        // No Host header and no configured domain: there is no absolute URL to
        // put in a <loc>, and a sitemap without one is malformed.
        return conn.with_status(Status::NotFound).halt();
    };

    let body = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n\
         \x20 <url><loc>{origin}/</loc></url>\n\
         </urlset>\n"
    );

    conn.with_status(Status::Ok)
        .with_response_header(KnownHeaderName::ContentType, "application/xml")
        .with_body(body)
        .halt()
}
