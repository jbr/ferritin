import { useQuery } from "@tanstack/react-query";
import { fetchItem, search } from "./client";

/** Documentation model for an item path; disabled when the path is empty. */
export function useItem(path: string) {
  return useQuery({
    queryKey: ["item", path],
    queryFn: () => fetchItem(path),
    enabled: path.length > 0,
  });
}

/** Crate-scoped search; disabled until both crate and query are present. */
export function useSearch(crate: string, q: string) {
  return useQuery({
    queryKey: ["search", crate, q],
    queryFn: () => search(crate, q),
    enabled: crate.length > 0 && q.length > 0,
  });
}
