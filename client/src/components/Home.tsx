import { CyclingType } from "./CyclingType";
import { DocsLayout } from "./layout/DocsLayout";
import { Install } from "./Install";
import { SearchBox } from "./search/SearchBox";
import { Snippet } from "./Snippet";
import { useDocumentTitle } from "../lib/useDocumentTitle";

/**
 * The landing page, shown when no item path is routed. Doubles as the pitch:
 * what ferritin is, what the model makes possible, and how to run it locally.
 */
export function Home() {
  // Bare site name — otherwise navigating back to the landing page keeps whatever
  // item title was set last.
  useDocumentTitle(undefined);
  return (
    <DocsLayout toc={[]} search={false}>
      <div className="home">
        <header className="home-hero">
          <h1 className="home-title">Ferritin Rust Documentation</h1>
          <p className="home-lede">
            Ferritin reads rustdoc's JSON output and resolves it into a
            structured model of the API that can be formatted in various
            contexts.
          </p>
          <p className="home-try">
            Open an item to start, or view any crate published to docs.rs:
          </p>
          <p className="home-cycle-row">
            <span className="home-cycle-lead">try</span>
            <CyclingType />
          </p>
          <SearchBox inline />
        </header>

        <section className="home-section">
          <h2>How it works</h2>
          <ol className="home-steps">
            <li>
              <strong>Fetch the JSON.</strong> Crates published to docs.rs ship
              a rustdoc JSON build alongside their HTML. Ferritin fetches that
              and caches it on disk.
            </li>
            <li>
              <strong>Resolve it into a model.</strong> Items, fields, impls and
              intra-doc links become a semantic model of the API, with
              re-exports followed across crate boundaries and external crates
              loaded on demand as paths reach into them.
            </li>
            <li>
              <strong>Render the model.</strong> That model supports several
              interfaces: an interactive TUI, a one-shot CLI, token-efficient
              markdown for agents, and this web app.
            </li>
          </ol>
        </section>

        <section className="home-section">
          <h2>What that makes possible</h2>
          <dl className="home-points">
            <div>
              <dt>Re-exports resolve where you look</dt>
              <dd>
                <code>std::vec::Vec</code> is defined in <code>alloc</code>.
                Ferritin follows the chain, loads the defining crate for you,
                and documents it under the path you actually typed.
              </dd>
            </div>
            <div>
              <dt>One toolkit for std, crates.io, and your own code</dt>
              <dd>
                The same path grammar and the same rendering, whether the crate
                came from rustup, from docs.rs, or from <code>cargo doc</code>{" "}
                over the workspace you're editing right now.
              </dd>
            </div>
            <div>
              <dt>Search that crosses crates</dt>
              <dd>
                BM25 ranking with document statistics pooled across every crate
                in scope, so a hit in one crate ranks meaningfully against a hit
                in another.
              </dd>
            </div>
          </dl>
        </section>

        <section className="home-section">
          <h2>Run it locally</h2>
          <p>
            Ferritin is a single binary. It fetches json from docs.rs on demand
            and caches what it fetches, so the second lookup of a crate is
            instant — and it builds and reads your own workspace's docs with the
            nightly toolchain the same way it reads anyone else's.
          </p>
          <Install />

          <dl className="home-points">
            <div>
              <dt>Interactive browser</dt>
              <dd>
                <Snippet code="ferritin -i" />A full TUI: keyboard and mouse
                navigation, history and breadcrumbs, syntax highlighting, hover
                previews, clickable links.
              </dd>
            </div>
            <div>
              <dt>One-shot CLI</dt>
              <dd>
                <Snippet
                  code={
                    "ferritin get tokio::net::TcpStream\nferritin search trillium 'response headers'"
                  }
                />
                A page of documentation straight to stdout — highlighted, with
                terminal hyperlinks — for when you know what you're looking for.
              </dd>
            </div>
            <div>
              <dt>Agent mode</dt>
              <dd>
                <Snippet code="ferritin --format agent get serde::Deserialize" />
                Token-efficient markdown, selected automatically when ferritin
                detects it's running under a coding agent. It ships an Agent
                Skill too, so your agent looks the API up instead of guessing at
                it.
              </dd>
            </div>
          </dl>
        </section>
      </div>
    </DocsLayout>
  );
}
