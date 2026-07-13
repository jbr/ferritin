import { useEffect } from "react";

/** Shown alone on the landing page, and as the suffix everywhere else. */
const SITE = "ferritin";

/**
 * Set `document.title` for the current view.
 *
 * The effect is keyed on the title string, so it writes once per distinct value
 * and not on every render — no guard ref needed, the dependency array *is* the
 * guard.
 *
 * Titles lead with the item path rather than the site name because tab titles
 * truncate from the right: with several tabs open you see the first few
 * characters, and a `ferritin: …` prefix would spend all of them on the one word
 * every page shares. `std::io::Read — ferritin` stays distinguishable where
 * `ferritin: std::io::…` does not. (rustdoc makes the same call.)
 */
export function useDocumentTitle(title: string | undefined) {
  const full = title ? `${title} — ${SITE}` : SITE;
  useEffect(() => {
    document.title = full;
  }, [full]);
}
