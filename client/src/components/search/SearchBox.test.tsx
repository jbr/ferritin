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
});
