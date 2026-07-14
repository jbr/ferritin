import { describe, expect, it } from "vitest";
import { lastSegment, parentPath, plural, spineOf } from "./nav";

describe("parentPath", () => {
  it("drops the last segment", () => {
    expect(parentPath("tokio::sync::Mutex")).toBe("tokio::sync");
  });

  it("has nothing above a bare crate", () => {
    expect(parentPath("tokio")).toBeUndefined();
  });
});

describe("lastSegment", () => {
  it("is the name a tree row displays", () => {
    expect(lastSegment("tokio::sync::Mutex")).toBe("Mutex");
    expect(lastSegment("tokio")).toBe("tokio");
  });
});

describe("spineOf", () => {
  it("is every prefix, crate root first", () => {
    expect(spineOf("std::os::unix::fs")).toEqual([
      "std",
      "std::os",
      "std::os::unix",
      "std::os::unix::fs",
    ]);
  });

  it("is just the crate at a crate root", () => {
    expect(spineOf("serde")).toEqual(["serde"]);
  });
});

describe("plural", () => {
  it("keeps a lone module singular", () => {
    expect(plural(1, "module")).toBe("1 module");
    expect(plural(77, "module")).toBe("77 modules");
  });
});
