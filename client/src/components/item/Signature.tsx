import { useState, type ReactNode } from "react";
import type { Node, Span } from "../../api/types";
import { useItem } from "../../api/queries";
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
 * The server ships an associated item's docs already truncated to their leading
 * block (`TruncationLevel::SingleLine`), flagging the cut with `truncated` — see
 * `json.rs`. That flag is the only signal that prose was dropped, so a card that
 * ignores it silently hides documentation.
 */
function isTruncated(docs: Node[] | undefined): boolean {
  return !!docs?.some(
    (node) => node.type === "truncatedBlock" && node.truncated,
  );
}

/**
 * A disclosure card keyed on a signature line: the summary shows the signature
 * (mono, inline) and the body reveals doc prose. Used for methods, trait
 * members, and implementors. Defaults open when it has docs to show.
 *
 * When the server truncated the prose *and* the caller supplied `expandPath` (the
 * item's own path — associated items are addressable, e.g. `trillium::Conn::ok`),
 * the card offers to fetch the full docs and swap them in place. The round trip is
 * the point: a small, cacheable, per-item request keeps the parent payload a
 * summary instead of shipping every method's essay up front.
 */
export function SigCard({
  spans,
  docs,
  id,
  expandPath,
  defaultOpen = true,
}: {
  spans: Span[] | undefined;
  docs?: Node[];
  id?: string;
  expandPath?: string;
  defaultOpen?: boolean;
}) {
  const [expanded, setExpanded] = useState(false);
  const canExpand = isTruncated(docs) && !!expandPath;
  const { data, isLoading } = useItem(expandPath ?? "", expanded && canExpand);

  const hasDocs = !!docs?.length;
  const shown = expanded && data?.docs?.length ? data.docs : docs;

  return (
    <details className="sig-card" id={id} open={defaultOpen && hasDocs}>
      <summary className="sig-card-summary">
        <code className="sig">
          <Spans spans={spans} />
        </code>
      </summary>
      {hasDocs ? (
        <div className="sig-card-docs">
          <Nodes nodes={shown} />
          {canExpand ? (
            <button
              type="button"
              className="expand-docs"
              aria-expanded={expanded}
              onClick={() => setExpanded((open) => !open)}
            >
              {isLoading
                ? "Loading…"
                : expanded
                  ? "Show less"
                  : "Show full documentation"}
            </button>
          ) : null}
        </div>
      ) : null}
    </details>
  );
}
