import { useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useRouter } from "rhoto-router";
import { useSearch, useTypeahead } from "../../api/queries";
import { itemHref } from "../../lib/paths";
import { searchShortcut, searchShortcutAria } from "../../lib/platform";

/** Typeahead debounce: long enough to skip intermediate keystrokes, short
 * enough that the list feels attached to the keyboard. */
const DEBOUNCE_MS = 130;

/**
 * Where the chip sits in the two-step removal. Scoping is a *destructive* edit
 * (it throws away the crate you were searching), and people overshoot when
 * backspacing through a query — so the first Backspace on an empty input only
 * *selects* the chip, and a second one removes it. Tab and ArrowLeft select it
 * too; typing, ArrowRight, or Escape put it back.
 */
type ChipState = "none" | "idle" | "selected";

/**
 * The top-bar search: **one persistent element in two states**, not a trigger
 * that opens a modal.
 *
 * Resting, it is honestly a *button* — it never masquerades as a text field, which
 * is what made the old trigger a sham. Focused, that same element morphs into the
 * search surface: its top and right edges stay pinned while it grows leftward and
 * downward, its background cross-fades, and the results (mounted all along, merely
 * clipped by `overflow: hidden`) are revealed.
 *
 * The **scope chip** (`in std ×`) makes the search's scope visible in both states.
 * Removing it drops into *crate mode*: the field addresses crates rather than items.
 * That also gives the crate-less home page a live search box for the first time —
 * no chip simply *is* crate mode.
 */
export function SearchBox({ crate }: { crate?: string }) {
  const [expanded, setExpanded] = useState(false);
  const [query, setQuery] = useState("");
  const [debounced, setDebounced] = useState("");
  const [selected, setSelected] = useState(0);
  const [scoped, setScoped] = useState(true);
  const [chipSelected, setChipSelected] = useState(false);

  const inputRef = useRef<HTMLInputElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const listRef = useRef<HTMLUListElement>(null);
  const restoreFocus = useRef(false);
  const { navigate } = useRouter();

  const listId = useId();
  const optionId = (i: number) => `${listId}-option-${i}`;

  // The chip exists only when there is a crate to scope *to* and the user has not
  // scoped out of it. Everything else keys off this one derived value.
  const chip: ChipState = !crate || !scoped ? "none" : chipSelected ? "selected" : "idle";
  const crateMode = chip === "none";

  const { data, isFetching } = useSearch(crateMode ? "" : (crate ?? ""), debounced);
  const results = crateMode ? [] : (data?.results ?? []);

  // Crate mode gets crate-name typeahead (the /api/typeahead endpoint, backed
  // by the daily crate-names artifact) instead of item search.
  const { data: crateData, isFetching: crateFetching } = useTypeahead(
    crateMode ? debounced.trim() : "",
  );
  const crateResults = crateMode ? (crateData?.results ?? []) : [];
  const listLength = crateMode ? crateResults.length : results.length;

  useEffect(() => {
    const timer = setTimeout(() => setDebounced(query), DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [query]);

  // Navigating re-scopes to wherever you landed: the chip follows the page.
  useEffect(() => {
    setScoped(true);
    setChipSelected(false);
  }, [crate]);

  const open = () => setExpanded(true);

  const close = () => {
    setExpanded(false);
    setQuery("");
    setDebounced("");
    setSelected(0);
    setScoped(true);
    setChipSelected(false);
    restoreFocus.current = true;
  };

  // Move focus in an effect, never inline or in a rAF: the element being focused is
  // the one the state change is about to mount (input on open, button on close), so
  // it does not exist until React has committed. A `requestAnimationFrame` is not a
  // substitute — it can run before the commit and silently no-op.
  useEffect(() => {
    if (expanded) {
      inputRef.current?.focus();
    } else if (restoreFocus.current) {
      restoreFocus.current = false;
      buttonRef.current?.focus();
    }
  }, [expanded]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        if (expanded) close();
        else open();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  useEffect(() => {
    if (!expanded) return;
    const previous = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    return () => {
      document.body.style.overflow = previous;
    };
  }, [expanded]);

  useEffect(() => {
    listRef.current
      ?.querySelector(`#${CSS.escape(optionId(selected))}`)
      ?.scrollIntoView({ block: "nearest" });
  }, [selected, optionId]);

  /** Drop the scope: the field now addresses crates, not items. */
  const unscope = () => {
    setScoped(false);
    setChipSelected(false);
    setQuery("");
    setDebounced("");
    inputRef.current?.focus();
  };

  const goToItem = (path: string) => {
    close();
    navigate(itemHref(path));
  };

  /** Open a crate by name — a typeahead suggestion, or exactly what was typed. */
  const goToCrate = (name: string) => {
    const target = name.trim();
    if (!target) return;
    close();
    navigate(itemHref(target));
  };

  const onKeyDown = (event: React.KeyboardEvent) => {
    // Chip selected: it owns the keyboard until it is removed or deselected.
    if (chip === "selected") {
      switch (event.key) {
        case "Backspace":
        case "Delete":
          event.preventDefault();
          unscope();
          return;
        case "ArrowRight":
        case "Escape":
          event.preventDefault();
          setChipSelected(false);
          return;
      }
      // Any other key means they meant to type: deselect and let it through.
      setChipSelected(false);
    }

    const atStart =
      inputRef.current?.selectionStart === 0 &&
      inputRef.current?.selectionEnd === 0;

    switch (event.key) {
      case "Escape":
        close();
        break;
      case "Tab":
        // The panel is the whole interactive surface, so Tab is free to mean
        // "reach for the chip" rather than "leave".
        event.preventDefault();
        if (chip === "idle") setChipSelected(true);
        break;
      case "Backspace":
        if (chip === "idle" && !query) {
          event.preventDefault();
          setChipSelected(true);
        }
        break;
      case "ArrowLeft":
        if (chip === "idle" && atStart) {
          event.preventDefault();
          setChipSelected(true);
        }
        break;
      case "ArrowDown":
        event.preventDefault();
        setSelected((i) => Math.min(i + 1, listLength - 1));
        break;
      case "ArrowUp":
        event.preventDefault();
        setSelected((i) => Math.max(i - 1, 0));
        break;
      case "Home":
        event.preventDefault();
        setSelected(0);
        break;
      case "End":
        event.preventDefault();
        setSelected(Math.max(listLength - 1, 0));
        break;
      case "Enter":
        event.preventDefault();
        // In crate mode the selected suggestion wins; free-typed text is the
        // fallback so a name the artifact doesn't know yet still navigates.
        if (crateMode) goToCrate(crateResults[selected]?.name ?? query);
        else if (results[selected]) goToItem(results[selected].path);
        break;
    }
  };

  const restingLabel = crate ? `Search ${crate}…` : "Search…";
  const placeholder = crateMode ? "Go to crate…" : "Search…";

  return (
    <div className="search-slot">
      {expanded
        ? createPortal(
            <div className="search-backdrop" onMouseDown={close} />,
            document.body,
          )
        : null}

      <div className="search-box" data-expanded={expanded || undefined}>
        {expanded ? (
          <div className="search-row">
            <span className="search-icon" aria-hidden>
              ⌕
            </span>

            {chip !== "none" ? (
              <span
                className={chip === "selected" ? "scope-chip selected" : "scope-chip"}
                // Announced as one unit; the × is the mouse equivalent of the
                // Backspace-Backspace path.
                aria-label={`Scoped to ${crate}`}
              >
                in {crate}
                <button
                  type="button"
                  className="scope-chip-x"
                  aria-label={`Search all crates instead of ${crate}`}
                  tabIndex={-1}
                  onMouseDown={(event) => {
                    event.preventDefault();
                    unscope();
                  }}
                >
                  ×
                </button>
              </span>
            ) : null}

            <input
              ref={inputRef}
              className="search-input"
              role="combobox"
              aria-expanded
              aria-controls={listId}
              aria-activedescendant={
                (crateMode ? crateResults[selected] : results[selected])
                  ? optionId(selected)
                  : undefined
              }
              aria-label={crateMode ? "Go to crate" : `Search ${crate}`}
              value={query}
              onChange={(event) => {
                setQuery(event.target.value);
                setSelected(0);
                setChipSelected(false);
              }}
              onKeyDown={onKeyDown}
              placeholder={placeholder}
              autoComplete="off"
              // Crate and item names are not prose: a phone keyboard would
              // otherwise capitalize the first letter and autocorrect the rest.
              autoCapitalize="none"
              autoCorrect="off"
              spellCheck={false}
            />

            <kbd className="search-kbd">{chip === "idle" ? "⇥ scope" : "esc"}</kbd>
          </div>
        ) : (
          <button
            ref={buttonRef}
            type="button"
            className="search-row search-trigger"
            aria-expanded={false}
            aria-haspopup="listbox"
            aria-keyshortcuts={searchShortcutAria}
            onClick={open}
          >
            <span className="search-icon" aria-hidden>
              ⌕
            </span>
            {crate ? (
              <span className="scope-chip">in {crate}</span>
            ) : (
              <span className="search-label">{restingLabel}</span>
            )}
            <span className="search-label-spacer" />
            <kbd className="search-kbd">{searchShortcut}</kbd>
          </button>
        )}

        {/* Mounted from the start; clipped by the box until it expands. */}
        <div className="search-panel">
          {crateMode ? (
            crateResults.length ? (
              <ul
                className="search-results"
                id={listId}
                role="listbox"
                ref={listRef}
              >
                {crateResults.map((result, i) => (
                  <li key={result.name} role="presentation">
                    <button
                      type="button"
                      id={optionId(i)}
                      role="option"
                      aria-selected={i === selected}
                      className={
                        i === selected
                          ? "search-result selected"
                          : "search-result"
                      }
                      tabIndex={-1}
                      onMouseEnter={() => setSelected(i)}
                      onMouseDown={(event) => {
                        event.preventDefault();
                        goToCrate(result.name);
                      }}
                    >
                      <span className="search-path mono">{result.name}</span>
                      <span className="search-version mono">
                        {result.version}
                      </span>
                    </button>
                  </li>
                ))}
              </ul>
            ) : (
              <p className="search-empty">
                {!debounced.trim() ? (
                  "Type a crate name."
                ) : crateFetching ? (
                  "Searching…"
                ) : (
                  <>
                    No crates start with{" "}
                    <span className="mono">{debounced.trim()}</span> — press{" "}
                    <kbd className="search-kbd">⏎</kbd> to try it anyway.
                  </>
                )}
              </p>
            )
          ) : (
            <>
              {results.length ? (
                <>
                  <p className="search-group">in {crate}</p>
                  <ul
                    className="search-results"
                    id={listId}
                    role="listbox"
                    ref={listRef}
                  >
                    {results.map((result, i) => (
                      <li key={result.path} role="presentation">
                        <button
                          type="button"
                          id={optionId(i)}
                          role="option"
                          aria-selected={i === selected}
                          className={
                            i === selected
                              ? "search-result selected"
                              : "search-result"
                          }
                          tabIndex={-1}
                          onMouseEnter={() => setSelected(i)}
                          // mousedown, not click: the input's blur would otherwise
                          // collapse the box out from under the pointer first.
                          onMouseDown={(event) => {
                            event.preventDefault();
                            goToItem(result.path);
                          }}
                        >
                          <span className={`search-kind kind-${result.kind}`}>
                            {result.kind}
                          </span>
                          <span className="search-path mono">{result.path}</span>
                        </button>
                      </li>
                    ))}
                  </ul>
                </>
              ) : (
                <p className="search-empty">
                  {!debounced
                    ? `Search ${crate} by name or documentation.`
                    : isFetching
                      ? "Searching…"
                      : `No matches for “${debounced}”.`}
                </p>
              )}
            </>
          )}

          <div className="search-footer">
            {/* Named keys only: hidden on touch, where none of them exist. */}
            <span className="search-hints">
              {crateMode
                ? crateResults.length
                  ? "↑↓ navigate · ⏎ open"
                  : "⏎ open crate"
                : "↑↓ navigate · ⏎ open · ⇥ scope"}
            </span>
            <span>
              {crateMode
                ? crateData && debounced.trim()
                  ? crateData.total > crateResults.length
                    ? `${crateResults.length} of ${crateData.total.toLocaleString()} crates`
                    : `${crateData.total} ${crateData.total === 1 ? "crate" : "crates"}`
                  : null
                : debounced
                  ? `${results.length} results`
                  : null}
            </span>
          </div>
        </div>
      </div>
    </div>
  );
}
