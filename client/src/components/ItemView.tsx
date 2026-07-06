import { Link } from "rhoto-router";
import { ApiError } from "../api/client";
import { useItem } from "../api/queries";
import { crateOf, itemHref } from "../lib/paths";
import { buildToc } from "../lib/toc";
import { DocsLayout } from "./layout/DocsLayout";
import { ItemHeader } from "./item/ItemHeader";
import { ItemBody } from "./item/ItemBody";

/**
 * The docs reading view for one item path. Renders inside the three-column
 * shell; the crate (derived from the path) drives the top bar and left nav even
 * while the item itself is still loading.
 */
export function ItemView({ path }: { path: string }) {
  const crate = crateOf(path);
  const isCrateRoot = path === crate;
  const { data, error, isLoading } = useItem(path);
  const toc = data ? buildToc(data) : [];

  return (
    <DocsLayout crate={crate} toc={toc} isCrateRoot={isCrateRoot}>
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
