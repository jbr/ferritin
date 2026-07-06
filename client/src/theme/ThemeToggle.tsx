import { useState } from "react";
import { initialTheme, setTheme, type Theme } from "./theme";

/** The light/dark switch in the top bar. Mirrors the design's pill toggle. */
export function ThemeToggle() {
  const [theme, setThemeState] = useState<Theme>(initialTheme);

  const toggle = () => {
    const next: Theme = theme === "dark" ? "light" : "dark";
    setTheme(next);
    setThemeState(next);
  };

  return (
    <button
      type="button"
      className="theme-toggle"
      onClick={toggle}
      aria-label={`Switch to ${theme === "dark" ? "light" : "dark"} theme`}
      aria-pressed={theme === "dark"}
      data-theme-state={theme}
    >
      <span className="theme-toggle-knob" />
    </button>
  );
}
