import { Route } from "rhoto-router";
import { Home } from "./components/Home";
import { InstallPage } from "./components/InstallPage";
import { ItemView } from "./components/ItemView";
import { HoverPreview } from "./components/preview/HoverPreview";
import { isReservedPath } from "./lib/paths";

export function App() {
  return (
    <>
      <Route path="/" exact>
        <Home />
      </Route>

      {/* Pages that aren't crate docs wear a `~` — see `RESERVED_SIGIL` for why
          that can never collide with a crate, and why it stays one segment. */}
      <Route path="/~install" exact>
        <InstallPage />
      </Route>

      {/* Everything else is an item path. The reserved sigil is carved out here
          rather than by route order: routes match independently, so this one
          would otherwise also claim `/~install` and render it as a crate. */}
      <Route path="/*path">
        {(params: { path?: string }) =>
          params.path && !isReservedPath(`/${params.path}`) ? (
            <ItemView path={params.path} />
          ) : null
        }
      </Route>

      {/* Outside the routes: the card is anchored to a link, not to a page, and
          survives the navigation its own links cause. */}
      <HoverPreview />
    </>
  );
}
