import { useState } from "react";
import { Link } from "rhoto-router";
import { useItem } from "../../api/queries";
import { itemHref } from "../../lib/paths";
import { moduleGroups } from "../../lib/toc";

// Groups larger than this collapse by default; smaller ones start expanded.
const COLLAPSE_THRESHOLD = 15;

/**
 * Left sidebar: the crate's top-level items, grouped by kind (mirroring the
 * terminal's grouping), sourced from the crate root module
 * (`GET /api/crates/{crate}`). The current item is highlighted via the router's
 * active-link class. Every group is a collapsible toggle — large ones start
 * collapsed, small ones expanded — so no group is visually the odd one out.
 */
export function CrateNav({ crate }: { crate?: string }) {
  const { data } = useItem(crate ?? "");
  // Kinds the user has flipped away from their default expanded state.
  const [flipped, setFlipped] = useState<Set<string>>(new Set());

  if (!crate) return <nav className="crate-nav" />;

  const items = data?.body.kind === "module" ? (data.body.items ?? []) : [];
  const groups = moduleGroups(items);

  const toggleGroup = (key: string) => {
    const next = new Set(flipped);
    if (next.has(key)) {
      next.delete(key);
    } else {
      next.add(key);
    }
    setFlipped(next);
  };

  return (
    <nav className="crate-nav" aria-label={`${crate} contents`}>
      {groups.map((group) => {
        const defaultExpanded = group.items.length <= COLLAPSE_THRESHOLD;
        const isExpanded = flipped.has(group.key)
          ? !defaultExpanded
          : defaultExpanded;

        return (
          <div key={group.key} className="crate-nav-group">
            <button
              className="crate-nav-label crate-nav-toggle"
              onClick={() => toggleGroup(group.key)}
              aria-expanded={isExpanded}
            >
              {group.label}
              <span className="crate-nav-toggle-icon" aria-hidden>
                {isExpanded ? "▼" : "▶"}
              </span>
            </button>
            {isExpanded && (
              <ul className="crate-nav-list">
                {group.items.map((it) => (
                  <li key={it.path}>
                    <Link
                      href={itemHref(`${crate}::${it.path}`)}
                      className="crate-nav-item"
                      currentClassName="active"
                      exact
                    >
                      {it.path}
                    </Link>
                  </li>
                ))}
              </ul>
            )}
          </div>
        );
      })}
    </nav>
  );
}
