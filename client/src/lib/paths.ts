/**
 * Item-path helpers. Item paths are `::`-joined (`trillium::Conn`) and map 1:1 to
 * app routes: the router's `/*path` catch-all captures the whole path as one
 * segment (since `::` is not `/`). The API layer percent-encodes on the way out,
 * so hrefs stay human-readable here.
 */

/**
 * The sigil marking a route as a page rather than an item path.
 *
 * A crate name is `[A-Za-z0-9_-]+` and the rest of an item path is Rust
 * identifiers, so `~` appears in neither — which makes `/~name` collision-proof
 * against every crate that exists or ever will. A bare reserved word like
 * `/install` would not be: `install` is a legal crate name, so we'd shadow a
 * real crate and need a reserved list that grows with every page we add.
 *
 * `~` in particular because it costs nothing: it's in RFC 3986's *unreserved*
 * set, so it survives every proxy and URL-normalizer untouched, and `/~name`
 * reads as a home page in the oldest sense on the web.
 *
 * **It is a prefix on one segment, not a directory.** `/~install`, not
 * `/~/install`. Every route this app has is exactly one path segment, and that
 * invariant is load-bearing twice over: relative URLs in `index.html` resolve
 * against the root from any route (so an asset href — or a change to vite's
 * `base` — can't silently 404 on a deep page), and the ops filter that bans
 * scanners reasons from "a route is one segment, no `/` and no `.`" to decide
 * what's a probe (`ferritin-ops/fail2ban/filter-ferritin-probe.conf`). A nested
 * page namespace would break both for no benefit we need: nothing wants a
 * hierarchy here.
 *
 * The server enforces the same carve-out, so these paths get the SPA while probe
 * traffic still 404s: see `ferritin/src/serve/spa_route.rs`.
 */
export const RESERVED_SIGIL = "~";

/** Whether a route path names a page rather than an item. */
export function isReservedPath(path: string): boolean {
  return path.startsWith(`/${RESERVED_SIGIL}`);
}

/** The app route for an item path (e.g. `trillium::Conn` → `/trillium::Conn`). */
export function itemHref(path: string): string {
  return `/${path}`;
}

/** The crate segment of an item path (`trillium::conn::Conn` → `trillium`). */
export function crateOf(path: string): string {
  return path.split("::", 1)[0] ?? path;
}

/** Join a crate to a bare child name into a full path (skips redundant crate). */
export function childPath(crate: string, path: string): string {
  return path.startsWith(`${crate}::`) || path === crate
    ? path
    : `${crate}::${path}`;
}

/** Lowercase, hyphenate a label into a DOM id fragment. */
export function slug(text: string): string {
  return text
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}
