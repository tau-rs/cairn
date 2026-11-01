import { useMemo, useState } from "react";
import { Button } from "../ui/Button";
import { lineDiff } from "./lineDiff";
import { DiffTable } from "./DiffTable";

type Mode = "diff" | "full";

/**
 * View a past revision. Defaults to a line-level diff against the current
 * working copy (`current`) — the pattern Notion / VS Code Timeline / Obsidian
 * converge on — with a Diff/Full toggle to fall back to the raw revision body.
 * Each diff row carries old/new gutter line numbers, GitHub/VS Code style.
 */
export function RevisionView({
  revision,
  contents,
  current,
  onBack,
  onRestore,
}: {
  revision: string;
  contents: string;
  current: string;
  onBack: () => void;
  onRestore: () => void;
}) {
  const [mode, setMode] = useState<Mode>("diff");
  const rows = useMemo(() => lineDiff(contents, current), [contents, current]);

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center gap-2 border-b border-accent/40 bg-accent/10 px-3 py-2 text-xs text-text">
        <span>
          Viewing <span className="font-mono">{revision}</span> — read-only
        </span>
        <div
          className="ml-2 flex overflow-hidden rounded border border-border"
          role="group"
          aria-label="View mode"
        >
          <button
            type="button"
            aria-pressed={mode === "diff"}
            onClick={() => setMode("diff")}
            className={
              "px-2 py-0.5 " +
              (mode === "diff"
                ? "bg-accent text-accent-fg"
                : "text-muted hover:text-text")
            }
          >
            Diff
          </button>
          <button
            type="button"
            aria-pressed={mode === "full"}
            onClick={() => setMode("full")}
            className={
              "px-2 py-0.5 " +
              (mode === "full"
                ? "bg-accent text-accent-fg"
                : "text-muted hover:text-text")
            }
          >
            Full
          </button>
        </div>
        <div className="ml-auto flex gap-2">
          <Button variant="ghost" onClick={onBack}>
            ← Back to current
          </Button>
          <Button variant="primary" onClick={onRestore}>
            Restore
          </Button>
        </div>
      </div>
      {mode === "diff" ? (
        <DiffTable rows={rows} />
      ) : (
        <pre className="min-h-0 flex-1 overflow-auto whitespace-pre-wrap p-3 font-mono text-sm text-muted">
          {contents}
        </pre>
      )}
    </div>
  );
}
