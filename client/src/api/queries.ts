import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { fetchItem, search, searchCrates } from "./client";

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
 * Previous results are held while the next query is in flight, so the list never
 * blanks out between keystrokes — it updates in place. That hold is deliberately
 * scoped to a *single crate*: re-scoping from one crate to another (or an empty
 * query while a new chip settles) must not leave the old crate's items showing
 * under the new scope, so the placeholder is dropped the moment the crate changes.
 */
export function useSearch(crate: string, q: string) {
  return useQuery({
    queryKey: ["search", crate, q],
    queryFn: () => search(crate, q),
    enabled: crate.length > 0 && q.length > 0,
    placeholderData: (previous, previousQuery) =>
      previousQuery?.queryKey[1] === crate ? previous : undefined,
  });
}

/**
 * Crate-name typeahead (the as-you-type crate-search endpoint); disabled until
 * a prefix is present. Same keep-previous-data behavior as search, for the
 * same reason.
 */
export function useTypeahead(q: string) {
  return useQuery({
    queryKey: ["crate-search", q],
    queryFn: () => searchCrates(q),
    enabled: q.length > 0,
    placeholderData: keepPreviousData,
  });
}
