import { useCallback, useRef, useState } from "react";

/**
 * A code block with a copy-to-clipboard affordance. Used for the shell snippets
 * on the landing page; deliberately separate from the `.code-block`s the doc
 * renderer emits, which are prose rather than things you run.
 */
export function Snippet({ code }: { code: string }) {
  const [copied, setCopied] = useState(false);
  // Held so a second click mid-flash restarts the timer rather than letting the
  // first one clear the label out from under it.
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  const copy = useCallback(() => {
    void navigator.clipboard.writeText(code).then(() => {
      setCopied(true);
      clearTimeout(timer.current);
      timer.current = setTimeout(() => setCopied(false), 1600);
    });
  }, [code]);

  return (
    <div className="snippet">
      <div className="code-block">
        <pre>
          <code>{code}</code>
        </pre>
      </div>
      <button
        type="button"
        className="snippet-copy"
        onClick={copy}
        aria-label={`Copy "${code}" to clipboard`}
      >
        {copied ? "Copied" : "Copy"}
      </button>
    </div>
  );
}
