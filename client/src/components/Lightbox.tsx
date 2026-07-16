import { useRef } from "react";

/**
 * A click-to-enlarge image.
 *
 * Built on `<dialog>` + `showModal()` rather than a hand-rolled overlay, because
 * the platform already implements the four things this pattern is easy to get
 * subtly wrong: the focus trap, Escape to dismiss, `inert` on everything behind,
 * and returning focus to the trigger on close. It also renders in the top layer,
 * so it cannot lose a z-index argument with the hover preview or the search
 * morph — the two overlays this app already has.
 *
 * `width`/`height` are the image's intrinsic pixels. They are what reserve the
 * right box before the image decodes, so the page doesn't jump.
 */
export function Lightbox({
  src,
  alt,
  width,
  height,
}: {
  src: string;
  alt: string;
  width: number;
  height: number;
}) {
  const dialog = useRef<HTMLDialogElement>(null);

  return (
    <>
      {/* The button carries the description *and* the action, so the image
          inside it is presentational — an `alt` here would only make a screen
          reader say the same sentence twice. In the dialog the image is the
          content, so it keeps its `alt`. */}
      <button
        type="button"
        className="lightbox-trigger"
        aria-label={`Enlarge: ${alt}`}
        onClick={() => dialog.current?.showModal()}
      >
        <img src={src} alt="" width={width} height={height} />
      </button>

      <dialog
        ref={dialog}
        className="lightbox"
        // The backdrop is the dialog's own box, so a click lands on the dialog
        // itself only when it misses the image — "click outside to close" with
        // no extra element to catch it.
        onClick={(event) => {
          if (event.target === dialog.current) dialog.current.close();
        }}
      >
        <button
          type="button"
          className="lightbox-close"
          aria-label="Close"
          onClick={() => dialog.current?.close()}
        >
          ✕
        </button>
        <img src={src} alt={alt} width={width} height={height} />
      </dialog>
    </>
  );
}
