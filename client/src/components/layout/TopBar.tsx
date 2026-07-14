import { Link } from "rhoto-router";
import { itemHref } from "../../lib/paths";
import { ThemeToggle } from "../../theme/ThemeToggle";
import { SearchBox } from "../search/SearchBox";

/**
 * The top bar: brand, breadcrumb crate, ⌘K search, and the theme toggle.
 *
 * `search` is off on the landing page, which puts the same field inline under its
 * hero — two search boxes on one screen would be two answers to "where do I type?".
 */
export function TopBar({
  crate,
  search = true,
}: {
  crate?: string;
  search?: boolean;
}) {
  return (
    <header className="topbar">
      <div className="topbar-brand">
        <Link href="/" className="logo">
          <span className="logo-mark" aria-hidden />
          <span className="logo-text">ferritin</span>
        </Link>
        {crate ? (
          <>
            <span className="brand-sep">/</span>
            <Link href={itemHref(crate)} className="brand-crate mono">
              {crate}
            </Link>
          </>
        ) : null}
      </div>
      {search ? <SearchBox crate={crate} /> : null}
      <div className="topbar-right">
        <ThemeToggle />
      </div>
    </header>
  );
}
