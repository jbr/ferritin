import { describe, expect, it } from "vitest";
import { GAP, MARGIN, place, type Rect, type Viewport } from "./position";

/** A rect, from its top-left corner and size. */
function rect(left: number, top: number, width: number, height: number): Rect {
  return {
    left,
    top,
    width,
    height,
    right: left + width,
    bottom: top + height,
  };
}

const VIEWPORT: Viewport = { width: 1000, height: 800 };
/** A card of a typical size — small enough to fit anywhere in `VIEWPORT`. */
const CARD = rect(0, 0, 300, 120);

describe("place, below (the `block` axis — prose and signatures)", () => {
  it("sits under the link, left-aligned to it", () => {
    const link = rect(400, 300, 60, 20);
    expect(place(link, CARD, VIEWPORT)).toEqual({
      side: "bottom",
      top: 320 + GAP,
      left: 400,
    });
  });

  it("flips above when the card would overflow the bottom", () => {
    // 60px of room below, and the card needs 120.
    const link = rect(400, 720, 60, 20);
    const placement = place(link, CARD, VIEWPORT);
    expect(placement.side).toBe("top");
    expect(placement.top).toBe(720 - GAP - 120);
  });

  it("stays below when flipping would not help either", () => {
    // A link near the top: little room below, but even less above. Flipping would
    // only trade one clipped edge for another, so it does not flip.
    const link = rect(400, 30, 60, 20);
    expect(place(link, CARD, VIEWPORT).side).toBe("bottom");
  });

  it("clamps a card that would overhang the right edge", () => {
    const link = rect(950, 300, 40, 20);
    // Left-aligning to the link would put the card's right edge at 1250.
    expect(place(link, CARD, VIEWPORT).left).toBe(1000 - 300 - MARGIN);
  });

  it("never crosses the left margin", () => {
    const link = rect(2, 300, 40, 20);
    expect(place(link, CARD, VIEWPORT).left).toBe(MARGIN);
  });
});

describe("place, beside (the `inline` axis — the crate nav)", () => {
  it("sits to the right of the link's text, top-aligned with it", () => {
    // The anchor is the *text* rect, which is why a short word in a wide nav row
    // still puts the card next to the word. See `anchorRect` in HoverPreview.
    const text = rect(30, 300, 24, 18);
    expect(place(text, CARD, VIEWPORT, "inline")).toEqual({
      side: "right",
      top: 300,
      left: 54 + GAP,
    });
  });

  it("flips to the left when the right runs out", () => {
    const text = rect(900, 300, 40, 18);
    const placement = place(text, CARD, VIEWPORT, "inline");
    expect(placement.side).toBe("left");
    expect(placement.left).toBe(900 - GAP - 300);
  });

  it("clamps vertically so a card beside the last row stays on screen", () => {
    const text = rect(30, 780, 24, 18);
    const placement = place(text, CARD, VIEWPORT, "inline");
    expect(placement.side).toBe("right");
    expect(placement.top).toBe(800 - 120 - MARGIN);
  });

  it("pins the near edge rather than centering a card too big to fit", () => {
    const tall = rect(0, 0, 300, 900); // taller than the viewport
    const text = rect(30, 400, 24, 18);
    expect(place(text, tall, VIEWPORT, "inline").top).toBe(MARGIN);
  });
});
