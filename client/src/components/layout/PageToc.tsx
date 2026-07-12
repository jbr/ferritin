import { useEffect, useRef } from "react";
import type { TocEntry } from "../../lib/toc";
import { useActiveTocEntry } from "../../lib/useActiveTocEntry";

/** Right sidebar: "on this page" anchors into the current item's sections. */
export function PageToc({ entries }: { entries: TocEntry[] }) {
  const { activeId, pin } = useActiveTocEntry(entries);
  const activeRef = useRef<HTMLAnchorElement>(null);

  // Keep the active entry visible *within the TOC sidebar* by nudging the
  // sidebar's own scroll — never `scrollIntoView`, which also scrolls the
  // window and would cancel the content scroll a TOC click just started.
  useEffect(() => {
    const anchor = activeRef.current;
    const rail = anchor?.closest<HTMLElement>(".page-toc");
    if (!anchor || !rail) return;
    const a = anchor.getBoundingClientRect();
    const r = rail.getBoundingClientRect();
    if (a.top < r.top) rail.scrollTop -= r.top - a.top + 8;
    else if (a.bottom > r.bottom) rail.scrollTop += a.bottom - r.bottom + 8;
  }, [activeId]);

  const handleTocClick = (e: React.MouseEvent<HTMLAnchorElement>) => {
    e.preventDefault();
    const href = e.currentTarget.getAttribute("href");
    if (!href) return;

    const targetId = href.slice(1); // Remove the #
    const target = document.getElementById(targetId);
    if (target) {
      // Pin so the highlight lands on the clicked entry, then jump. Native
      // smooth scrolling misbehaves in this shell (it no-ops mid-flight), so we
      // scroll instantly — reliable, and the pin keeps the highlight correct.
      pin(targetId);
      target.scrollIntoView({ block: "start" });
    }
  };

  if (entries.length <= 1) return <aside className="page-toc" />;
  return (
    <aside className="page-toc" aria-label="On this page">
      <div className="page-toc-label">On this page</div>
      <ul className="page-toc-list">
        {entries.map((entry) => (
          <li
            key={entry.id}
            className={entry.depth === 1 ? "toc-child" : "toc-top"}
          >
            <a
              ref={entry.id === activeId ? activeRef : null}
              href={`#${entry.id}`}
              onClick={handleTocClick}
              data-active={entry.id === activeId}
            >
              {entry.label}
            </a>
          </li>
        ))}
      </ul>
    </aside>
  );
}
