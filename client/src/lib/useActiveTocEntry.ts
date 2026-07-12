import { useCallback, useEffect, useRef, useState } from "react";
import type { TocEntry } from "./toc";

// Distance below the viewport top (px) that counts as "you're reading here" —
// roughly the sticky top bar's height plus a little breathing room, matching the
// `scroll-margin-top` anchors are offset by.
const ACTIVE_LINE = 96;

/**
 * Track which TOC entry is "current" and expose a `pin` for TOC clicks.
 *
 * Two inputs decide the active entry, in priority order:
 *  1. **Pin** — the most recently clicked entry stays active as long as its
 *     element remains in view. Clicking scrolls it near the top; the pin holds
 *     the highlight there (including through the smooth-scroll animation, which
 *     would otherwise flicker across every section it passes) until the reader
 *     scrolls it out of view, at which point the pin releases.
 *  2. **Scrollspy** — otherwise, the active entry is the last one whose heading
 *     has scrolled above the `ACTIVE_LINE`, i.e. the section you're reading.
 *     Entries arrive in document order (see `buildToc`), so a single top-down
 *     pass finds it.
 */
export function useActiveTocEntry(entries: TocEntry[]): {
  activeId: string | null;
  pin: (id: string) => void;
} {
  const [activeId, setActiveId] = useState<string | null>(null);
  // The pinned entry, and whether its element has been seen in view yet. While
  // still `seeking` (the click's smooth-scroll is en route), the pin holds
  // unconditionally so the highlight doesn't flicker through passed sections;
  // once it has `arrived`, the pin releases as soon as it scrolls back out.
  const pinRef = useRef<{ id: string; seeking: boolean } | null>(null);

  useEffect(() => {
    if (entries.length === 0) {
      setActiveId(null);
      return;
    }
    // A fresh entry set means a new page — drop any stale pin.
    pinRef.current = null;

    let raf = 0;
    const compute = () => {
      raf = 0;

      // 1. Honor a live pin.
      const pinned = pinRef.current;
      if (pinned) {
        const el = document.getElementById(pinned.id);
        const r = el?.getBoundingClientRect();
        const inView = r ? r.bottom > 0 && r.top < window.innerHeight : false;
        if (inView) pinned.seeking = false; // reached it
        if (pinned.seeking || inView) {
          setActiveId(pinned.id);
          return;
        }
        pinRef.current = null; // arrived earlier, now scrolled away — release
      }

      // 2. Scrollspy: the lowest heading still above the active line. At the
      //    very bottom of the page the last section may never cross the line, so
      //    snap to it once we've hit the end of the scroll range.
      const atBottom =
        window.innerHeight + window.scrollY >=
        document.documentElement.scrollHeight - 2;
      if (atBottom) {
        setActiveId(entries[entries.length - 1].id);
        return;
      }

      let current = entries[0].id;
      for (const entry of entries) {
        const el = document.getElementById(entry.id);
        if (!el) continue;
        if (el.getBoundingClientRect().top <= ACTIVE_LINE) current = entry.id;
      }
      setActiveId(current);
    };

    const onScroll = () => {
      if (!raf) raf = requestAnimationFrame(compute);
    };

    compute();
    window.addEventListener("scroll", onScroll, { passive: true });
    window.addEventListener("resize", onScroll);
    return () => {
      window.removeEventListener("scroll", onScroll);
      window.removeEventListener("resize", onScroll);
      if (raf) cancelAnimationFrame(raf);
    };
  }, [entries]);

  const pin = useCallback((id: string) => {
    pinRef.current = { id, seeking: true };
    setActiveId(id);
  }, []);

  return { activeId, pin };
}
