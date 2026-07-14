import { describe, expect, it } from "vitest";
import type { Item, Node } from "../api/types";
import { itemPreview } from "./preview";

function para(text: string): Node {
  return { type: "paragraph", spans: [{ text, style: "Plain" }] } as Node;
}

/** An item with the given body and docs; only the fields the preview reads matter. */
function item(body: unknown, docs?: Node[]): Item {
  return {
    meta: {
      name: "Thing",
      kind: "struct",
      visibility: "public",
      crateName: "c",
    },
    body,
    docs,
  } as Item;
}

describe("itemPreview blurb", () => {
  it("is the opening paragraph, and only that", () => {
    const preview = itemPreview(
      item({ kind: "struct" }, [para("The first."), para("The second.")]),
    );
    expect(preview.blurb).toEqual([para("The first.")]);
  });

  it("looks inside a truncatedBlock — the terminal's cut marker is not a paragraph", () => {
    const docs = [
      { type: "truncatedBlock", truncated: true, nodes: [para("Inside.")] },
    ] as Node[];
    expect(itemPreview(item({ kind: "struct" }, docs)).blurb).toEqual([
      para("Inside."),
    ]);
  });

  it("skips a leading heading to reach the prose", () => {
    const docs = [
      {
        type: "heading",
        level: "Title",
        spans: [{ text: "H", style: "Plain" }],
      },
      para("The prose."),
    ] as Node[];
    expect(itemPreview(item({ kind: "struct" }, docs)).blurb).toEqual([
      para("The prose."),
    ]);
  });

  it("is empty for an undocumented item", () => {
    expect(itemPreview(item({ kind: "struct" })).blurb).toEqual([]);
  });
});

describe("itemPreview facts", () => {
  it("counts a struct's fields, methods, and trait impls", () => {
    const preview = itemPreview(
      item({
        kind: "struct",
        fields: [{}, {}],
        methods: [{}, {}, {}],
        traitImpls: [{}],
      }),
    );
    expect(preview.facts).toEqual(["2 fields", "3 methods", "1 trait impl"]);
  });

  it("includes a struct's hidden (private) fields in the count", () => {
    // The server reports these separately; a reader wants the total.
    const preview = itemPreview(
      item({ kind: "struct", fields: [{}], hiddenFieldCount: 4 }),
    );
    expect(preview.facts).toEqual(["5 fields"]);
  });

  it("omits zero counts rather than saying '0 fields'", () => {
    expect(itemPreview(item({ kind: "struct", methods: [{}] })).facts).toEqual([
      "1 method",
    ]);
  });

  it("splits a trait's members into required and provided", () => {
    const preview = itemPreview(
      item({
        kind: "trait",
        members: [
          { hasDefault: false },
          { hasDefault: true },
          { hasDefault: true },
        ],
        implementors: [{}, {}],
      }),
    );
    expect(preview.facts).toEqual([
      "1 required method",
      "2 provided methods",
      "2 implementors",
    ]);
  });

  it("counts an enum's variants", () => {
    const preview = itemPreview(
      item({ kind: "enum", variants: [{}, {}], methods: [{}] }),
    );
    expect(preview.facts).toEqual(["2 variants", "1 method"]);
  });

  it("counts a module's items", () => {
    expect(
      itemPreview(item({ kind: "module", items: [{}, {}] })).facts,
    ).toEqual(["2 items"]);
  });

  it("reports a function's modifiers, which are what a glance wants", () => {
    const preview = itemPreview(
      item({ kind: "function", isAsync: true, isUnsafe: true }),
    );
    expect(preview.facts).toEqual(["async", "unsafe"]);
  });

  it("has nothing to say about a constant", () => {
    expect(itemPreview(item({ kind: "constant" })).facts).toEqual([]);
  });
});
