import { useEffect, useRef, useState } from "react";
import { Link } from "rhoto-router";
import { useItem } from "../../api/queries";
import type { ModuleItem } from "../../api/types";
import { crateOf, itemHref } from "../../lib/paths";
import { lastSegment, parentPath, plural, spineOf } from "../../lib/nav";
import { moduleGroups } from "../../lib/toc";

/** Kind groups larger than this start collapsed, in a level that shows its items. */
const COLLAPSE_THRESHOLD = 15;

/**
 * An ancestor with at most this many submodules shows all of them by default.
 *
 * The threshold is on *modules*, not on total children, because a level's
 * non-module children collapse into a handful of kind-group headers while its
 * modules each cost a row. It is what makes the tree self-tuning across crate
 * shapes: `trillium` (3) and `tokio` (11) open their roots and read much as the
 * old flat nav did, while `std` (78) shows you only where you are.
 */
const ANCESTOR_MODULE_THRESHOLD = 15;

/**
 * Left sidebar: a lazy module tree, opened along the path to the current item.
 *
 * Modules are *branches* and everything else is a *leaf* — modules are structure
 * you descend, types are content you read — so submodules render as bare
 * expandable rows while the rest of a module's children keep the kind grouping
 * (Structs, Enums, …) the terminal uses.
 *
 * Three rules decide what is open by default, and nothing else does:
 *
 * 1. The **spine** — the modules from the crate root down to the one you are in —
 *    is open. The deepest of them (the *scope*) shows all its children.
 * 2. The scope's kind group containing the current item is open in full. Modules
 *    top out around 30 children of any one kind, so there is no truncation to
 *    design: you see all your siblings.
 * 3. Everything else collapses behind a counted expander — `⋯ 10 modules, 8
 *    items` on an ancestor, a twisty on an off-spine module — so crate-wide
 *    navigation stays one click away without costing 78 rows of `std` up front.
 *
 * All three are defaults, not decrees: any module's twisty overrides the rule that
 * would otherwise govern it, in either direction (see `wish`, below).
 *
 * Levels are fetched as they open (`GET /api/crates/{module}`), so arriving at
 * `tokio::sync::Mutex` costs one request per ancestor — and none at all for the
 * ones you clicked through to get there, which are already in the query cache.
 */
export function CrateNav({ path }: { path?: string }) {
  // What the reader has explicitly asked of a module, overriding what the spine
  // would otherwise decide: `true` is "show me everything in here", `false` is
  // "show me nothing in here". The twisty and the ⋯ expander are two ways to set
  // the same value, which is why they are not two mechanisms.
  const [wish, setWish] = useState<Map<string, boolean>>(new Map());
  // Kind groups flipped away from their default state, keyed `${module}#${group}`.
  const [flipped, setFlipped] = useState<Set<string>>(new Set());

  // An expansion is a lasting preference about the crate, so it survives
  // navigation. A collapse is a remark about the tree currently in front of you —
  // and if one survived, you could navigate *into* a module the nav has been told
  // to hide and land with no visible highlight at all. So collapses are dropped
  // on arrival. (The prop-change reset, done during render: the stale wish never
  // reaches the DOM, and there is no second render pass.)
  const [arrivedAt, setArrivedAt] = useState(path);
  if (arrivedAt !== path) {
    setArrivedAt(path);
    if ([...wish.values()].some((open) => !open)) {
      setWish(new Map([...wish].filter(([, open]) => open)));
    }
  }

  const { data } = useItem(path ?? "");

  if (!path) return <nav className="crate-nav" />;

  // The scope is the item itself when it is a module, and its parent otherwise.
  // Until the item loads we assume the latter — much the commoner case — and a
  // module page simply gains its own level when the fetch lands.
  const isModule = data?.body.kind === "module";
  const scope = (isModule ? path : parentPath(path)) ?? path;
  const spine = spineOf(scope);

  const nav: NavState = {
    wish,
    flipped,
    active: isModule ? undefined : path,
    setWish: (key, open) => setWish((prev) => new Map(prev).set(key, open)),
    toggleGroup: (key) => setFlipped((prev) => toggled(prev, key)),
  };

  return (
    // Hover cards open to the *right* of a nav row rather than below it: this is a
    // column of rows, and a card thrown downward would cover the very list the
    // reader is scanning. See `axisOf` in `useHoverTarget`.
    <nav
      className="crate-nav"
      aria-label={`${crateOf(path)} contents`}
      data-preview-axis="inline"
    >
      <ul className="nav-tree">
        <ModuleBranch
          path={spine[0]}
          spine={spine.slice(1)}
          onSpine
          depth={0}
          nav={nav}
        />
      </ul>
    </nav>
  );
}

type NavState = {
  /** Explicit per-module wishes; absent means "let the spine decide". */
  wish: Map<string, boolean>;
  flipped: Set<string>;
  /** The current item, when it is not itself a module. */
  active?: string;
  setWish: (path: string, open: boolean) => void;
  toggleGroup: (key: string) => void;
};

function toggled(set: Set<string>, key: string): Set<string> {
  const next = new Set(set);
  if (!next.delete(key)) next.add(key);
  return next;
}

/**
 * Sort a level's children by name — for the *nav only*.
 *
 * rustdoc emits children in declaration order, which is what the main column and
 * the page TOC show, and there it is arguably meaningful. A navigator is not read,
 * it is scanned for a name you already have in mind, and `std`'s 77 root modules
 * in declaration order (`prelude, f128, f16, f32, f64, thread, ascii, …`) are not
 * scannable at all. Sorting here leaves `moduleGroups`, and so both other
 * consumers, untouched.
 */
function byName<T extends { path: string }>(items: T[]): T[] {
  return [...items].sort((a, b) => a.path.localeCompare(b.path));
}

/**
 * One module row, plus its children when open.
 *
 * The spine is open by default but not by decree: you can shut the module you are
 * standing in — a long one buries the rest of the crate, and wanting it out of the
 * way is not a strange thing to want. Navigating back into it reopens it.
 */
function ModuleBranch({
  path,
  spine,
  onSpine,
  depth,
  nav,
}: {
  path: string;
  /** Remaining spine below this module, deepest last. Empty at the scope. */
  spine: string[];
  onSpine: boolean;
  depth: number;
  nav: NavState;
}) {
  const open = nav.wish.get(path) ?? onSpine;
  const name = lastSegment(path);

  return (
    <li className="nav-branch">
      <div
        className="nav-row"
        style={{ "--depth": depth } as React.CSSProperties}
      >
        <button
          type="button"
          className={onSpine ? "nav-twisty spine" : "nav-twisty"}
          aria-expanded={open}
          aria-label={`${open ? "Collapse" : "Expand"} ${name}`}
          onClick={() => nav.setWish(path, !open)}
        >
          {open ? "⏷" : "⏵"}
        </button>
        <Link
          href={itemHref(path)}
          className={onSpine ? "nav-module spine" : "nav-module"}
          currentClassName="active"
          exact
          data-item-path={path}
        >
          {name}
        </Link>
      </div>
      {open ? (
        <NavLevel
          path={path}
          spine={spine}
          onSpine={onSpine}
          depth={depth + 1}
          nav={nav}
        />
      ) : null}
    </li>
  );
}

/** The children of an open module: submodule branches, then kind groups. */
function NavLevel({
  path,
  spine,
  onSpine,
  depth,
  nav,
}: {
  path: string;
  spine: string[];
  onSpine: boolean;
  depth: number;
  nav: NavState;
}) {
  const { data } = useItem(path);
  const items = data?.body.kind === "module" ? (data.body.items ?? []) : [];

  const child = (item: ModuleItem) => `${path}::${item.path}`;
  const modules = byName(items.filter((item) => item.kind === "module"));
  const leaves = items.filter((item) => item.kind !== "module");

  const spineChild = spine[0];
  // A spine that points at something this level does not have as a module — an
  // enum variant's path, say — is a spine that ends here. Stop following it,
  // rather than rendering a level with nothing in it.
  const descend =
    spineChild && modules.some((item) => child(item) === spineChild)
      ? spineChild
      : undefined;

  // The scope: the deepest spine module, whose contents are the point of the nav.
  const isScope = onSpine && !descend;
  // An ancestor is *context*: it shows the way down and little else, unless it is
  // small enough to show for free or the reader has opened it. An off-spine module
  // the reader opened has no way down, so it always shows everything.
  const showAll =
    !descend ||
    nav.wish.get(path) === true ||
    modules.length <= ANCESTOR_MODULE_THRESHOLD;

  const shown = showAll
    ? modules
    : modules.filter((item) => child(item) === descend);
  const hiddenModules = modules.length - shown.length;

  return (
    <ul className="nav-children">
      {shown.map((item) => {
        const childPath = child(item);
        const isSpine = childPath === descend;
        return (
          <ModuleBranch
            key={childPath}
            path={childPath}
            spine={isSpine ? spine.slice(1) : []}
            onSpine={isSpine}
            depth={depth}
            nav={nav}
          />
        );
      })}

      {showAll ? (
        leaves.length ? (
          <KindGroups
            path={path}
            leaves={leaves}
            // Ancestor levels keep their items behind headers: you are passing
            // through, not reading here.
            context={onSpine && !isScope}
            depth={depth}
            nav={nav}
          />
        ) : null
      ) : (
        <li>
          <button
            type="button"
            className="nav-more"
            style={{ "--depth": depth } as React.CSSProperties}
            onClick={() => nav.setWish(path, true)}
          >
            ⋯ {expanderLabel(hiddenModules, leaves.length)}
          </button>
        </li>
      )}
    </ul>
  );
}

function expanderLabel(modules: number, items: number): string {
  const parts: string[] = [];
  if (modules) parts.push(plural(modules, "module"));
  if (items) parts.push(plural(items, "item"));
  return parts.join(", ");
}

/** A level's non-module children, under the terminal's kind headings. */
function KindGroups({
  path,
  leaves,
  context,
  depth,
  nav,
}: {
  path: string;
  leaves: ModuleItem[];
  context: boolean;
  depth: number;
  nav: NavState;
}) {
  return (
    <>
      {moduleGroups(leaves).map((group) => {
        const key = `${path}#${group.key}`;
        const holdsActive =
          !!nav.active &&
          group.items.some((item) => `${path}::${item.path}` === nav.active);
        // The group you are reading in is open in full; the rest follow the flat
        // nav's old size heuristic, except on an ancestor where they all shut.
        const byDefault =
          holdsActive || (!context && group.items.length <= COLLAPSE_THRESHOLD);
        const open = nav.flipped.has(key) ? !byDefault : byDefault;

        return (
          <li key={group.key} className="nav-group">
            <button
              type="button"
              className="nav-group-label"
              style={{ "--depth": depth } as React.CSSProperties}
              aria-expanded={open}
              onClick={() => nav.toggleGroup(key)}
            >
              <span className="nav-twisty" aria-hidden>
                {open ? "⏷" : "⏵"}
              </span>
              {group.label}
              {open ? null : (
                <span className="nav-group-count">{group.items.length}</span>
              )}
            </button>
            {open ? (
              <ul className="nav-children">
                {byName(group.items).map((item) => (
                  <NavLeaf
                    key={item.path}
                    path={`${path}::${item.path}`}
                    name={item.path}
                    depth={depth + 1}
                    active={`${path}::${item.path}` === nav.active}
                  />
                ))}
              </ul>
            ) : null}
          </li>
        );
      })}
    </>
  );
}

/**
 * A leaf item. The active one scrolls itself into view: it can sit hundreds of
 * rows down a `std` group, and a nav that does not show you where you are is the
 * problem this tree exists to fix.
 */
function NavLeaf({
  path,
  name,
  depth,
  active,
}: {
  path: string;
  name: string;
  depth: number;
  active: boolean;
}) {
  const ref = useRef<HTMLLIElement>(null);

  useEffect(() => {
    if (active) ref.current?.scrollIntoView({ block: "nearest" });
  }, [active]);

  return (
    <li ref={ref}>
      <Link
        href={itemHref(path)}
        className="nav-item"
        currentClassName="active"
        exact
        style={{ "--depth": depth } as React.CSSProperties}
        data-item-path={path}
      >
        {name}
      </Link>
    </li>
  );
}
