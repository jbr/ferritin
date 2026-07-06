/**
 * Synthesize the header "declaration" span line (`pub struct Conn<…>`) shown in
 * the sunken signature box. The domain model gives structure (fields, variants,
 * members) rather than a rendered one-liner for container kinds, so we assemble a
 * concise declaration from the parts we do have. Single-signature kinds
 * (function, type-alias, const, static) already carry a full span sequence and
 * are passed through. Returns `null` for kinds with no useful one-liner (module,
 * macro — the macro renders its definition as a code block instead).
 */
import type { Item, Span } from "../api/types";

const span = (text: string, style: Span["style"]): Span => ({ text, style });
const kw = (t: string) => span(t, "Keyword");
const ty = (t: string) => span(t, "TypeName");
const plain = (t: string) => span(t, "Plain");
const punct = (t: string) => span(t, "Punctuation");

function whereSuffix(clause: Span[] | undefined): Span[] {
  return clause?.length ? [plain(" "), ...clause] : [];
}

export function declarationSpans(item: Item): Span[] | null {
  const body = item.body;
  const isPub = item.meta.visibility === "public";
  const vis = isPub ? [kw("pub"), plain(" ")] : [];

  switch (body.kind) {
    case "struct":
    case "union":
    case "enum": {
      const keyword =
        body.kind === "struct"
          ? "struct"
          : body.kind === "union"
            ? "union"
            : "enum";
      return [
        ...vis,
        kw(keyword),
        plain(" "),
        ty(body.name),
        ...(body.generics ?? []),
        ...whereSuffix(body.whereClause),
      ];
    }

    case "trait":
      return [
        ...vis,
        kw("trait"),
        plain(" "),
        ty(body.name),
        ...(body.generics ?? []),
        ...(body.supertraits?.length ? [punct(": "), ...body.supertraits] : []),
        ...whereSuffix(body.whereClause),
      ];

    case "function":
      // Already a full `pub fn name(...) -> ...` sequence.
      return body.signature;

    case "typeAlias":
      return [
        ...vis,
        kw("type"),
        plain(" "),
        ty(body.name),
        plain(" = "),
        ...body.aliased,
      ];

    case "constant":
      return [
        ...vis,
        kw("const"),
        plain(" "),
        span(body.name, "Plain"),
        punct(": "),
        ...body.type,
        ...(body.value ? [plain(" = "), plain(body.value)] : []),
      ];

    case "static":
      return [
        ...vis,
        kw("static"),
        plain(" "),
        span(body.name, "Plain"),
        punct(": "),
        ...body.type,
        plain(" = "),
        plain(body.value),
      ];

    case "assocItem":
      return body.signature;

    default:
      return null;
  }
}
