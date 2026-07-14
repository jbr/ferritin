/**
 * Reduce a full `Item` to the "taste" a hover card shows: what kind of thing it
 * is, its opening sentence, and a few counts that tell a reader whether the page
 * is worth a click.
 *
 * The card deliberately derives this client-side from the item endpoint rather
 * than asking for a summary: the fetch it triggers lands in the same
 * `["item", path]` query cache `ItemView` reads, so hovering a link warms the
 * navigation that usually follows it.
 */
import type { Item, Node } from "../api/types";

export interface Preview {
  /** Lowercased item kind — `struct`, `trait`, `fn`, … */
  kind: string;
  name: string;
  /** The opening paragraph of the docs, or empty when the item is undocumented. */
  blurb: Node[];
  /** Short count phrases: `12 fields`, `48 methods`, `30 trait impls`. */
  facts: string[];
}

export function itemPreview(item: Item): Preview {
  return {
    kind: item.meta.kind,
    name: item.meta.name,
    blurb: leadParagraph(item.docs),
    facts: facts(item),
  };
}

/**
 * The first paragraph of a doc stream, looking through the wrappers that can
 * enclose it. `truncatedBlock` is the terminal's cut marker and `conditional`
 * gates TUI-only content; neither is a paragraph itself, but either can hold the
 * one we want.
 */
function leadParagraph(docs: Node[] | undefined): Node[] {
  for (const node of docs ?? []) {
    if (node.type === "paragraph") return [node];
    if (node.type === "truncatedBlock" || node.type === "conditional") {
      const inner = leadParagraph(node.nodes);
      if (inner.length) return inner;
    }
  }
  return [];
}

/**
 * Count phrases per body kind. Only non-zero counts appear, so a fieldless unit
 * struct doesn't advertise "0 fields" — an empty fact row is the honest answer.
 */
function facts(item: Item): string[] {
  const body = item.body;
  switch (body.kind) {
    case "struct":
    case "union": {
      const fields = (body.fields?.length ?? 0) + (body.hiddenFieldCount ?? 0);
      return [
        count(fields, "field"),
        count(body.methods?.length, "method"),
        count(body.traitImpls?.length, "trait impl"),
      ].filter(nonEmpty);
    }

    case "enum":
      return [
        count(body.variants?.length, "variant"),
        count(body.methods?.length, "method"),
        count(body.traitImpls?.length, "trait impl"),
      ].filter(nonEmpty);

    case "trait": {
      const members = body.members ?? [];
      const required = members.filter((m) => !m.hasDefault).length;
      const provided = members.length - required;
      return [
        count(required, "required method"),
        count(provided, "provided method"),
        count(body.implementors?.length, "implementor"),
      ].filter(nonEmpty);
    }

    case "module":
      return [count(body.items?.length, "item")].filter(nonEmpty);

    case "function": {
      // Modifiers, not counts — for a function they are the thing worth knowing
      // at a glance, and they read the same way in the fact row.
      const modifiers = [
        body.isAsync && "async",
        body.isConst && "const",
        body.isUnsafe && "unsafe",
      ].filter((m): m is string => typeof m === "string");
      return modifiers;
    }

    default:
      return [];
  }
}

/** `1 field` / `12 fields`; empty when the count is zero or absent. */
function count(n: number | undefined, noun: string): string {
  if (!n) return "";
  return `${n} ${noun}${n === 1 ? "" : "s"}`;
}

function nonEmpty(s: string): boolean {
  return s.length > 0;
}
