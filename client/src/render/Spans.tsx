import { Link } from "rhoto-router";
import type { Span } from "../api/types";
import { itemHref } from "../lib/paths";

/**
 * Render a span sequence — the shared "leaf" vocabulary the server emits for
 * every signature, type reference, field type, and bound. Each span carries a
 * semantic `style` (mapped to a syntax color) and optionally a navigation target:
 * `path` for in-app routing (preferred), or `url` for an external docs.rs page.
 */
export function Spans({ spans }: { spans: Span[] | undefined }) {
  if (!spans?.length) return null;
  return (
    <>
      {spans.map((span, i) => (
        <SpanLeaf key={i} span={span} />
      ))}
    </>
  );
}

function SpanLeaf({ span }: { span: Span }) {
  const className = `tok tok-${span.style}`;

  if (span.path) {
    return (
      <Link href={itemHref(span.path)} className={className}>
        {span.text}
      </Link>
    );
  }

  if (span.url) {
    return (
      <a href={span.url} className={className} target="_blank" rel="noreferrer">
        {span.text}
      </a>
    );
  }

  return <span className={className}>{span.text}</span>;
}
