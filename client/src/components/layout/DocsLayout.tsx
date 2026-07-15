import type { ReactNode } from "react";
import { crateOf } from "../../lib/paths";
import type { TocEntry } from "../../lib/toc";
import { TopBar } from "./TopBar";
import { CrateNav } from "./CrateNav";
import { PageToc } from "./PageToc";
import { Colophon } from "./Colophon";

/**
 * The three-column reading shell: top bar spanning the width, a left crate-nav,
 * the scrolling main column, and a right "on this page" TOC. `path` drives the
 * top-bar breadcrumb and the left nav (which opens itself along that path); `toc`
 * the right rail.
 *
 * The crate root is not a special case. It used to render an empty left rail,
 * since a flat list of the root's children only repeated the main column — but a
 * tree is not a repeat of that listing: it descends into the submodules the
 * listing can only name.
 */
export function DocsLayout({
  path,
  toc,
  children,
  search = true,
}: {
  path?: string;
  toc: TocEntry[];
  children: ReactNode;
  /** Off on the landing page, which hosts the search field inline instead. */
  search?: boolean;
}) {
  const crate = path ? crateOf(path) : undefined;
  return (
    <div className="docs">
      <TopBar crate={crate} search={search} />
      <div className="docs-cols">
        <CrateNav path={path} />
        <main className="docs-main">{children}</main>
        <PageToc entries={toc} />
      </div>
      <Colophon />
    </div>
  );
}
