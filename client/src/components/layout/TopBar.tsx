import { Link } from "rhoto-router";
import { itemHref } from "../../lib/paths";
import { ThemeToggle } from "../../theme/ThemeToggle";
import { CommandPalette } from "../search/CommandPalette";

/** The top bar: brand, breadcrumb crate, ⌘K search, and the theme toggle. */
export function TopBar({ crate }: { crate?: string }) {
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
      <CommandPalette crate={crate} />
      <div className="topbar-right">
        <ThemeToggle />
      </div>
    </header>
  );
}
