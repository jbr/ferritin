import createClient from "openapi-fetch";
import type { components, paths } from "./schema.gen";

/** The structured documentation model for a single item. */
export type Item = components["schemas"]["JsonItem"];
/** Search results for a crate-scoped query. */
export type SearchResponse = components["schemas"]["JsonSearch"];
/** A failed lookup, carrying "did you mean" suggestions. */
export type NotFound = components["schemas"]["JsonNotFound"];
/** Crate search: top matches by evidence tier and download rank, plus the total. */
export type CrateSearchResponse = components["schemas"]["JsonCrateSearch"];

/**
 * Low-level typed transport, generated from the OpenAPI document. The app and the
 * API share one origin (the trillium server, which serves this client via
 * trillium-frontend); the JSON API is mounted under `/api` (the OpenAPI `servers`
 * entry). Same-origin, no CORS.
 *
 * baseUrl is `origin + /api` rather than a bare `/api` so that URL resolution also
 * works outside a browser document (jsdom tests, where a relative `Request` has no
 * base) — behaviorally identical to same-origin in the browser.
 *
 * Everything below this line is the encapsulation seam: callers import the domain
 * functions, not the routes, so the endpoint shape can change without touching the UI.
 */
const http = createClient<paths>({
  baseUrl: `${window.location.origin}/api`,
  // Resolve the ambient fetch at call time rather than letting openapi-fetch
  // capture it at construction, so tests can stub `globalThis.fetch` (and any
  // runtime fetch wrapper is respected).
  fetch: (input: RequestInfo | URL, init?: RequestInit) =>
    globalThis.fetch(input, init),
});

/** An API call that resolved to a non-success status. */
export class ApiError extends Error {
  readonly status: number;
  /** Present when the failure was a structured not-found result. */
  readonly notFound?: NotFound;

  constructor(message: string, status: number, notFound?: NotFound) {
    super(message);
    this.name = "ApiError";
    this.status = status;
    this.notFound = notFound;
  }
}

/**
 * Fetch the documentation model for an item path (e.g. `serde::Deserialize`).
 */
export async function fetchItem(path: string): Promise<Item> {
  const result = await http.GET("/crates/{crate_name}", {
    params: { path: { crate_name: path } },
  });
  // Read the status before narrowing: openapi-fetch types the result as a union of
  // "has data" | "has error", so inside the malformed branch below it considers
  // `response` unreachable (`never`) — the very case we are guarding against.
  const status = result.response.status;

  if (result.error) {
    const notFound = isNotFound(result.error) ? result.error : undefined;
    throw new ApiError(
      notFound ? `Not found: ${path}` : `Failed to load ${path}`,
      status,
      notFound,
    );
  }
  // A response openapi-fetch can parse as neither success nor error (a non-JSON
  // body — an HTML error page from a proxy, say) leaves *both* `data` and `error`
  // undefined. Returning that would hand back `undefined` while promising an
  // `Item`, and the caller renders a blank page instead of an error. Fail loudly.
  if (!result.data) {
    throw new ApiError(`Malformed response for ${path}`, status);
  }
  return result.data;
}

/**
 * Search within a single crate. The endpoint only declares a 2xx response (a
 * `JsonSearch` carries its own error states in-band), so we branch on `data`
 * rather than the response `error`, which is typed `never` here.
 */
export async function search(
  crate: string,
  q: string,
): Promise<SearchResponse> {
  const { data, response } = await http.GET("/search/{crate_name}", {
    params: { path: { crate_name: crate }, query: { q } },
  });
  if (!data) {
    throw new ApiError(`Search failed in ${crate}`, response.status);
  }
  return data;
}

/**
 * Crate search (as-you-type: the server matches tokens as prefixes and falls
 * back to fuzzy matching, so this doubles as typeahead). Like search, only a
 * 2xx is declared; a 503 (the server hasn't loaded the crate-names artifact
 * yet) surfaces as a thrown ApiError, which the UI treats the same as "no
 * suggestions".
 */
export async function searchCrates(q: string): Promise<CrateSearchResponse> {
  const { data, response } = await http.GET("/crates", {
    params: { query: { q } },
  });
  if (!data) {
    throw new ApiError("Crate search failed", response.status);
  }
  return data;
}

function isNotFound(value: unknown): value is NotFound {
  if (typeof value !== "object" || value === null) return false;
  // Both discriminants carry a `JsonNotFound` payload the UI can render: a plain
  // miss (`notFound`, with suggestions) and a crate that exists on crates.io but
  // whose docs we can't serve (`crateUnavailable`, with `unavailableCrate`).
  const error = (value as { error?: string }).error;
  return error === "notFound" || error === "crateUnavailable";
}
