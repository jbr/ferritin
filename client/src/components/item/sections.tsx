import { useState } from "react";
import type {
  Field,
  Implementor,
  Method,
  TraitImpl,
  TraitMember,
  Variant,
} from "../../api/types";
import { Spans } from "../../render/Spans";
import { Nodes } from "../../render/Nodes";
import { SectionHeading, SigCard } from "./Signature";
import { SectionId, methodId } from "../../lib/toc";

/**
 * Inherent associated items ("Implementations"). `parentPath` is the path of the
 * type they hang off, which makes each one addressable (`{parent}::{name}`) — that
 * is what lets a card fetch its own full docs when the summary was truncated.
 */
export function MethodList({
  methods,
  parentPath,
}: {
  methods: Method[] | undefined;
  parentPath?: string;
}) {
  if (!methods?.length) return null;
  return (
    <section className="item-section">
      <SectionHeading id={SectionId.implementations}>
        Implementations
      </SectionHeading>
      <div className="card-list">
        {methods.map((method, i) => (
          <SigCard
            key={i}
            id={methodId(method.name)}
            spans={method.signature}
            docs={method.docs}
            expandPath={
              parentPath ? `${parentPath}::${method.name}` : undefined
            }
          />
        ))}
      </div>
    </section>
  );
}

/** Struct / union fields. */
export function FieldList({
  fields,
  hidden,
}: {
  fields: Field[] | undefined;
  hidden: number | undefined;
}) {
  if (!fields?.length && !hidden) return null;
  return (
    <section className="item-section">
      <SectionHeading id={SectionId.fields}>Fields</SectionHeading>
      <ul className="field-list">
        {fields?.map((field, i) => (
          <li key={i} className="field">
            <code className="sig">
              {!field.pub ? (
                <span className="tok tok-Comment">private </span>
              ) : null}
              <span className="tok tok-FieldName">
                {field.name ?? String(field.index)}
              </span>
              <span className="tok tok-Punctuation">: </span>
              <Spans spans={field.type} />
            </code>
            {field.docs?.length ? (
              <div className="field-docs">
                <Nodes nodes={field.docs} />
              </div>
            ) : null}
          </li>
        ))}
      </ul>
      {hidden ? (
        <p className="muted">
          {`/* ${hidden} private field${hidden > 1 ? "s" : ""} */`}
        </p>
      ) : null}
    </section>
  );
}

/** Enum variants, each rendered with its shape (unit / tuple / struct). */
export function VariantList({ variants }: { variants: Variant[] | undefined }) {
  if (!variants?.length) return null;
  return (
    <section className="item-section">
      <SectionHeading id={SectionId.variants}>Variants</SectionHeading>
      <ul className="variant-list">
        {variants.map((variant, i) => (
          <li key={i} className="variant">
            <code className="sig">
              <span className="tok tok-TypeName">{variant.name}</span>
              <VariantShape variant={variant} />
            </code>
            {variant.docs?.length ? (
              <div className="field-docs">
                <Nodes nodes={variant.docs} />
              </div>
            ) : null}
          </li>
        ))}
      </ul>
    </section>
  );
}

function VariantShape({ variant }: { variant: Variant }) {
  if (variant.shape === "tuple" && variant.tupleFields?.length) {
    return (
      <>
        <span className="tok tok-Punctuation">(</span>
        {variant.tupleFields.map((spans, i) => (
          <span key={i}>
            {i > 0 ? <span className="tok tok-Punctuation">, </span> : null}
            <Spans spans={spans} />
          </span>
        ))}
        <span className="tok tok-Punctuation">)</span>
      </>
    );
  }
  if (variant.shape === "struct" && variant.fields?.length) {
    return (
      <>
        <span className="tok tok-Punctuation"> {"{ "}</span>
        {variant.fields.map((field, i) => (
          <span key={i}>
            {i > 0 ? <span className="tok tok-Punctuation">, </span> : null}
            <span className="tok tok-FieldName">{field.name}</span>
            <span className="tok tok-Punctuation">: </span>
            <Spans spans={field.type} />
          </span>
        ))}
        <span className="tok tok-Punctuation">{" }"}</span>
      </>
    );
  }
  return null;
}

/** Trait members (required + provided), rendered as signature cards. */
export function MembersList({
  members,
  parentPath,
}: {
  members: TraitMember[] | undefined;
  parentPath?: string;
}) {
  if (!members?.length) return null;
  return (
    <section className="item-section">
      <SectionHeading id={SectionId.members}>Members</SectionHeading>
      <div className="card-list">
        {members.map((member, i) => (
          <SigCard
            key={i}
            spans={member.signature}
            docs={member.docs}
            expandPath={
              parentPath ? `${parentPath}::${member.name}` : undefined
            }
          />
        ))}
      </div>
    </section>
  );
}

/** Trait implementations, as collapsed chips that expand to methods / assoc types. */
export function TraitImplList({ impls }: { impls: TraitImpl[] | undefined }) {
  if (!impls?.length) return null;
  return (
    <section className="item-section">
      <SectionHeading id={SectionId.traitImpls}>
        Trait Implementations
      </SectionHeading>
      <div className="chip-list">
        {impls.map((impl, i) => (
          <TraitImplChip key={i} impl={impl} />
        ))}
      </div>
    </section>
  );
}

function TraitImplChip({ impl }: { impl: TraitImpl }) {
  const detail =
    !!impl.assocTypes?.length || !!impl.methods?.length || !!impl.docs?.length;
  return (
    <details className="impl-chip">
      <summary className="impl-chip-summary">
        <code className="sig">
          {impl.isNegative ? <span className="tok tok-Operator">!</span> : null}
          {impl.isUnsafe ? (
            <span className="tok tok-Keyword">unsafe </span>
          ) : null}
          <span className="tok tok-TypeName">{impl.traitName}</span>
          <Spans spans={impl.args} />
        </code>
      </summary>
      {detail ? (
        <div className="impl-detail">
          {impl.docs?.length ? <Nodes nodes={impl.docs} /> : null}
          {impl.assocTypes?.map((assoc, i) => (
            <code key={i} className="sig assoc-type">
              <span className="tok tok-Keyword">type </span>
              <span className="tok tok-TypeName">{assoc.name}</span>
              <span className="tok tok-Punctuation"> = </span>
              <Spans spans={assoc.type} />
            </code>
          ))}
          {impl.methods?.map((method, i) => (
            <SigCard
              key={i}
              spans={method.signature}
              docs={method.docs}
              defaultOpen={false}
            />
          ))}
        </div>
      ) : null}
    </details>
  );
}

/**
 * Types implementing a trait ("Implementors"). The server now sends the whole
 * list (the 20-item cap is a terminal concern), so the overflow is revealed in
 * place rather than being lost — no request needed.
 */
const IMPLEMENTOR_PREVIEW = 20;

export function ImplementorList({
  implementors,
}: {
  implementors: Implementor[] | undefined;
}) {
  const [showAll, setShowAll] = useState(false);
  if (!implementors?.length) return null;

  const hidden = implementors.length - IMPLEMENTOR_PREVIEW;
  const shown =
    showAll || hidden <= 0
      ? implementors
      : implementors.slice(0, IMPLEMENTOR_PREVIEW);

  return (
    <section className="item-section">
      <SectionHeading id={SectionId.implementors}>Implementors</SectionHeading>
      <div className="chip-list">
        {shown.map((impl, i) => (
          <details key={i} className="impl-chip">
            <summary className="impl-chip-summary">
              <code className="sig">
                <span className="tok tok-Keyword">impl </span>
                <Spans spans={impl.forType} />
              </code>
            </summary>
            {impl.methods?.length || impl.assocTypes?.length ? (
              <div className="impl-detail">
                {impl.assocTypes?.map((assoc, j) => (
                  <code key={j} className="sig assoc-type">
                    <span className="tok tok-Keyword">type </span>
                    <span className="tok tok-TypeName">{assoc.name}</span>
                    <span className="tok tok-Punctuation"> = </span>
                    <Spans spans={assoc.type} />
                  </code>
                ))}
                {impl.methods?.map((method, j) => (
                  <SigCard
                    key={j}
                    spans={method.signature}
                    docs={method.docs}
                    defaultOpen={false}
                  />
                ))}
              </div>
            ) : null}
          </details>
        ))}
      </div>
      {hidden > 0 && !showAll ? (
        <button
          type="button"
          className="expand-docs"
          onClick={() => setShowAll(true)}
        >
          {`Show ${hidden} more implementor${hidden > 1 ? "s" : ""}`}
        </button>
      ) : null}
    </section>
  );
}
