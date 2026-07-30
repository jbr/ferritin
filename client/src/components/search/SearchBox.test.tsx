import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Router } from "rhoto-router";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createQueryClient } from "../../api/queryClient";
import { SearchBox } from "./SearchBox";

/**
 * The scope chip is the search's only visible statement of *what it searches*, so
 * the two transitions that change it are the ones worth pinning: unscoping (which
 * drops into crate mode) and navigating to another crate (which must re-scope).
 *
 * The re-scope is a prop-change reset performed during render rather than in an
 * effect. That is easy to regress back into an effect, which would leave one
 * committed frame showing the *previous* crate's scope.
 */
function renderBox(crate?: string) {
  return render(
    <QueryClientProvider client={createQueryClient()}>
      <Router>
        <SearchBox crate={crate} />
      </Router>
    </QueryClientProvider>,
  );
}

/** Stub `fetch` so crate search and item search return canned JSON keyed off the URL. */
function stubApi(handler: (url: string) => unknown) {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL) => {
      const url = input instanceof Request ? input.url : String(input);
      return new Response(JSON.stringify(handler(url)), {
        status: 200,
        headers: { "content-type": "application/json" },
      });
    }),
  );
}

afterEach(() => vi.unstubAllGlobals());

describe("SearchBox", () => {
  it("restores the scope chip when navigating to a different crate", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(
        async () =>
          new Response(JSON.stringify({ results: [], total: 0 }), {
            status: 200,
            headers: { "content-type": "application/json" },
          }),
      ),
    );

    const user = userEvent.setup();
    const { rerender } = renderBox("std");

    // The resting element is honestly a button; its accessible name is the chip.
    await user.click(screen.getByRole("button", { expanded: false }));
    expect(await screen.findByText("in std")).toBeInTheDocument();

    // Backspace on an empty input selects the chip; a second one removes it,
    // dropping into crate mode — the field now addresses crates, not items.
    await user.keyboard("{Backspace}{Backspace}");
    await waitFor(() =>
      expect(
        screen.getByRole("combobox", { name: /go to crate/i }),
      ).toBeInTheDocument(),
    );
    expect(screen.queryByText("in std")).not.toBeInTheDocument();

    // Landing on another crate re-scopes: the chip follows the page.
    rerender(
      <QueryClientProvider client={createQueryClient()}>
        <Router>
          <SearchBox crate="serde" />
        </Router>
      </QueryClientProvider>,
    );

    expect(await screen.findByText("in serde")).toBeInTheDocument();
  });

  /**
   * The mirror of unscoping: from crate mode, choosing a crate re-scopes *into*
   * it. Tab on the highlighted suggestion promotes it to the chip and the field
   * switches back to item search — the keyboard path for hopping between crates
   * without leaving the field. This works even with no page crate at all.
   */
  it("promotes a highlighted crate to the chip on Tab", async () => {
    stubApi((url) =>
      url.includes("/crates?")
        ? { results: [{ name: "serde", version: "1.0.0" }], total: 1 }
        : { results: [], total: 0 },
    );

    const user = userEvent.setup();
    renderBox(); // no page crate — the field opens straight into crate mode.

    await user.click(screen.getByRole("button", { expanded: false }));
    await user.keyboard("serde");

    // The suggestion arrives highlighted; Tab scopes into it.
    await screen.findByRole("option", { name: /serde/ });
    await user.keyboard("{Tab}");

    expect(await screen.findByText("in serde")).toBeInTheDocument();
    // Item search, scoped to the picked crate — no longer crate mode.
    expect(
      screen.getByRole("combobox", { name: /search serde/i }),
    ).toBeInTheDocument();
  });

  /**
   * The typed shorthand for the same move: in crate mode a `::` is a scope
   * gesture, not text — `tokio::` makes `tokio` the chip and carries anything
   * after the separator into the item query.
   */
  it("scopes into the crate when `::` is typed", async () => {
    stubApi((url) =>
      url.includes("/crates?")
        ? { results: [{ name: "tokio", version: "1.0.0" }], total: 1 }
        : { results: [], total: 0 },
    );

    const user = userEvent.setup();
    renderBox();

    await user.click(screen.getByRole("button", { expanded: false }));
    await user.keyboard("tokio::");

    expect(await screen.findByText("in tokio")).toBeInTheDocument();
    expect(
      screen.getByRole("combobox", { name: /search tokio/i }),
    ).toBeInTheDocument();
  });

  /**
   * Both queries hold their previous results as a placeholder so the list never
   * blanks between keystrokes — but an emptied input is a *cleared* search, not a
   * pause, so the held results must collapse with it rather than lingering under
   * the chip.
   */
  it("clears the results when the query is emptied", async () => {
    stubApi((url) =>
      url.includes("/search/")
        ? { results: [{ path: "tokio::spawn", kind: "function" }], total: 1 }
        : { results: [], total: 0 },
    );

    const user = userEvent.setup();
    renderBox("tokio");

    await user.click(screen.getByRole("button", { expanded: false }));
    await user.keyboard("spawn");
    expect(await screen.findByText("tokio::spawn")).toBeInTheDocument();

    await user.keyboard("{Backspace>5/}");
    await waitFor(() =>
      expect(screen.queryByText("tokio::spawn")).not.toBeInTheDocument(),
    );
    expect(
      screen.getByText(/search tokio by name or documentation/i),
    ).toBeInTheDocument();
  });
});
