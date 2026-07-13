//! Which request paths the SPA index is allowed to answer.
//!
//! Without this gate the frontend's SPA fallback serves `index.html`, with a
//! `200`, for *every* path no asset matched — including `/.env`,
//! `/wp-login.php` and `/vendor/phpunit/.../eval-stdin.php`. That is wrong on
//! its own terms (those are not client-side routes) and it also denies the host
//! any signal to act on: a log in which every probe succeeds gives fail2ban
//! nothing to count.
//!
//! The app's routes are exactly `/` and `/{rust::item::path}` — a single
//! segment. So a path that can't be a Rust item path can't be a route, and
//! 404ing it is both more honest and more useful.

use percent_encoding::percent_decode_str;

/// Whether `path` could name a Rust item, and so may be answered with the SPA
/// index.
///
/// Accepts the root and one segment of `::`-separated identifiers: a crate name
/// (which may contain `-`) followed by path segments (which may not). Rust item
/// paths contain no `/` and no `.`, which is what excludes essentially all
/// probe traffic.
///
/// The path is percent-decoded first, so an encoded `/` or `.` can't smuggle
/// itself past the check.
pub(super) fn is_app_route(path: &str) -> bool {
    let path = percent_decode_str(path).decode_utf8_lossy();
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
