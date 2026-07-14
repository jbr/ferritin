/** Both Meta+K and Ctrl+K open the search everywhere; this only decides which one
 * we *name* in the UI. */
const isApple =
  typeof navigator !== "undefined" &&
  /Mac|iPhone|iPad|iPod/.test(navigator.platform || navigator.userAgent);

export const searchShortcut = isApple ? "⌘K" : "Ctrl K";
export const searchShortcutAria = isApple ? "Meta+K" : "Control+K";

/**
 * The OSes we ship an installer for. Deliberately coarser than the four target
 * triples in `dist-workspace.toml`: the shell installer picks the architecture
 * itself, so Apple Silicon and Intel would print the identical command — and
 * telling them apart in a browser needs a deprecated WebGL renderer probe that
 * the user-agent actively lies about. We don't need the answer, so we don't ask.
 */
export type PlatformId = "mac" | "linux" | "windows";

/**
 * Best-guess platform, or null when we have no prebuilt binary for it (mobile,
 * an unknown OS) — in which case the installer widget pre-selects nothing and
 * shows every option. Detection is heuristic, so the UI always lets you override.
 */
export function detectPlatform(): PlatformId | null {
  if (typeof navigator === "undefined") return null;

  const ua = navigator.userAgent || "";
  const platform = navigator.platform || "";

  if (/Android|iPhone|iPad|iPod/.test(ua)) return null;
  if (platform.startsWith("Win") || ua.includes("Windows")) return "windows";
  if (platform.startsWith("Mac") || ua.includes("Mac OS")) return "mac";
  if (platform.includes("Linux") || ua.includes("Linux")) return "linux";
  return null;
}
