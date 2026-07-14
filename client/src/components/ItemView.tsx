import { Link } from "rhoto-router";
import { ApiError } from "../api/client";
import { useItem } from "../api/queries";
import { itemHref } from "../lib/paths";
import { buildToc } from "../lib/toc";
import { useDocumentTitle } from "../lib/useDocumentTitle";
import { DocsLayout } from "./layout/DocsLayout";
import { ItemHeader } from "./item/ItemHeader";
import { ItemBody } from "./item/ItemBody";

/**
 * The docs reading view for one item path. Renders inside the three-column
 * shell; the path drives the top bar and the left nav's tree — both of which are
 * useful while the item itself is still loading.
 */
export function ItemView({ path }: { path: string }) {
  const { data, error, isLoading } = useItem(path);
  const toc = data ? buildToc(data) : [];

  // Keyed on the route path, not the loaded item, so the title (and the history
  // entry it names) is right the moment you navigate — before the fetch resolves.
  useDocumentTitle(error ? `Not found: ${path}` : path);

  return (
    <DocsLayout path={path} toc={toc}>
      {isLoading ? (
        <p className="status">Loading {path}…</p>
      ) : error ? (
        <NotFoundView path={path} error={error} />
      ) : data ? (
        <article className="item">
          <ItemHeader item={data} path={path} />
          <ItemBody item={data} path={path} />
        </article>
      ) : null}
    </DocsLayout>
  );
}

function NotFoundView({ path, error }: { path: string; error: Error }) {
  const suggestions =
    error instanceof ApiError ? error.notFound?.suggestions : undefined;
  return (
    <div className="status error">
      <h1 className="not-found-title">No item at {path}</h1>
      {suggestions?.length ? (
        <>
          <p>Did you mean:</p>
          <ul className="suggestions">
            {suggestions.map((s) => (
              <li key={s.path}>
                <Link href={itemHref(s.path)}>{s.path}</Link>
                {s.kind ? <span className="muted"> — {s.kind}</span> : null}
              </li>
            ))}
          </ul>
        </>
      ) : (
        <p>{error.message}</p>
      )}
    </div>
  );
}
