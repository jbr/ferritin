/**
 * The site colophon: a quiet footer on every page crediting the stack and
 * offering a way back to the source. Rendered once inside `DocsLayout`, below
 * the reading columns, so it sits at the true bottom of the scroll rather than
 * inside any one rail.
 *
 * The "report an issue" link is built at click time from `window.location` so it
 * always carries the page the reader is actually looking at — prefilled into
 * GitHub's new-issue template rather than dropping them on an empty form.
 */

const REPO = "https://github.com/jbr/ferritin";

function reportIssueHref(): string {
  const here = window.location.href;
  const params = new URLSearchParams({
    title: "Issue with a documentation page",
    body: `**Page:** ${here}\n\n**What went wrong:**\n\n`,
    labels: "docs",
  });
  return `${REPO}/issues/new?${params}`;
}

/** An external link that opens in a new tab, the footer's only kind. */
function Ext({ href, children }: { href: string; children: React.ReactNode }) {
  return (
    <a href={href} target="_blank" rel="noopener noreferrer">
      {children}
    </a>
  );
}

export function Colophon() {
  return (
    <footer className="colophon">
      <div className="colophon-inner">
        {/* Each credit is its own span so a segment wraps as a unit; the dot
            separators between them are drawn in CSS. */}
        <div className="colophon-credits">
          <span>
            Powered by <Ext href="https://trillium.rs">Trillium</Ext>
          </span>
          <span>
            by <Ext href="https://jbr.me">jbr</Ext>
          </span>
          <span>
            Documentation from <Ext href="https://docs.rs">docs.rs</Ext> and{" "}
            <Ext href="https://crates.io">crates.io</Ext>
          </span>
          <span>
            <Ext href={`${REPO}/blob/main/LICENSE-MIT`}>MIT</Ext>/
            <Ext href={`${REPO}/blob/main/LICENSE-APACHE`}>Apache-2.0</Ext> on{" "}
            <Ext href={REPO}>GitHub</Ext>
          </span>
          <span>
            Set in{" "}
            <Ext href="https://fonts.google.com/specimen/Hanken+Grotesk">
              Hanken Grotesk
            </Ext>{" "}
            and{" "}
            <Ext href="https://www.jetbrains.com/lp/mono/">JetBrains Mono</Ext>
          </span>
        </div>
        <a
          className="colophon-report"
          href={reportIssueHref()}
          target="_blank"
          rel="noopener noreferrer"
          onClick={(e) => {
            // Rebuild from the live location: the footer may have rendered on a
            // different item than the one the reader is on now.
            e.currentTarget.href = reportIssueHref();
          }}
        >
          Report an issue with this page
        </a>
      </div>
    </footer>
  );
}
