import { Link } from 'rhoto-router';
import type { JsonSpan } from '../types/api';

export function SpanRenderer({ spans }: { spans: JsonSpan[] }) {
  return (
    <>
      {spans.map((span, i) => (
        <SpanElement key={i} span={span} />
      ))}
    </>
  );
}

function SpanElement({ span }: { span: JsonSpan }) {
  const className = `span-${span.style.toLowerCase()}`;
  const content = renderTextWithBreaks(span.text);

  if (span.url) {
    if (span.url.startsWith('http://') || span.url.startsWith('https://')) {
      return (
        <a href={span.url} className={className} target="_blank" rel="noopener">
          {content}
        </a>
      );
    }

    return (
      <Link href={span.url} className={className}>
        {content}
      </Link>
    );
  }

  return <span className={className}>{content}</span>;
}

function renderTextWithBreaks(text: string) {
  const lines = text.split('\n');
  return lines.flatMap((line, i) =>
    i < lines.length - 1 ? [line, <br key={i} />] : [line]
  );
}
