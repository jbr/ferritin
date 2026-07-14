import { Route } from "rhoto-router";
import { Home } from "./components/Home";
import { ItemView } from "./components/ItemView";
import { HoverPreview } from "./components/preview/HoverPreview";

export function App() {
  return (
    <>
      <Route path="/" exact>
        <Home />
      </Route>
      <Route path="/*path">
        {(params: { path?: string }) =>
          params.path ? <ItemView path={params.path} /> : null
        }
      </Route>
      {/* Outside the routes: the card is anchored to a link, not to a page, and
          survives the navigation its own links cause. */}
      <HoverPreview />
    </>
  );
}
