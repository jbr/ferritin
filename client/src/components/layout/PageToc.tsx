import { useEffect, useRef } from "react";
import type { TocEntry } from "../../lib/toc";
import { useActiveTocEntry } from "../../lib/useActiveTocEntry";

/** Right sidebar: "on this page" anchors into the current item's sections. */
export function PageToc({ entries }: { entries: TocEntry[] }) {
  const activeId = useActiveTocEntry(entries);
  const activeRef = useRef<HTMLAnchorElement>(null);

  // Auto-scroll the TOC to keep the active entry visible
  useEffect(() => {
    if (activeRef.current) {
      activeRef.current.scrollIntoView({
        behavior: "smooth",
        block: "nearest",
      });
    }
  }, [activeId]);

  const handleTocClick = (e: React.MouseEvent<HTMLAnchorElement>) => {
    e.preventDefault();
    const href = e.currentTarget.getAttribute("href");
    if (!href) return;

    const targetId = href.slice(1); // Remove the #
    const target = document.getElementById(targetId);
    if (target) {
      target.scrollIntoView({ behavior: "smooth" });
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
