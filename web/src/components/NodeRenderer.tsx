import type { JsonNode } from '../types/api';
import { SpanRenderer } from './SpanRenderer';

export function NodeRenderer({ node }: { node: JsonNode }) {
  switch (node.type) {
    case 'paragraph':
      return (
        <p>
          <SpanRenderer spans={node.spans} />
        </p>
      );

    case 'heading':
      const HeadingTag = node.level === 'Title' ? 'h1' : 'h2';
      return (
        <HeadingTag>
          <SpanRenderer spans={node.spans} />
        </HeadingTag>
      );

    case 'section':
      return (
        <section>
          {node.title && (
            <h3>
              <SpanRenderer spans={node.title} />
            </h3>
          )}
          {node.nodes.map((n, i) => (
            <NodeRenderer key={i} node={n} />
          ))}
        </section>
      );

    case 'list':
      return (
        <ul>
          {node.items.map((item, i) => (
            <li key={i}>
              {item.content.map((n, j) => (
                <NodeRenderer key={j} node={n} />
              ))}
            </li>
          ))}
        </ul>
      );

    case 'codeBlock':
      return (
        <pre>
          <code className={node.lang ? `language-${node.lang}` : undefined}>
            {node.code}
          </code>
        </pre>
      );

    case 'generatedCode':
      return (
        <pre className="generated-code">
          <code>
            <SpanRenderer spans={node.spans} />
          </code>
        </pre>
      );

    case 'horizontalRule':
      return <hr />;

    case 'blockQuote':
      return (
        <blockquote>
          {node.nodes.map((n, i) => (
            <NodeRenderer key={i} node={n} />
          ))}
        </blockquote>
      );

    case 'table':
      return (
        <table>
          {node.header && (
            <thead>
              <tr>
                {node.header.map((cell, i) => (
                  <th key={i}>
                    <SpanRenderer spans={cell.spans} />
                  </th>
                ))}
              </tr>
            </thead>
          )}
          <tbody>
            {node.rows.map((row, i) => (
              <tr key={i}>
                {row.map((cell, j) => (
                  <td key={j}>
                    <SpanRenderer spans={cell.spans} />
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      );
  }
}
