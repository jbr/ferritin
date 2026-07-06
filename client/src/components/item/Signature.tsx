import type { ReactNode } from "react";
import type { Node, Span } from "../../api/types";
import { Spans } from "../../render/Spans";
import { Nodes } from "../../render/Nodes";

/** The small uppercase kind chip beside the item title (STRUCT, TRAIT, …). */
export function KindBadge({ kind }: { kind: string }) {
  return <span className="kind-badge">{kind}</span>;
}

/** A section title with an anchor id (matches the "on this page" TOC links). */
export function SectionHeading({
  id,
  children,
}: {
  id: string;
  children: ReactNode;
}) {
  return (
    <h2 id={id} className="section-heading">
      {children}
    </h2>
  );
}

/** The sunken declaration block under the item title. */
export function SignatureBlock({ spans }: { spans: Span[] | null }) {
  if (!spans?.length) return null;
  return (
    <pre className="signature">
      <code>
        <Spans spans={spans} />
      </code>
    </pre>
  );
}

/**
 * A disclosure card keyed on a signature line: the summary shows the signature
 * (mono, inline) and the body reveals doc prose. Used for methods, trait
 * members, and implementors. Defaults open when it has docs to show.
 */
export function SigCard({
  spans,
  docs,
  id,
  defaultOpen = true,
}: {
  spans: Span[] | undefined;
  docs?: Node[];
  id?: string;
  defaultOpen?: boolean;
}) {
  const hasDocs = !!docs?.length;
  return (
    <details className="sig-card" id={id} open={defaultOpen && hasDocs}>
      <summary className="sig-card-summary">
        <code className="sig">
          <Spans spans={spans} />
        </code>
      </summary>
      {hasDocs ? (
        <div className="sig-card-docs">
          <Nodes nodes={docs} />
        </div>
      ) : null}
    </details>
  );
}
