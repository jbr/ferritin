/**
 * The landing page's rotating example: one famous Rust type at a time, cross-faded
 * to the next on a slow timer so the page has a pulse without ever nagging.
 *
 * Three behaviours make it feel considered rather than busy:
 *  - it pauses while the pointer rests on the type, so a name you want to read (or
 *    click) does not slip away mid-thought;
 *  - it carries `data-item-path`, so the shared hover card (see `HoverPreview`)
 *    opens over it exactly as it does over any type in a signature; and
 *  - it honours `prefers-reduced-motion` by swapping instantly, with no dip.
 */
import { useEffect, useMemo, useRef, useState } from "react";
import { Link } from "rhoto-router";
import { FAMOUS_TYPES } from "../lib/famousTypes";
import { itemHref } from "../lib/paths";

/** How long each type is held before the next fades in. */
const HOLD_MS = 3600;
/**
 * The fade half-duration. Kept in step with the `opacity` transition in
 * `.home-cycle-type` (layout.css) — the timer swaps the text at the trough, so if
 * the two drift the swap becomes visible.
 */
const FADE_MS = 650;

/** A fresh Fisher–Yates order, so reloads don't always open on the same type. */
function shuffled<T>(items: readonly T[]): T[] {
  const out = items.slice();
  for (let i = out.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [out[i], out[j]] = [out[j], out[i]];
  }
  return out;
}

function prefersReducedMotion(): boolean {
  return (
    typeof window !== "undefined" &&
    window.matchMedia?.("(prefers-reduced-motion: reduce)").matches
  );
}

export function CyclingType() {
  const order = useMemo(() => shuffled(FAMOUS_TYPES), []);
  const [index, setIndex] = useState(0);
  const [visible, setVisible] = useState(true);
  /** Read by the timer without re-arming it: hovering must not restart the cycle. */
  const paused = useRef(false);

  useEffect(() => {
    let holdTimer: number | undefined;
    let fadeTimer: number | undefined;

    // A self-rescheduling loop rather than a fixed interval, so hovering can defer
    // the next swap without tearing down and rebuilding a timer.
    const scheduleHold = (delay: number) => {
      holdTimer = window.setTimeout(tick, delay);
    };

    const tick = () => {
      // Poll while the pointer rests on the type: the shared hover card is keyed to
      // this element's `data-item-path`, so advancing the index under an open card
      // would leave the card describing one type and the link showing the next.
      if (paused.current) return scheduleHold(400);

      const advance = () => setIndex((n) => (n + 1) % order.length);
      if (prefersReducedMotion()) {
        advance(); // no dip; swap in place
        return scheduleHold(HOLD_MS);
      }

      setVisible(false); // fade the current type out...
      fadeTimer = window.setTimeout(() => {
        setVisible(true); // ...and back in, whether or not we swap
        // A hover that landed mid-fade aborts the swap: the same type fades back,
        // so the pointer never has a card pulled out from under it.
        if (!paused.current) advance();
        scheduleHold(HOLD_MS);
      }, FADE_MS);
    };

    scheduleHold(HOLD_MS);
    return () => {
      window.clearTimeout(holdTimer);
      window.clearTimeout(fadeTimer);
    };
  }, [order.length]);

  const path = order[index];

  return (
    <span
      className="home-cycle"
      onPointerEnter={() => {
        paused.current = true;
      }}
      onPointerLeave={() => {
        paused.current = false;
      }}
    >
      <Link
        href={itemHref(path)}
        className="home-cycle-type"
        data-item-path={path}
        style={{ opacity: visible ? 1 : 0 }}
      >
        {path}
      </Link>
    </span>
  );
}
