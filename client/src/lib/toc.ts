/**
 * "On this page" table of contents, derived from the item model. The same id
 * helpers are used by the body components so anchors and TOC links stay in sync
 * — there is no separate registration step.
 */
import type { Item, Method } from "../api/types";
import { slug } from "./paths";

export type TocEntry = { id: string; label: string; depth: 0 | 1 };

export const SectionId = {
  top: "item-top",
  fields: "fields",
  variants: "variants",
  members: "members",
  implementations: "implementations",
  traitImpls: "trait-implementations",
  implementors: "implementors",
} as const;

/** Anchor id for an inherent/impl method card. */
export function methodId(name: string): string {
  return `method-${slug(name)}`;
}

/** Anchor id for a module-listing group heading (grouped by kind). */
export function groupId(kind: string): string {
  return `group-${slug(kind)}`;
}

/** Human label for the top entry, e.g. "Struct Conn". */
function topLabel(item: Item): string {
  const kind = item.meta.kind;
  const titled = kind.charAt(0).toUpperCase() + kind.slice(1);
  return `${titled} ${item.meta.name}`;
}

export function buildToc(item: Item): TocEntry[] {
  const entries: TocEntry[] = [
    { id: SectionId.top, label: topLabel(item), depth: 0 },
  ];
  const body = item.body;

  const push = (id: string, label: string, depth: 0 | 1 = 0) =>
    entries.push({ id, label, depth });

  switch (body.kind) {
    case "struct":
    case "union":
      if (body.fields?.length) push(SectionId.fields, "Fields");
      pushMethods(body.methods, push);
      if (body.traitImpls?.length)
        push(SectionId.traitImpls, "Trait Implementations");
      break;

    case "enum":
      if (body.variants?.length) push(SectionId.variants, "Variants");
      pushMethods(body.methods, push);
      if (body.traitImpls?.length)
        push(SectionId.traitImpls, "Trait Implementations");
      break;

    case "trait":
      if (body.members?.length) push(SectionId.members, "Members");
      if (body.implementors?.length)
        push(SectionId.implementors, "Implementors");
      break;

    case "module":
      for (const kind of moduleKinds(body.items)) {
        push(groupId(kind), moduleGroupLabel(kind));
      }
      break;

    default:
      break;
  }

  return entries;
}

function pushMethods(
  methods: Method[] | undefined,
  push: (id: string, label: string, depth?: 0 | 1) => void,
): void {
  if (!methods?.length) return;
  push(SectionId.implementations, "Implementations");
  for (const method of methods) {
    push(methodId(method.name), method.name, 1);
  }
}

/** Distinct kinds present in a module listing, in first-seen order. */
export function moduleKinds(items: { kind: string }[] | undefined): string[] {
  const seen: string[] = [];
  for (const item of items ?? []) {
    if (!seen.includes(item.kind)) seen.push(item.kind);
  }
  return seen;
}

/** Pluralized, title-cased group label for a module kind ("struct" → "Structs"). */
export function moduleGroupLabel(kind: string): string {
  const titled = kind.charAt(0).toUpperCase() + kind.slice(1);
  if (kind === "macro") return "Macros";
  return `${titled}s`;
}
