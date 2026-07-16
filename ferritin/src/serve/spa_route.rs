//! Which request paths the SPA index is allowed to answer.
//!
//! Without this gate the frontend's SPA fallback serves `index.html`, with a
//! `200`, for *every* path no asset matched — including `/.env`,
//! `/wp-login.php` and `/vendor/phpunit/.../eval-stdin.php`. That is wrong on
//! its own terms (those are not client-side routes) and it also denies the host
//! any signal to act on: a log in which every probe succeeds gives fail2ban
//! nothing to count.
//!
//! The app's routes are exactly `/`, `/{rust::item::path}`, and the handful of
//! `/~name` pages — all of them a single segment, with no `/` and no `.`. So a
//! path that is none of those can't be a route, and 404ing it is both more
//! honest and more useful.

use percent_encoding::percent_decode_str;

/// The pages that aren't item paths.
///
/// A crate name is `[A-Za-z0-9_-]+`, so a leading `~` can never collide with one
/// — that is the whole reason the pages are named this way, and why this list can
/// grow without ever shadowing a crate.
///
/// Exact matches, not a `/~` prefix: this gate's job is to name what exists. A
/// prefix match would answer `/~anything` with the index and hand scanners a
/// path that always 200s.
///
/// Keep in sync with the client's routes (`client/src/App.tsx`,
/// `client/src/lib/paths.ts`).
const PAGES: &[&str] = &["/~install"];

/// Whether `path` could name a Rust item or a known page, and so may be answered
/// with the SPA index.
///
/// Accepts the root, the pages in [`PAGES`], and one segment of `::`-separated
/// identifiers: a crate name (which may contain `-`) followed by path segments
/// (which may not). Rust item paths contain no `/` and no `.`, which is what
/// excludes essentially all probe traffic — and the pages hold to that same
/// shape, so nothing here weakens it.
///
/// The path is percent-decoded first, so an encoded `/`, `.` or `~` can't
/// smuggle itself past the check.
pub(super) fn is_app_route(path: &str) -> bool {
    let path = percent_decode_str(path).decode_utf8_lossy();

    if PAGES.contains(&path.as_ref()) {
        return true;
    }

    let Some(path) = path.strip_prefix('/') else {
        return false;
    };

    if path.is_empty() {
        return true;
    }

    let mut segments = path.split("::");

    // The crate name, which unlike the rest of the path may contain `-`.
    let Some(crate_name) = segments.next() else {
        return false;
    };
    if crate_name.is_empty()
        || !crate_name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return false;
    }

    segments.all(|segment| {
        !segment.is_empty()
            && segment
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

#[cfg(test)]
mod tests {
    use super::is_app_route;

    #[test]
    fn accepts_the_app_s_own_routes() {
        assert!(is_app_route("/"));
        assert!(is_app_route("/serde"));
        assert!(is_app_route("/serde::Deserialize"));
        assert!(is_app_route("/trillium::conn::Conn"));
        assert!(is_app_route("/trillium-ratelimit"));
        assert!(is_app_route("/base64::engine::general_purpose::STANDARD"));
        assert!(is_app_route("/std::vec::Vec"));
    }

    #[test]
    fn accepts_the_pages() {
        assert!(is_app_route("/~install"));
        // The decode happens before the check, so the encoded form is the same
        // route rather than a second, unlisted one.
        assert!(is_app_route("/%7Einstall"));
    }

    /// The `~` names the pages that exist; it does not open a namespace that
    /// always answers. A crate can never live here, but neither can anything we
    /// haven't listed.
    #[test]
    fn rejects_unlisted_pages() {
        assert!(!is_app_route("/~"));
        assert!(!is_app_route("/~nope"));
        assert!(!is_app_route("/%7Enope"));
        // Not a directory: the nested spelling is not the page, and never was.
        assert!(!is_app_route("/~/install"));
        assert!(!is_app_route("/~install/extra"));
    }

    /// The pages are one segment like every other route, so traversal is
    /// rejected by the item-path grammar itself — there is no `/` for it to
    /// hide behind.
    #[test]
    fn rejects_traversal_dressed_as_a_page() {
        assert!(!is_app_route("/~install/../../.env"));
        assert!(!is_app_route("/~/../.env"));
        assert!(!is_app_route("/%7Einstall%2F..%2F.env"));
    }

    #[test]
    fn rejects_probe_traffic() {
        assert!(!is_app_route("/.env"));
        assert!(!is_app_route("/wp-login.php"));
        assert!(!is_app_route(
            "/vendor/phpunit/phpunit/src/Util/PHP/eval-stdin.php"
        ));
        assert!(!is_app_route("/cgi-bin/bin/sh"));
        assert!(!is_app_route("/.git/config"));
        assert!(!is_app_route("/hello.world"));
    }

    #[test]
    fn rejects_encoded_separators() {
        // The probe in the logs used `%%32%65` to smuggle `.` past naive
        // filters; decoding before the check is what closes that.
        assert!(!is_app_route("/cgi-bin%2Fbin%2Fsh"));
        assert!(!is_app_route("/%2Eenv"));
    }

    #[test]
    fn rejects_malformed_item_paths() {
        assert!(!is_app_route("/serde::"));
        assert!(!is_app_route("/::Deserialize"));
        assert!(!is_app_route("/serde::my-item"));
        // Not a path at all — the router hands us an absolute path.
        assert!(!is_app_route("serde"));
    }
}
