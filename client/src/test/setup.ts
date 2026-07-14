// Extends `expect` with jest-dom matchers (toBeInTheDocument, etc.) and unmounts
// React trees between tests. We don't use Vitest globals, so cleanup is wired
// explicitly rather than via an auto-registered global `afterEach`.
import "@testing-library/jest-dom/vitest";
import { afterEach } from "vitest";
import { cleanup } from "@testing-library/react";

// jsdom exposes a `localStorage` whose methods are not callable under this runner,
// so anything mounting the app shell (the theme toggle reads it on first render)
// dies before it can assert anything. A trivial in-memory store is enough.
if (typeof localStorage?.getItem !== "function") {
  const store = new Map<string, string>();
  Object.defineProperty(globalThis, "localStorage", {
    configurable: true,
    value: {
      getItem: (key: string) => store.get(key) ?? null,
      setItem: (key: string, value: string) =>
        void store.set(key, String(value)),
      removeItem: (key: string) => void store.delete(key),
      clear: () => store.clear(),
      key: (i: number) => [...store.keys()][i] ?? null,
      get length() {
        return store.size;
      },
    },
  });
}

// Same story for `matchMedia`, which the theme reads to pick up the OS preference.
if (typeof window !== "undefined" && !window.matchMedia) {
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    value: (query: string) => ({
      matches: false,
      media: query,
      onchange: null,
      addEventListener: () => {},
      removeEventListener: () => {},
      addListener: () => {},
      removeListener: () => {},
      dispatchEvent: () => false,
    }),
  });
}

afterEach(cleanup);
