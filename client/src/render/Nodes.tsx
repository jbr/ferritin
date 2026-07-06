import type { CodeSpan, Node } from "../api/types";
import { Spans } from "./Spans";

/**
 * Render the presentation-node stream (`JsonNode[]`) the server ships for doc
 * prose, item metadata, and any not-yet-modeled body. This is a faithful mirror
 * of the terminal's `DocumentNode` tree, one level flatter than HTML.
 */
export function Nodes({ nodes }: { nodes: Node[] | undefined }) {
  if (!nodes?.length) return null;
  return (
    <>
      {nodes.map((node, i) => (
        <NodeBlock key={i} node={node} />
      ))}
    </>
  );
}

function NodeBlock({ node }: { node: Node }) {
  switch (node.type) {
    case "paragraph":
      return (
        <p>
          <Spans spans={node.spans} />
        </p>
      );

    case "heading":
      return node.level === "Title" ? (
        <h2>
          <Spans spans={node.spans} />
        </h2>
      ) : (
        <h3>
          <Spans spans={node.spans} />
        </h3>
      );

    case "section":
      return (
        <section className="doc-section">
          {node.title ? (
            <h3>
              <Spans spans={node.title} />
            </h3>
          ) : null}
          <Nodes nodes={node.nodes} />
        </section>
      );

    case "list":
      return (
        <ul>
          {node.items.map((item, i) => (
            <li key={i}>
              <Nodes nodes={item.content} />
            </li>
          ))}
        </ul>
      );

    case "codeBlock":
      return <CodeBlock spans={node.spans} attrs={node.attrs} />;

    case "generatedCode":
      return (
        <pre className="code-block">
          <code>
            <Spans spans={node.spans} />
          </code>
        </pre>
      );

    case "blockQuote":
      return (
        <blockquote>
          <Nodes nodes={node.nodes} />
        </blockquote>
      );

    case "horizontalRule":
      return <hr />;

    case "table":
      return (
        <table className="doc-table">
          {node.header ? (
            <thead>
              <tr>
                {node.header.map((cell, i) => (
                  <th key={i}>
                    <Spans spans={cell.spans} />
                  </th>
                ))}
              </tr>
            </thead>
          ) : null}
          <tbody>
            {node.rows.map((row, r) => (
              <tr key={r}>
                {row.map((cell, c) => (
                  <td key={c}>
                    <Spans spans={cell.spans} />
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      );

    case "metadata":
      return (
        <dl className="doc-metadata">
          {node.fields.map((field, i) => (
            <div key={i} className="doc-metadata-row">
              <dt>{field.label}</dt>
              <dd>
                <Spans spans={field.value} />
              </dd>
            </div>
          ))}
        </dl>
      );

    case "truncatedBlock":
      // On the web there is no truncation — render the full block.
      return <Nodes nodes={node.nodes} />;

    case "conditional":
      // Interactive-only content belongs to the TUI; everything else renders.
      return node.show_when === "Interactive" ? null : (
        <Nodes nodes={node.nodes} />
      );

    default:
      return null;
  }
}

/**
 * A fenced code block: syntect-highlighted spans (server-side, class-tagged) plus
 * a reader note for doctest attributes that assert the example's behavior.
 * `should_panic`/`compile_fail` mean the snippet is a deliberate counterexample,
 * so they get a prominent banner rather than a quiet chip.
 */
const CODE_NOTES: Record<string, { label: string; kind: "warn" | "fail" }> = {
  should_panic: {
    label: "Panics — this example is expected to panic at runtime.",
    kind: "warn",
  },
  compile_fail: {
    label: "Does not compile — this example is expected to fail to compile.",
    kind: "fail",
  },
};

function CodeBlock({
  spans,
  attrs,
}: {
  spans: CodeSpan[];
  attrs?: string[];
}) {
  const notes = (attrs ?? [])
    .map((attr) => CODE_NOTES[attr])
    .filter((note): note is (typeof CODE_NOTES)[string] => Boolean(note));
  return (
    <div className={notes.length ? "code-block has-note" : "code-block"}>
      {notes.length ? (
        <div className="code-note">
          {notes.map((note) => (
            <span key={note.label} className={`code-note-item ${note.kind}`}>
              <span aria-hidden>⚠</span> {note.label}
            </span>
          ))}
        </div>
      ) : null}
      <pre>
        <code>
          {spans.map((span, i) =>
            span.class ? (
              <span key={i} className={`syn syn-${span.class}`}>
                {span.text}
              </span>
            ) : (
              <span key={i}>{span.text}</span>
            ),
          )}
        </code>
      </pre>
    </div>
  );
}
