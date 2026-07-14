/**
 * The pointer state machine behind the hover card.
 *
 * One delegated listener on the document, not a handler per link: a single item
 * page renders hundreds of `[data-item-path]` spans (every type in every
 * signature), and binding each one would cost more than the feature is worth.
 *
 * The design goal is *unobtrusive*. A card only appears after the pointer has
 * deliberately rested on a link, it never appears while you are scrolling,
 * dragging, or clicking through, and it always yields to an explicit dismissal.
 */
import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type RefObject,
} from "react";
import type { Axis } from "./position";

/**
 * Timing knobs, gathered so the feel can be tuned in one place.
 *
 * `warmOpenMs` is the shorter delay used when a card was open moments ago: once
 * you are reading previews, scanning to the next type should keep up with you,
 * while the first card of a session still has to be asked for.
 */
export const HOVER_TUNING = {
  /** Rest time on a link before the first card opens. */
  openMs: 380,
  /** Rest time when another card was open within `warmWindowMs`. */
  warmOpenMs: 120,
  /** How long a card stays "warm" after closing. */
  warmWindowMs: 500,
  /** Grace period after the pointer leaves, so it can travel into the card. */
  closeMs: 140,
};

export interface HoverTarget {
  el: HTMLElement;
  path: string;
  /** How the card is thrown from this link — see `data-preview-axis`, below. */
  axis: Axis;
}

/**
 * A region may declare which way cards should open inside it, with
 * `data-preview-axis="inline"` on any ancestor of its links. The crate nav does:
 * it is a column of rows, and a card thrown *downward* would cover the rows the
 * reader is scanning. Prose says nothing and gets the default.
 *
 * Declaring it on the region keeps the preview from having to know what a "nav"
 * is — placement is a property of the surface the link lives on, not of the card.
 */
function axisOf(el: Element): Axis {
  return el
    .closest("[data-preview-axis]")
    ?.getAttribute("data-preview-axis") === "inline"
    ? "inline"
    : "block";
}

/**
 * Track which item link the pointer is resting on.
 *
 * `cardRef` is the rendered card; the machine needs it to tell "the pointer left
 * the link" from "the pointer moved into the card", which are the same DOM event
 * from the link's point of view.
 */
export function useHoverTarget(cardRef: RefObject<HTMLElement | null>) {
  const [target, setTarget] = useState<HoverTarget | null>(null);

  const openTimer = useRef<number | undefined>(undefined);
  const closeTimer = useRef<number | undefined>(undefined);
  /** Set to the last-closed time, so a follow-up hover opens faster. */
  const warmUntil = useRef(0);
  /** A link dismissed with Escape stays dismissed until the pointer leaves it. */
  const suppressed = useRef<Element | null>(null);
  /** Mirrors `target` for the event handlers, which are bound once. */
  const current = useRef<HoverTarget | null>(null);
  /** The link the pending open timer belongs to. */
  const pending = useRef<Element | null>(null);

  const clearTimers = useCallback(() => {
    window.clearTimeout(openTimer.current);
    window.clearTimeout(closeTimer.current);
    openTimer.current = undefined;
    closeTimer.current = undefined;
    pending.current = null;
  }, []);

  const close = useCallback(() => {
    clearTimers();
    if (current.current)
      warmUntil.current = Date.now() + HOVER_TUNING.warmWindowMs;
    current.current = null;
    setTarget(null);
  }, [clearTimers]);

  useEffect(() => {
    const insideCard = (node: EventTarget | null) =>
      node instanceof Node && !!cardRef.current?.contains(node);

    /**
     * Open `el` after the appropriate rest.
     *
     * A countdown belongs to the link that started it. Moving to a *different*
     * link restarts it — otherwise the pointer could leave A for the adjacent B
     * and still be handed a card for A when A's timer came due. Rows in the crate
     * nav are directly adjacent, with no prose between them to cancel the timer,
     * so this is the common case there rather than a corner one.
     */
    const scheduleOpen = (el: HTMLElement, path: string) => {
      if (current.current?.el === el) {
        // Already showing this link: the pointer merely moved within it.
        window.clearTimeout(closeTimer.current);
        closeTimer.current = undefined;
        return;
      }
      if (pending.current === el) return; // already counting down for this link

      window.clearTimeout(openTimer.current);
      pending.current = el;
      const delay =
        Date.now() < warmUntil.current
          ? HOVER_TUNING.warmOpenMs
          : HOVER_TUNING.openMs;
      openTimer.current = window.setTimeout(() => {
        openTimer.current = undefined;
        pending.current = null;
        window.clearTimeout(closeTimer.current);
        closeTimer.current = undefined;
        const next = { el, path, axis: axisOf(el) };
        current.current = next;
        setTarget(next);
      }, delay);
    };

    const scheduleClose = () => {
      window.clearTimeout(openTimer.current);
      openTimer.current = undefined;
      pending.current = null;
      if (!current.current || closeTimer.current !== undefined) return;
      closeTimer.current = window.setTimeout(close, HOVER_TUNING.closeMs);
    };

    /**
     * `pointerover` fires for every element the pointer enters, so it alone can
     * drive the machine: an item link arms the open timer, anything else arms
     * the close timer, and the card itself holds the card open.
     */
    const onPointerOver = (event: PointerEvent) => {
      if (event.pointerType !== "mouse") return; // touch has no hover to speak of

      if (insideCard(event.target)) {
        // The pointer made it into the card — cancel the pending close.
        window.clearTimeout(closeTimer.current);
        closeTimer.current = undefined;
        return;
      }

      const el =
        event.target instanceof Element
          ? event.target.closest<HTMLElement>("[data-item-path]")
          : null;

      if (!el) {
        suppressed.current = null;
        scheduleClose();
        return;
      }
      if (el === suppressed.current) return;
      suppressed.current = null;

      const path = el.dataset.itemPath;
      if (!path) return;
      scheduleOpen(el, path);
    };

    /** Escape dismisses, and the dismissed link stays quiet while you sit on it. */
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || !current.current) return;
      suppressed.current = current.current.el;
      close();
    };

    /**
     * A press outside the card means the reader is doing something else —
     * following the link, or starting a text selection. Get out of the way at
     * once, with no grace period. A press *inside* the card is the reader using
     * it, so it must survive long enough for the click to land.
     */
    const onPointerDown = (event: PointerEvent) => {
      if (!insideCard(event.target)) close();
    };

    /** A click inside the card followed one of its links: the page is changing. */
    const onClick = (event: MouseEvent) => {
      if (insideCard(event.target)) close();
    };

    /** A card anchored to a link that is sliding up the page is just debris. */
    const onScroll = () => close();

    const onPointerLeave = () => close();
    const onBlur = () => close();

    document.addEventListener("pointerover", onPointerOver);
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("click", onClick);
    document.addEventListener("keydown", onKeyDown);
    document.addEventListener("pointerleave", onPointerLeave);
    window.addEventListener("scroll", onScroll, {
      capture: true,
      passive: true,
    });
    window.addEventListener("blur", onBlur);

    return () => {
      document.removeEventListener("pointerover", onPointerOver);
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("click", onClick);
      document.removeEventListener("keydown", onKeyDown);
      document.removeEventListener("pointerleave", onPointerLeave);
      window.removeEventListener("scroll", onScroll, { capture: true });
      window.removeEventListener("blur", onBlur);
      clearTimers();
    };
  }, [cardRef, close, clearTimers]);

  return target;
}
