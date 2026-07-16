import { DocsLayout } from "./layout/DocsLayout";
import { Install } from "./Install";
import { Lightbox } from "./Lightbox";
import { Snippet } from "./Snippet";
import { useDocumentTitle } from "../lib/useDocumentTitle";
import tuiScreenshot from "../assets/tui-pattern.webp";

/** The screenshot's intrinsic pixels — see `Lightbox` for why they're passed. */
const TUI_SHOT = { width: 2624, height: 1830 };

/**
 * The local-ferritin page: how to install the binary, and what the same engine
 * that renders this site does outside the browser.
 *
 * Split out of the landing page, which now pitches the hosted site and links
 * here rather than carrying the whole CLI story inline. Lives at `/~install` —
 * see `RESERVED_SIGIL` for why a page route looks like that.
 */
export function InstallPage() {
  useDocumentTitle("Run it locally");

  return (
    <DocsLayout toc={[]}>
      <div className="home">
        <header className="home-hero">
          <h1 className="home-title">Run ferritin locally</h1>
          <p className="home-lede">
            Ferritin is a single binary. It fetches json from docs.rs on demand
            and caches what it fetches, so the second lookup of a crate is
            instant — and it builds and reads your own workspace's docs with the
            nightly toolchain the same way it reads anyone else's.
          </p>
          <Install />
        </header>

        <section className="home-section">
          <h2>Three ways to read</h2>
          <dl className="home-points">
            <div>
              <dt>Interactive browser</dt>
              <dd>
                <Snippet code="ferritin -i" />A full TUI: keyboard and mouse
                navigation, history and breadcrumbs, syntax highlighting, hover
                previews, clickable links.
                <Lightbox
                  src={tuiScreenshot}
                  alt={
                    "The ferritin TUI showing core::str::pattern::Pattern: a table of " +
                    "pattern types against their match conditions, a syntax-highlighted " +
                    "example, and a breadcrumb trail recording the search that got here."
                  }
                  {...TUI_SHOT}
                />
              </dd>
            </div>
            <div>
              <dt>One-shot CLI</dt>
              <dd>
                <Snippet code={"ferritin get tokio::net::TcpStream"} />
                <Snippet code={"ferritin search trillium 'response headers'"} />
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
