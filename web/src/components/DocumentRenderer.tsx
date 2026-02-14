import type { JsonDocument } from '../types/api';
import { NodeRenderer } from './NodeRenderer';

export function DocumentRenderer({ document }: { document: JsonDocument }) {
  return (
    <div className="ferritin-document">
      {document.nodes.map((node, i) => (
        <NodeRenderer key={i} node={node} />
      ))}
    </div>
  );
}
