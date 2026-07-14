/**
 * Placement for the hover card.
 *
 * Two axes, because the two places a reader hovers from want different things.
 * Prose and signatures are read across, so a card sits *under* the link and the
 * eye returns to the same column. The crate nav is a stack of rows, and a card
 * under one row covers the rows beneath it — the very list you are scanning — so
 * a nav card sits *beside* the row instead.
 *
 * Kept pure — rects in, coordinates out — so the geometry can be tested without a
 * layout engine, and so the component is left with nothing to do but measure.
 */

/** Distance held between the link and the card's visible edge. */
export const GAP = 8;
/** Closest the card may sit to the viewport edge. */
export const MARGIN = 10;

export interface Rect {
  top: number;
  left: number;
  right: number;
  bottom: number;
  width: number;
  height: number;
}

export interface Viewport {
  width: number;
  height: number;
}

/**
 * Which way the card is thrown from its link: `block` puts it below (flipping
 * above), `inline` puts it to the right (flipping left).
 */
export type Axis = "block" | "inline";

export type Side = "top" | "bottom" | "left" | "right";

export interface Placement {
  /** Viewport coordinates — the card is `position: fixed`. */
  top: number;
  left: number;
  /** The side of the link the card landed on; drives the bridge and the animation. */
  side: Side;
}

/**
 * Place `card` against `anchor`.
 *
 * `anchor` is the rect the card is thrown from — for an `inline` card that is the
 * link's *text*, not its box. See `anchorRect` in `HoverPreview`.
 */
export function place(
  anchor: Rect,
  card: Rect,
  viewport: Viewport,
  axis: Axis = "block",
): Placement {
  return axis === "inline"
    ? beside(anchor, card, viewport)
    : below(anchor, card, viewport);
}

/**
 * Below the link, flipping above only when the card would overflow the bottom
 * *and* there is more room above — flipping to a side where it still does not fit
 * only trades one clipped edge for another.
 *
 * Horizontally it is left-aligned to the link, not centered: a signature reads
 * left to right, and an aligned card keeps the eye on the column it was already in.
 */
function below(anchor: Rect, card: Rect, viewport: Viewport): Placement {
  const room = viewport.height - anchor.bottom - GAP - MARGIN;
  const roomAbove = anchor.top - GAP - MARGIN;
  const side: Side = card.height > room && roomAbove > room ? "top" : "bottom";

  const top =
    side === "bottom" ? anchor.bottom + GAP : anchor.top - GAP - card.height;

  return { top, left: clamp(anchor.left, card.width, viewport.width), side };
}

/**
 * Beside the link — to the right, flipping left when the right runs out (a nav
 * pinned to the right edge of a narrow window, say).
 *
 * This is thrown from the end of the link's *text*, which is why `anchor` must be
 * the text's rect and not the element's. A nav row is a full-width block: its
 * right edge is the far side of the column however short the name inside it, so a
 * card thrown from the box drifts a column away from a word like `pin` and stops
 * reading as that word's card.
 *
 * Vertically the card aligns with the row, so the name being pointed at and the
 * name on the card sit on the same line. Clamped, which is what keeps a tall card
 * anchored to the last row of a long list on screen.
 */
function beside(anchor: Rect, card: Rect, viewport: Viewport): Placement {
  const room = viewport.width - anchor.right - GAP - MARGIN;
  const roomLeft = anchor.left - GAP - MARGIN;
  const side: Side = card.width > room && roomLeft > room ? "left" : "right";

  const left =
    side === "right" ? anchor.right + GAP : anchor.left - GAP - card.width;

  return {
    top: clamp(anchor.top, card.height, viewport.height),
    left: clamp(left, card.width, viewport.width),
    side,
  };
}

/**
 * Hold a `size`-long card starting at `start` inside `extent`, honoring the
 * margin at both ends. The outer `max` guards the case where the card is larger
 * than the viewport allows: pinning the near edge beats centering the overflow.
 */
function clamp(start: number, size: number, extent: number): number {
  const max = extent - size - MARGIN;
  return Math.min(Math.max(start, MARGIN), Math.max(max, MARGIN));
}
