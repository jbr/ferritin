import { useState } from "react";
import { Link } from "rhoto-router";
import { DocsLayout } from "./layout/DocsLayout";
import { Snippet } from "./Snippet";
import { useDocumentTitle } from "../lib/useDocumentTitle";

/**
 * The public endpoint, hardcoded rather than derived from `location.origin`.
 *
 * `/mcp` only exists when the server was built with the `mcp` feature, which is
 * off by default and which the client has no way to see — the bundle is static
 * and nothing hands it build flags. So an origin-derived URL would, on every
 * local `ferritin serve` without that feature, produce a config the reader
 * copies into their agent and which then 404s. The hosted endpoint is always
 * there, and is what someone reading this page is being offered anyway.
 *
 * Worth revisiting if trillium-frontend grows a way to wire server-side values
 * into the client.
 */
const ENDPOINT = "https://ferritin.rs/mcp";

type Client = {
  id: string;
  label: string;
  code: string;
  /** Where the snippet goes, for the clients that want a file rather than a command. */
  note?: React.ReactNode;
};

const CLIENTS: Client[] = [
  {
    id: "claude-code",
    label: "Claude Code",
    code: `claude mcp add --transport http ferritin ${ENDPOINT}`,
  },
  {
    id: "codex",
    label: "Codex CLI",
    code: `codex mcp add ferritin --url ${ENDPOINT}`,
  },
  {
    id: "opencode",
    label: "OpenCode",
    code: `{
  "mcp": {
    "ferritin": {
      "type": "remote",
      "url": "${ENDPOINT}",
      "enabled": true
    }
  }
}`,
    note: (
      <>
        Merge into <code>~/.config/opencode/opencode.json</code>.
      </>
    ),
  },
  {
    id: "cursor",
    label: "Cursor",
    code: `{
  "mcpServers": {
    "ferritin": { "url": "${ENDPOINT}" }
  }
}`,
    note: (
      <>
        Merge into <code>~/.cursor/mcp.json</code> — the CLI (
        <code>cursor-agent</code>) reads the same config as the IDE. Verify with{" "}
        <code>cursor-agent mcp list</code>.
      </>
    ),
  },
];

/**
 * The hosted MCP server's connection page. Lives at `/~mcp` — see
 * `RESERVED_SIGIL` for why a page route looks like that, and note that it is
 * deliberately *not* `/mcp`, which is the endpoint itself.
 *
 * The picker mirrors `Install`: one tab per client, a copyable snippet, and no
 * hidden state beyond which tab is showing. Unlike `Install` there's nothing to
 * detect — a browser can't tell us which agent the reader runs — so the first
 * tab is simply the most common one.
 */
export function McpPage() {
  useDocumentTitle("Connect your agent");

  const [activeId, setActiveId] = useState(CLIENTS[0]!.id);
  const active = CLIENTS.find((c) => c.id === activeId) ?? CLIENTS[0]!;

  return (
    <DocsLayout toc={[]}>
      <div className="home">
        <header className="home-hero">
          <h1 className="home-title">Connect your agent</h1>
          <p className="home-lede">
            Ferritin serves this site's documentation over MCP at{" "}
            <code>{ENDPOINT}</code>. Point a coding agent at it and it looks the
            API up instead of guessing at it — no install, no API key, no local
            toolchain.
          </p>

          <div className="picker">
            <div className="picker-tabs" role="tablist" aria-label="Agent">
              {CLIENTS.map((c) => (
                <button
                  key={c.id}
                  type="button"
                  role="tab"
                  aria-selected={c.id === activeId}
                  className="picker-tab"
                  onClick={() => setActiveId(c.id)}
                >
                  {c.label}
                </button>
              ))}
            </div>

            <Snippet code={active.code} />

            {active.note ? <p className="picker-note">{active.note}</p> : null}
          </div>
        </header>

        <section className="home-section">
          <h2>What your agent gets</h2>
          <dl className="home-points">
            <div>
              <dt>
                <code>search</code>
              </dt>
              <dd>
                Find items in a crate by what they do, not just what they're
                called — documentation, intra-doc links and signatures all rank.
              </dd>
            </div>
            <div>
              <dt>
                <code>get</code>
              </dt>
              <dd>
                A whole item — signature, docs, methods, trait implementations —
                as token-efficient markdown, resolved to where it's actually
                defined even when that's another crate.
              </dd>
            </div>
          </dl>
        </section>

        <section className="home-section">
          <h2>Or run it locally</h2>
          <p>
            The same lookups run offline against a cache you control — and
            against your own workspace's docs, which no hosted server can see.{" "}
            <Link href="/~install">Install ferritin</Link>: it ships an Agent
            Skill that teaches your agent the CLI, so it reads local crates the
            same way it reads this one.
          </p>
        </section>
      </div>
    </DocsLayout>
  );
}
