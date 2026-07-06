import { useEffect, useState } from "react";
import type { TocEntry } from "./toc";

/**
 * Track which TOC entry is currently visible in the viewport and return its ID.
 * Uses IntersectionObserver to detect when sections enter/exit the viewport.
 */
export function useActiveTocEntry(entries: TocEntry[]): string | null {
  const [activeId, setActiveId] = useState<string | null>(null);

  useEffect(() => {
    if (entries.length === 0) return;

    // Create a ref to track which element is closest to the top of the viewport
    let closestEntry: { id: string; distance: number } | null = null;

    const observer = new IntersectionObserver(
      (sections) => {
        // For each entry, find its position relative to the viewport top
        for (const section of sections) {
          const rect = section.boundingClientRect;
          // Distance from top of viewport (negative = above, positive = below)
          const distance = Math.abs(rect.top);

          // Only consider sections that are in view or recently passed
          if (rect.top >= -100 && rect.top < window.innerHeight) {
            if (!closestEntry || distance < closestEntry.distance) {
              closestEntry = {
                id: section.target.id,
                distance,
              };
            }
          }
        }

        // If we found a close entry, highlight it
        if (closestEntry) {
          setActiveId(closestEntry.id);
        }
      },
      {
        // Trigger callback when any entry is partially or fully visible
        threshold: [0, 0.25, 0.5, 0.75, 1],
      }
    );

    // Observe all sections corresponding to TOC entries
    for (const entry of entries) {
      const element = document.getElementById(entry.id);
      if (element) {
        observer.observe(element);
      }
    }

    return () => {
      observer.disconnect();
    };
  }, [entries]);

  return activeId;
}
