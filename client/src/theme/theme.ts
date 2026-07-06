/**
 * Theme persistence. The active theme lives on `<html data-theme>`; the choice is
 * stored in localStorage and otherwise follows the OS `prefers-color-scheme`.
 * Kept framework-free so `main.tsx` can apply it before first paint (no flash).
 */

export type Theme = "light" | "dark";

const STORAGE_KEY = "ferritin-theme";

/** The stored theme, or the OS preference when nothing is stored. */
export function initialTheme(): Theme {
  const stored = localStorage.getItem(STORAGE_KEY);
  if (stored === "light" || stored === "dark") {
    return stored;
  }
  return window.matchMedia("(prefers-color-scheme: dark)").matches
    ? "dark"
    : "light";
}

/** Reflect a theme onto the document root. Light is the default (no attribute). */
export function applyTheme(theme: Theme): void {
  const root = document.documentElement;
  if (theme === "dark") {
    root.dataset.theme = "dark";
  } else {
    delete root.dataset.theme;
  }
}

/** Persist and apply a theme. */
export function setTheme(theme: Theme): void {
  localStorage.setItem(STORAGE_KEY, theme);
  applyTheme(theme);
}
