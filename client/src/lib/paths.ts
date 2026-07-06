/**
 * Item-path helpers. Item paths are `::`-joined (`trillium::Conn`) and map 1:1 to
 * app routes: the router's `/*path` catch-all captures the whole path as one
 * segment (since `::` is not `/`). The API layer percent-encodes on the way out,
 * so hrefs stay human-readable here.
 */

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
