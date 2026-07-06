/**
 * Convenience aliases for the generated OpenAPI schema types, so components can
 * import `Span`/`Node`/`Method` directly instead of indexing `components` each
 * time. The generated `schema.gen.ts` remains the single source of truth.
 */
import type { components } from "./schema.gen";

type S = components["schemas"];

export type Item = S["JsonItem"];
export type Meta = S["JsonMeta"];
export type Body = S["JsonBody"];
export type Span = S["JsonSpan"];
export type CodeSpan = S["JsonCodeSpan"];
export type Node = S["JsonNode"];
export type Method = S["JsonMethod"];
export type Field = S["JsonField"];
export type Variant = S["JsonVariant"];
export type TraitImpl = S["JsonTraitImpl"];
export type Implementor = S["JsonImplementor"];
export type TraitMember = S["JsonTraitMember"];
export type ImplAssocType = S["JsonImplAssocType"];
export type ModuleItem = S["JsonModuleItem"];
export type SearchResult = S["JsonSearchResult"];
export type Suggestion = S["JsonSuggestion"];

/** The `kind` discriminants a `JsonBody` can carry. */
export type BodyKind = Body["kind"];
