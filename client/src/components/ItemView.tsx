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
  const notFound = error instanceof ApiError ? error.notFound : undefined;
  const suggestions = notFound?.suggestions;
  // A crate that exists on crates.io but whose docs we can't serve is a distinct
  // outcome from a typo: name it, and offer no misleading "did you mean".
  const unavailableCrate =
    notFound?.error === "crateUnavailable"
      ? notFound.unavailableCrate
      : undefined;

  if (unavailableCrate) {
    return (
      <div className="status error">
        <h1 className="not-found-title">
          Documentation unavailable for {unavailableCrate}
        </h1>
        <p>
          <code>{unavailableCrate}</code> exists on crates.io, but its
          documentation isn’t available here — docs.rs has no rustdoc JSON for
          it (often a build failure or a crate with no library target).
        </p>
        <p>
          <a
            href={`https://docs.rs/crate/${encodeURIComponent(unavailableCrate)}`}
            target="_blank"
            rel="noopener noreferrer"
          >
            View {unavailableCrate} on docs.rs
          </a>{" "}
          for its build status and versions.
        </p>
      </div>
    );
  }

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
