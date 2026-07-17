import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import { Router } from "rhoto-router";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createQueryClient } from "../api/queryClient";
import { ItemView } from "./ItemView";
import notFoundFixture from "../test/fixtures/not-found.json";
import crateUnavailableFixture from "../test/fixtures/crate-unavailable.json";

/**
 * A 404 must render the not-found view with its suggestions. It previously rendered
 * *nothing*: the query never reached an error state, so every branch of `ItemView`'s
 * loading/error/data ternary was false and it returned `null` — a blank page for any
 * mistyped path.
 */
function renderAt(path: string) {
  // The app's own client (not a test-tuned one), so retry behaviour here is the
  // behaviour in the browser.
  return render(
    <QueryClientProvider client={createQueryClient()}>
      <Router>
        <ItemView path={path} />
      </Router>
    </QueryClientProvider>,
  );
}

afterEach(() => vi.unstubAllGlobals());

describe("ItemView", () => {
  it("shows the not-found view and suggestions on a 404", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(
        async () =>
          new Response(JSON.stringify(notFoundFixture), {
            status: 404,
            headers: { "content-type": "application/json" },
          }),
      ),
    );

    renderAt("std::vec::Voc");

    // Deliberately tight: a 404 must not be retried. With React Query's default
    // retry policy this takes ~7s of exponential backoff — during which the page
    // is blank — so a generous timeout here would hide exactly the bug we care about.
    await waitFor(
      () => expect(screen.getByText(/No item at/)).toBeInTheDocument(),
      { timeout: 1000 },
    );
    expect(screen.getByText(/did you mean/i)).toBeInTheDocument();
    // Suggestions render as in-app links, so a mistyped path is one click from the
    // right one — that is the whole point of the not-found view.
    const suggestion = screen.getByRole("link", { name: "std::vec::Drain" });
    expect(suggestion).toHaveAttribute("href", "/std::vec::Drain");
  });

  it("names an existing-but-unavailable crate instead of offering suggestions", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(
        async () =>
          new Response(JSON.stringify(crateUnavailableFixture), {
            status: 404,
            headers: { "content-type": "application/json" },
          }),
      ),
    );

    renderAt("ripgrep");

    await waitFor(
      () =>
        expect(
          screen.getByText(/Documentation unavailable for ripgrep/),
        ).toBeInTheDocument(),
      { timeout: 1000 },
    );
    // It is a real crate, not a typo — so no "did you mean" list.
    expect(screen.queryByText(/did you mean/i)).not.toBeInTheDocument();
  });
});
