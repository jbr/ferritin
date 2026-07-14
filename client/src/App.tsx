import { Route } from "rhoto-router";
import { Home } from "./components/Home";
import { ItemView } from "./components/ItemView";

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
    </>
  );
}
