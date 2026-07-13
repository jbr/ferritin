import { afterEach, describe, expect, it, vi } from "vitest";
import { ApiError, fetchItem } from "./client";
import functionFixture from "../test/fixtures/function.json";
import notFoundFixture from "../test/fixtures/not-found.json";

/** Stub global fetch (what openapi-fetch calls) with a canned JSON response. */
function mockFetch(body: unknown, status = 200) {
  vi.stubGlobal(
    "fetch",
    vi.fn(
      async () =>
        new Response(JSON.stringify(body), {
          status,
          headers: { "content-type": "application/json" },
        }),
    ),
  );
}

/** Stub global fetch with a non-JSON body — what a proxy or gateway returns when
 * it swallows the real response (an HTML error page). */
function mockHtmlFetch(status: number) {
  vi.stubGlobal(
    "fetch",
    vi.fn(
      async () =>
        new Response("<!doctype html><title>starting up…</title>", {
          status,
          headers: { "content-type": "text/html; charset=utf-8" },
        }),
    ),
  );
}

afterEach(() => vi.unstubAllGlobals());

describe("fetchItem", () => {
  it("returns the parsed item model on 200", async () => {
    mockFetch(functionFixture);
    const item = await fetchItem("std::mem::swap");
    expect(item.meta.name).toBe("swap");
    expect(item.body.kind).toBe("function");
  });

  it("throws ApiError carrying the not-found payload on 404", async () => {
    mockFetch(notFoundFixture, 404);
    const error = await fetchItem("std::vec::Voc").catch((e) => e);
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).status).toBe(404);
    expect((error as ApiError).notFound?.suggestions?.length).toBeGreaterThan(
      0,
    );
  });

  // A failure openapi-fetch cannot parse as JSON must still *reject*. If it
  // resolves with `undefined`, callers get a value that lies about its type and
  // render a blank page with no error state — silent, and very hard to diagnose.
  it("throws rather than resolving undefined when the body is not JSON", async () => {
    mockHtmlFetch(503);
    const error = await fetchItem("std::io::NoSuchThing").catch((e) => e);
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).status).toBe(503);
  });
});
