import { useState } from "react";
import { detectPlatform, type PlatformId } from "../lib/platform";
import { Snippet } from "./Snippet";

const RELEASES = "https://github.com/jbr/ferritin/releases";
// The installer scripts live on a rolling tag that always points at the newest
// release; the archives themselves only exist on the versioned ones, which is
// why the "download directly" escape hatch links the releases page rather than
// deep-linking an asset.
const LATEST = `${RELEASES}/download/ferritin-latest`;

const SHELL = `curl --proto '=https' --tlsv1.2 -LsSf ${LATEST}/ferritin-installer.sh | sh`;
const POWERSHELL = `powershell -c "irm ${LATEST}/ferritin-installer.ps1 | iex"`;

const PLATFORMS: { id: PlatformId; label: string; command: string }[] = [
  { id: "mac", label: "macOS", command: SHELL },
  { id: "linux", label: "Linux", command: SHELL },
  { id: "windows", label: "Windows", command: POWERSHELL },
];

/** Shown to visitors we can't place — the most common platform for this audience. */
const DEFAULT_PLATFORM: PlatformId = "mac";

/**
 * Platform-detecting install picker. Detection is a heuristic (`navigator.platform`
 * is deprecated and Apple Silicon under-reports itself), so every option stays one
 * click away and the detected one is only ever a pre-selection.
 */
export function Install() {
  // Detection runs once, in a lazy initializer rather than an effect: this is a
  // pure client render, so there's no server pass to stay consistent with.
  const [detected] = useState(detectPlatform);
  const [override, setOverride] = useState<PlatformId | null>(null);

  const activeId = override ?? detected ?? DEFAULT_PLATFORM;
  const active =
    PLATFORMS.find((p) => p.id === activeId) ??
    PLATFORMS.find((p) => p.id === DEFAULT_PLATFORM)!;
  const autoDetected = override === null && detected !== null;

  return (
    <div className="picker install">
      <div className="picker-tabs" role="tablist" aria-label="Platform">
        {PLATFORMS.map((p) => (
          <button
            key={p.id}
            type="button"
            role="tab"
            aria-selected={p.id === activeId}
            className="picker-tab"
            onClick={() => setOverride(p.id)}
          >
            {p.label}
          </button>
        ))}
      </div>

      <Snippet code={active.command} />

      <p className="picker-note">
        {autoDetected ? (
          <>
            Detected <strong>{active.label}</strong> — not right? Pick another
            above.{" "}
          </>
        ) : null}
        On any platform you can also{" "}
        <a href="https://github.com/cargo-bins/cargo-binstall">
          <code>cargo binstall ferritin</code>
        </a>
        , build from source with <code>cargo install ferritin</code>, or grab an
        archive from the <a href={RELEASES}>releases page</a>.
      </p>
    </div>
  );
}
