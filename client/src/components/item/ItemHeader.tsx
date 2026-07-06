import { Link } from "rhoto-router";
import type { Item } from "../../api/types";
import { Nodes } from "../../render/Nodes";
import { itemHref } from "../../lib/paths";
import { SectionId } from "../../lib/toc";
import { declarationSpans } from "../../lib/declaration";
import { KindBadge, SignatureBlock } from "./Signature";

/** Item title block: breadcrumb, kind badge, name, declaration, and own docs. */
export function ItemHeader({ item, path }: { item: Item; path: string }) {
  const { meta } = item;
  return (
    <header className="item-header" id={SectionId.top}>
      <Breadcrumb path={path} />
      <div className="item-title-row">
        <KindBadge kind={meta.kind} />
        <h1 className="item-title">{meta.name}</h1>
        {item.canonicalUrl ? (
          <a
            className="source-link"
            href={item.canonicalUrl}
            target="_blank"
            rel="noreferrer"
          >
            source ↗
          </a>
        ) : null}
      </div>
      <SignatureBlock spans={declarationSpans(item)} />
      {item.docs?.length ? (
        <div className="item-docs">
          <Nodes nodes={item.docs} />
        </div>
      ) : null}
    </header>
  );
}

/** `trillium :: conn :: Conn`, each prefix segment a link to that path. */
function Breadcrumb({ path }: { path: string }) {
  const segments = path.split("::");
  return (
    <nav className="breadcrumb" aria-label="Breadcrumb">
      {segments.map((segment, i) => {
        const isLast = i === segments.length - 1;
        const upto = segments.slice(0, i + 1).join("::");
        return (
          <span key={i}>
            {i > 0 ? <span className="breadcrumb-sep">::</span> : null}
            {isLast ? (
              <span className="breadcrumb-current">{segment}</span>
            ) : (
              <Link href={itemHref(upto)} className="breadcrumb-link">
                {segment}
              </Link>
            )}
          </span>
        );
      })}
    </nav>
  );
}
