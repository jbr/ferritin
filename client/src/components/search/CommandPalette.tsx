import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useRouter } from "rhoto-router";
import { useSearch } from "../../api/queries";
import { itemHref } from "../../lib/paths";

/**
 * The ⌘K search palette. Renders the top-bar search trigger and, when open, a
 * centered modal that searches the current crate and navigates to a result.
 * Crate-scoped because the search endpoint is (`/api/search/{crate}`); without a
 * crate in context the trigger is disabled.
 */
export function CommandPalette({ crate }: { crate?: string }) {
  const [open, setOpen] = useState(false);

  // Global ⌘K / Ctrl+K to open, Escape handled inside the modal.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setOpen((v) => !v);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <>
      <button
        type="button"
        className="search-trigger"
        onClick={() => setOpen(true)}
        disabled={!crate}
      >
        <span className="search-trigger-icon" aria-hidden>
          ⌕
        </span>
        <span className="search-trigger-label">
          {crate ? `Search ${crate}…` : "Search…"}
        </span>
        <kbd className="search-trigger-kbd">⌘K</kbd>
      </button>
      {open && crate ? (
        <PaletteModal crate={crate} onClose={() => setOpen(false)} />
      ) : null}
    </>
  );
}

function PaletteModal({
  crate,
  onClose,
}: {
  crate: string;
  onClose: () => void;
}) {
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const { navigate } = useRouter();
  const { data } = useSearch(crate, query);
  const results = data?.results ?? [];

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  const go = (path: string) => {
    navigate(itemHref(path));
    onClose();
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") {
      onClose();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      setSelected((i) => Math.min(i + 1, results.length - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setSelected((i) => Math.max(i - 1, 0));
    } else if (e.key === "Enter" && results[selected]) {
      e.preventDefault();
      go(results[selected].path);
    }
  };

  // Portal to <body>: the topbar has `backdrop-filter`, which makes it the
  // containing block for fixed-position descendants — rendering the overlay
  // inline would clip its `inset: 0` (and its blur) to the topbar's box.
  return createPortal(
    <div className="palette-overlay" onMouseDown={onClose}>
      <div
        className="palette"
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={onKeyDown}
      >
        <div className="palette-input-row">
          <span className="palette-icon" aria-hidden>
            ⌕
          </span>
          <input
            ref={inputRef}
            className="palette-input"
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setSelected(0);
            }}
            placeholder={`Search ${crate}…`}
            aria-label={`Search ${crate}`}
          />
          <kbd className="palette-esc">esc</kbd>
        </div>
        <ul className="palette-results">
          {results.map((result, i) => (
            <li
              key={result.path}
              className={
                i === selected ? "palette-result selected" : "palette-result"
              }
              onMouseEnter={() => setSelected(i)}
              onClick={() => go(result.path)}
            >
              <span className="palette-kind">{result.kind}</span>
              <span className="palette-path mono">{result.path}</span>
              <span className="palette-crate">{crate}</span>
            </li>
          ))}
        </ul>
        <div className="palette-footer">
          <span>↑↓ navigate</span>
          <span>↵ open</span>
          <span className="palette-count">
            {query ? `${results.length} results` : ""}
          </span>
        </div>
      </div>
    </div>,
    document.body,
  );
}
