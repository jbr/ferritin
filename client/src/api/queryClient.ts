import { QueryClient } from "@tanstack/react-query";
import { ApiError } from "./client";

/**
 * The app's React Query configuration. Shared by `main.tsx` and the tests, so a
 * test exercises the same retry behaviour the browser does — the two drifting apart
 * is how a "passing" test coexists with a broken page.
 */
export function createQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: {
        // Never retry a 4xx. "No such item" is a deterministic answer, not a blip:
        // retrying it three times (React Query's default, with exponential backoff)
        // just delays the not-found view by ~7 seconds while the page sits empty.
        // Server and network failures still get the default retries.
        retry: (failureCount, error) =>
          !(
            error instanceof ApiError &&
            error.status >= 400 &&
            error.status < 500
          ) && failureCount < 3,
      },
    },
  });
}
