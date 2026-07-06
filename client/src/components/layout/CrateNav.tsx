import { useState } from "react";
import { Link } from "rhoto-router";
import { useItem } from "../../api/queries";
import { itemHref } from "../../lib/paths";
import { moduleGroupLabel, moduleKinds } from "../../lib/toc";

// Threshold for collapsing a group by default
const COLLAPSE_THRESHOLD = 15;

/**
 * Left sidebar: the crate's top-level items, grouped by kind, sourced from the
 * crate root module (`GET /api/crates/{crate}`). The current item is highlighted
 * via the router's active-link class. Large groups are collapsible.
 */
export function CrateNav({ crate }: { crate?: string }) {
  const { data } = useItem(crate ?? "");
  const [expandedGroups, setExpandedGroups] = useState<Set<string>>(new Set());

  if (!crate) return <nav className="crate-nav" />;

  const items = data?.body.kind === "module" ? (data.body.items ?? []) : [];
  const kinds = moduleKinds(items);

  const toggleGroup = (kind: string) => {
    const next = new Set(expandedGroups);
    if (next.has(kind)) {
      next.delete(kind);
    } else {
      next.add(kind);
    }
    setExpandedGroups(next);
  };

  return (
    <nav className="crate-nav" aria-label={`${crate} contents`}>
      {kinds.map((kind) => {
        const kindItems = items.filter((it) => it.kind === kind);
        const isLarge = kindItems.length > COLLAPSE_THRESHOLD;
        const isExpanded = expandedGroups.has(kind) || !isLarge;

        return (
          <div key={kind} className="crate-nav-group">
            {isLarge ? (
              <button
                className="crate-nav-label crate-nav-toggle"
                onClick={() => toggleGroup(kind)}
                aria-expanded={isExpanded}
              >
                {moduleGroupLabel(kind)}
                <span className="crate-nav-toggle-icon" aria-hidden>
                  {isExpanded ? "▼" : "▶"}
                </span>
              </button>
            ) : (
              <div className="crate-nav-label">{moduleGroupLabel(kind)}</div>
            )}
            {isExpanded && (
              <ul className="crate-nav-list">
                {kindItems.map((it) => (
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
