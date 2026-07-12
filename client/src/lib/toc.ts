/**
 * "On this page" table of contents, derived from the item model. The same id
 * helpers are used by the body components so anchors and TOC links stay in sync
 * — there is no separate registration step.
 */
import type { Item, Method, Node, Span } from "../api/types";
import { slug } from "./paths";

export type TocEntry = { id: string; label: string; depth: 0 | 1 };

/** Plain text of a span sequence (its concatenated leaf text). */
export function spansText(spans: Span[] | undefined): string {
  return (spans ?? []).map((s) => s.text).join("");
}

/**
 * Slugify heading text into a DOM id, matching rustdoc's own scheme so anchor
 * links authors wrote in doc comments (`[…](#read-and-write)`) resolve here too.
 *
 * rustdoc's rule (librustdoc `slugify`): ASCII alphanumerics are lowercased,
 * `-`/`_` kept, non-ASCII alphanumerics kept as-is, whitespace maps to a single
 * `-`, everything else is dropped — runs are *not* collapsed. Validated against
 * ~/.cargo/rustdoc-json: "Why Send + Sync + 'static" → "why-send--sync--static",
 * and "…the `_with` method suffix" keeps the `_` and drops the backticks.
 */
export function slugifyHeading(text: string): string {
  let out = "";
  for (const ch of text) {
    if (/[0-9A-Za-z]/.test(ch)) out += ch.toLowerCase();
    else if (ch === "-" || ch === "_") out += ch;
    else if (/\s/.test(ch)) out += "-";
    else if (/[\p{L}\p{N}]/u.test(ch)) out += ch; // non-ASCII alphanumerics kept
    // else: dropped
  }
  return out;
}

/** DOM id for a rendered doc-prose heading, from its span sequence. */
export function headingId(spans: Span[] | undefined): string {
  return slugifyHeading(spansText(spans));
}

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

/** Anchor id for a module-listing group heading. */
export function groupId(key: string): string {
  return `group-${key}`;
}

/** Human label for the top entry, e.g. "Struct Conn". */
function topLabel(item: Item): string {
  const kind = item.meta.kind;
  const titled = kind.charAt(0).toUpperCase() + kind.slice(1);
  return `${titled} ${item.meta.name}`;
}

/**
 * Collect the headings from an item's own doc prose, recursing through the
 * container nodes that can hold them. Interactive-only content is skipped —
 * `Nodes` doesn't render it on the web, so it must not appear in the minimap.
 */
function collectDocHeadings(nodes: Node[] | undefined, out: TocEntry[]): void {
  for (const node of nodes ?? []) {
    switch (node.type) {
      case "heading":
        out.push({
          id: headingId(node.spans),
          label: spansText(node.spans),
          depth: node.level === "Title" ? 0 : 1,
        });
        break;
      case "section":
        collectDocHeadings(node.nodes, out);
        break;
      case "blockQuote":
        collectDocHeadings(node.nodes, out);
        break;
      case "truncatedBlock":
        collectDocHeadings(node.nodes, out);
        break;
      case "conditional":
        if (node.show_when !== "Interactive")
          collectDocHeadings(node.nodes, out);
        break;
      default:
        break;
    }
  }
}

export function buildToc(item: Item): TocEntry[] {
  const entries: TocEntry[] = [
    { id: SectionId.top, label: topLabel(item), depth: 0 },
  ];
  // Prose headings appear right after the top entry — matching the reading
  // order, where the item's own docs render above the structural sections.
  collectDocHeadings(item.docs, entries);
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
      for (const group of moduleGroups(body.items)) {
        push(groupId(group.key), group.label);
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

/**
 * Module-child grouping, mirroring the terminal's `GROUP_ORDER`
 * (ferritin/src/format/module.rs): a fixed display order, and several
 * macro-flavored kinds (`macro`, `procattribute`, `procderive`) coalesced under
 * one "Macros" group so a raw rustdoc kind never surfaces as its own heading
 * (no stray "Procattributes"). The JSON ships the raw `kind` precisely so
 * clients can group however they like; this is our grouping.
 */
const GROUP_ORDER: readonly {
  key: string;
  label: string;
  kinds: readonly string[];
}[] = [
  { key: "modules", label: "Modules", kinds: ["module"] },
  { key: "structs", label: "Structs", kinds: ["struct"] },
  { key: "enums", label: "Enums", kinds: ["enum"] },
  { key: "traits", label: "Traits", kinds: ["trait"] },
  { key: "unions", label: "Unions", kinds: ["union"] },
  { key: "type-aliases", label: "Type Aliases", kinds: ["typealias"] },
  { key: "functions", label: "Functions", kinds: ["function"] },
  { key: "constants", label: "Constants", kinds: ["constant"] },
  { key: "statics", label: "Statics", kinds: ["static"] },
  {
    key: "macros",
    label: "Macros",
    kinds: ["macro", "procattribute", "procderive"],
  },
  { key: "primitives", label: "Primitives", kinds: ["primitive"] },
  { key: "variants", label: "Variants", kinds: ["variant"] },
];

export type ModuleGroup<T> = { key: string; label: string; items: T[] };

/**
 * Group a module's children into the terminal's fixed group order. Any kind the
 * table doesn't know about still gets its own trailing group rather than
 * silently vanishing from the nav.
 */
export function moduleGroups<T extends { kind: string }>(
  items: T[] | undefined,
): ModuleGroup<T>[] {
  if (!items?.length) return [];
  const groups: ModuleGroup<T>[] = [];
  const claimed = new Set<string>();
  for (const { key, label, kinds } of GROUP_ORDER) {
    const members = items.filter((it) => kinds.includes(it.kind));
    if (members.length) {
      groups.push({ key, label, items: members });
      for (const k of kinds) claimed.add(k);
    }
  }
  for (const item of items) {
    if (claimed.has(item.kind)) continue;
    claimed.add(item.kind);
    const label = item.kind.charAt(0).toUpperCase() + item.kind.slice(1) + "s";
    groups.push({
      key: item.kind,
      label,
      items: items.filter((it) => it.kind === item.kind),
    });
  }
  return groups;
}
