import type { ReactNode } from "react";
import type { TocEntry } from "../../lib/toc";
import { TopBar } from "./TopBar";
import { CrateNav } from "./CrateNav";
import { PageToc } from "./PageToc";

/**
 * The three-column reading shell: top bar spanning the width, a left crate-nav,
 * the scrolling main column, and a right "on this page" TOC. `crate` drives both
 * the top-bar breadcrumb and the left nav; `toc` the right rail.
 */
export function DocsLayout({
  crate,
  toc,
  children,
  isCrateRoot,
}: {
  crate?: string;
  toc: TocEntry[];
  children: ReactNode;
  isCrateRoot?: boolean;
}) {
  return (
    <div className="docs">
      <TopBar crate={crate} />
      <div className="docs-cols">
        {isCrateRoot ? (
          // Keep the left column's width so the main content stays put; we may
          // fill it with crate-root-specific navigation later.
          <div className="crate-nav" aria-hidden />
        ) : (
          <CrateNav crate={crate} />
        )}
        <main className="docs-main">{children}</main>
        <PageToc entries={toc} />
      </div>
    </div>
  );
}
