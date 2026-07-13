/** Both Meta+K and Ctrl+K open the search everywhere; this only decides which one
 * we *name* in the UI. */
const isApple =
  typeof navigator !== "undefined" &&
  /Mac|iPhone|iPad|iPod/.test(navigator.platform || navigator.userAgent);

export const searchShortcut = isApple ? "⌘K" : "Ctrl K";
export const searchShortcutAria = isApple ? "Meta+K" : "Control+K";
