/**
 * A hover card: the shape of an item, in one glance.
 *
 * Mounted once, at the app root, and shown for whichever `[data-item-path]` link
 * the pointer is resting on — every type in every signature, every name in the
 * prose. It is a *taste*, not a window into a window: what kind of item it is,
 * its opening sentence, and a few counts. Anything more and you should click.
 */
import { useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useItem } from "../../api/queries";
import { itemPreview } from "../../lib/preview";
import { Nodes } from "../../render/Nodes";
import { place, type Axis, type Placement, type Rect } from "./position";
import { useHoverTarget } from "./useHoverTarget";

/**
 * The rect a card is thrown from.
 *
 * For a card placed *beside* its link, that is the link's rendered text, not its
 * element box. A crate-nav row is a full-width block, so its box reaches the far
 * side of the column however short the name inside it — and a card thrown from
 * that edge lands a column away from a word like `pin`, no longer reading as that
 * word's card. A `Range` over the contents measures the glyphs themselves.
 *
 * Cards placed *below* keep the element box: those links wrap their own text
 * already, and the box is what a wrapped multi-line link should be aligned to.
 */
function anchorRect(el: HTMLElement, axis: Axis): Rect {
  if (axis !== "inline") return el.getBoundingClientRect();

  const range = document.createRange();
  range.selectNodeContents(el);
  const text = range.getBoundingClientRect();
  // An element with no laid-out text (an icon-only row) measures zero; its box is
  // then the only anchor there is.
  return text.width > 0 ? text : el.getBoundingClientRect();
}

export function HoverPreview() {
  const cardRef = useRef<HTMLDivElement>(null);
  const target = useHoverTarget(cardRef);
  const path = target?.path ?? "";

  // The item endpoint, on the same `["item", path]` key `ItemView` reads: the
  // card's fetch is also a prefetch of the page a hover so often precedes. A
  // failed lookup resolves to no data, and the card simply never appears.
  const { data } = useItem(path, path.length > 0);

  const [placement, setPlacement] = useState<Placement | null>(null);

  // Measured, then placed, before the browser paints — so the card is never seen
  // at the origin on its way to the anchor. Placement is cleared with the target,
  // which is what keeps a stale position from being reused by the next card.
  useLayoutEffect(() => {
    const card = cardRef.current;
    if (!card || !target || !data) {
      setPlacement(null);
      return;
    }
    setPlacement(
      place(
        anchorRect(target.el, target.axis),
        card.getBoundingClientRect(),
        { width: window.innerWidth, height: window.innerHeight },
        target.axis,
      ),
    );
  }, [target, data]);

  if (!target || !data) return null;

  const preview = itemPreview(data);

  return createPortal(
    <div
      ref={cardRef}
      className="hover-preview"
      data-side={placement?.side ?? "bottom"}
      // Before the layout effect has measured it the card is laid out but not
      // shown; `visibility` keeps it measurable while `hidden` would not.
      style={{
        top: placement?.top ?? 0,
        left: placement?.left ?? 0,
        visibility: placement ? "visible" : "hidden",
      }}
    >
      <header className="hover-preview-head">
        <span className="kind-badge">{preview.kind}</span>
        <code className="hover-preview-path">{path}</code>
      </header>

      {preview.blurb.length ? (
        <div className="hover-preview-blurb">
          <Nodes nodes={preview.blurb} />
        </div>
      ) : (
        <p className="hover-preview-empty">No documentation.</p>
      )}

      {preview.facts.length ? (
        <ul className="hover-preview-facts">
          {preview.facts.map((fact) => (
            <li key={fact}>{fact}</li>
          ))}
        </ul>
      ) : null}
    </div>,
    document.body,
  );
}
