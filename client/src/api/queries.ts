import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { fetchItem, search, typeahead } from "./client";

/**
 * Documentation model for an item path; disabled when the path is empty, or when
 * `enabled` is false — the caller fetches lazily (an expand-on-demand affordance
 * that only wants the item once the reader asks for it).
 */
export function useItem(path: string, enabled = true) {
  return useQuery({
    queryKey: ["item", path],
    queryFn: () => fetchItem(path),
    enabled: enabled && path.length > 0,
  });
}

/**
 * Crate-scoped search; disabled until both crate and query are present.
 *
 * Previous results are held while the next query is in flight, so a typeahead list
 * never blanks out between keystrokes — it updates in place.
 */
export function useSearch(crate: string, q: string) {
  return useQuery({
    queryKey: ["search", crate, q],
    queryFn: () => search(crate, q),
    enabled: crate.length > 0 && q.length > 0,
    placeholderData: keepPreviousData,
  });
}

/**
 * Crate-name typeahead; disabled until a prefix is present. Same
 * keep-previous-data behavior as search, for the same reason.
 */
export function useTypeahead(q: string) {
  return useQuery({
    queryKey: ["typeahead", q],
    queryFn: () => typeahead(q),
    enabled: q.length > 0,
    placeholderData: keepPreviousData,
  });
}
