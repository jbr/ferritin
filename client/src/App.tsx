import { Link, Route } from "rhoto-router";
import { ItemView } from "./components/ItemView";
import { DocsLayout } from "./components/layout/DocsLayout";

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

function Home() {
  return (
    <DocsLayout toc={[]}>
      <div className="home">
        <div className="home-badge">rustdoc, rebuilt from JSON</div>
        <h1 className="home-title">Ferritin Rust Documentation</h1>
        <p className="home-lede">
          Ferritin turns rustdoc JSON into a structured reading experience. Open
          an item to start — for example{" "}
          <Link href="/trillium::Conn">trillium::Conn</Link>,{" "}
          <Link href="/std::vec::Vec">std::vec::Vec</Link>, or{" "}
          <Link href="/serde::Deserialize">serde::Deserialize</Link>.
        </p>
      </div>
    </DocsLayout>
  );
}
