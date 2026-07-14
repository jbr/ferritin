/**
 * Path arithmetic for the crate-nav tree.
 *
 * The nav renders one *level* per module along the **spine** — the chain of
 * modules from the crate root down to the module the reader is currently in.
 * Everything else in the tree hangs off that chain, collapsed until asked for.
 */

/** The module path containing `path`, or undefined for a bare crate name. */
export function parentPath(path: string): string | undefined {
  const i = path.lastIndexOf("::");
  return i === -1 ? undefined : path.slice(0, i);
}

/** The last segment of a path — what a tree row displays. */
export function lastSegment(path: string): string {
  return path.split("::").pop() ?? path;
}

/**
 * Every prefix of `scope`, crate root first: `a::b::c` → `[a, a::b, a::b::c]`.
 *
 * A level whose spine child turns out not to be one of its modules simply stops
 * recursing (see `CrateNav`), so a path through a non-module — an enum variant,
 * say — degrades to "show me the deepest module we did reach" rather than
 * rendering an empty tree.
 */
export function spineOf(scope: string): string[] {
  const segments = scope.split("::");
  return segments.map((_, i) => segments.slice(0, i + 1).join("::"));
}

/** `n` with a plural `s`, for the "12 modules, 8 items" expander. */
export function plural(n: number, noun: string): string {
  return `${n} ${noun}${n === 1 ? "" : "s"}`;
}
