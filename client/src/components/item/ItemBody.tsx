import { Link } from "rhoto-router";
import type { Item, ModuleItem } from "../../api/types";
import { Nodes } from "../../render/Nodes";
import { itemHref } from "../../lib/paths";
import { groupId, moduleGroups } from "../../lib/toc";
import { SectionHeading } from "./Signature";
import {
  FieldList,
  ImplementorList,
  MembersList,
  MethodList,
  TraitImplList,
  VariantList,
} from "./sections";

/**
 * Dispatch on the item's body kind to the structural sections below the header.
 * Single-signature kinds (function, type-alias, const, static, assoc item) are
 * fully described by the header declaration, so they render nothing extra.
 */
export function ItemBody({ item, path }: { item: Item; path: string }) {
  const body = item.body;

  switch (body.kind) {
    case "struct":
    case "union":
      return (
        <>
          <FieldList fields={body.fields} hidden={body.hiddenFieldCount} />
          <MethodList methods={body.methods} parentPath={path} />
          <TraitImplList impls={body.traitImpls} />
        </>
      );

    case "enum":
      return (
        <>
          <VariantList variants={body.variants} />
          <MethodList methods={body.methods} parentPath={path} />
          <TraitImplList impls={body.traitImpls} />
        </>
      );

    case "trait":
      return (
        <>
          <MembersList members={body.members} parentPath={path} />
          <ImplementorList implementors={body.implementors} />
        </>
      );

    case "module":
      return <ModuleBody items={body.items} base={path} />;

    case "macro":
      return (
        <pre className="signature">
          <code>{body.definition}</code>
        </pre>
      );

    case "presentation":
      return <Nodes nodes={body.nodes} />;

    default:
      return null;
  }
}

/** A module's children, grouped by kind (Structs, Enums, Traits, …). */
function ModuleBody({
  items,
  base,
}: {
  items: ModuleItem[] | undefined;
  base: string;
}) {
  if (!items?.length) return null;
  return (
    <>
      {moduleGroups(items).map((group) => (
        <section className="item-section" key={group.key}>
          <SectionHeading id={groupId(group.key)}>{group.label}</SectionHeading>
          <ul className="module-list">
            {group.items.map((it, i) => (
              <li key={i} className="module-row">
                <Link
                  href={itemHref(`${base}::${it.path}`)}
                  className="module-item"
                >
                  {it.path}
                </Link>
                {it.docs?.length ? (
                  <div className="module-item-docs">
                    <Nodes nodes={it.docs} />
                  </div>
                ) : null}
              </li>
            ))}
          </ul>
        </section>
      ))}
    </>
  );
}
