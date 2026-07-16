import { Link } from "rhoto-router";
import { CyclingType } from "./CyclingType";
import { DocsLayout } from "./layout/DocsLayout";
import { SearchBox } from "./search/SearchBox";
import { useDocumentTitle } from "../lib/useDocumentTitle";

const REPO = "https://github.com/jbr/ferritin";

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
          <h1 className="home-title">Read documentation for Rust crates</h1>
          <p className="home-lede">
            Every crate on docs.rs, searchable as you type, with every type a
            click from where it's defined.
          </p>
          <SearchBox inline />
          <p className="home-cycle-row">
            <span className="home-cycle-lead">or try</span>
            <CyclingType />
          </p>
        </header>

        <section className="home-section">
          <dl className="home-points">
            <div>
              <dt>Search that goes straight to the item.</dt>
              <dd>
                Results rank documentation, intra-doc links and signatures, not
                just type names.
              </dd>
            </div>
            <div>
              <dt>Preview any type before you click.</dt>
              <dd>
                Hover any type for a preview of what it is. Follow it and
                Ferritin takes you to where it's actually defined, across crate
                boundaries
              </dd>
            </div>
            <div>
              <dt>Navigation that keeps its place.</dt>
              <dd>
                A crate tree on the left and a live outline on the right track
                where you are as you read and let you jump anywhere in the
                crate.
              </dd>
            </div>
          </dl>
        </section>

        <section className="home-section">
          <h2>How it works</h2>
          <p>
            Ferritin reads the rustdoc JSON that docs.rs builds for published
            crates, resolves it into a model of the API — following re-exports
            across crates, loading external crates as paths reach into them —
            and renders that model here. std::vec::Vec is defined in alloc;
            Ferritin follows the chain and documents it under the path you
            looked up.
          </p>
        </section>
        <section className="home-section">
          <h2>Run it locally</h2>
          <p>
            Ferritin is open source. Prefer the terminal? The same engine runs
            locally as a CLI, an interactive TUI, and an MCP server for coding
            agents.
          </p>
          <p className="home-cta-row">
            <Link className="home-cta" href="/~install">
              Install ferritin
            </Link>
            <a className="home-cta secondary" href={REPO}>
              Source on GitHub
            </a>
          </p>
        </section>
      </div>
    </DocsLayout>
  );
}
