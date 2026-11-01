import { useEffect, useState } from "react";
import { AnswerView } from "./AnswerView";
import type { AskSurfaceProps } from "./AskBar";

/** Docked right-side panel: full turn list + docked composer. Renders null when
 *  closed so the shell region collapses. */
export function AskPanel(props: AskSurfaceProps) {
  const { open, turns, streaming, error, onSubmit, onClose, onOpenNote } =
    props;
  const [value, setValue] = useState("");
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);
  if (!open) return null;

  const submit = () => {
    const q = value.trim();
    if (!q) return;
    onSubmit(q);
    setValue("");
  };
  const lastIdx = turns.length - 1;

  return (
    <aside
      data-testid="ask-panel"
      className="flex w-[340px] shrink-0 flex-col border-l border-border bg-surface"
    >
      <div className="flex items-center justify-between border-b border-border px-3 py-2 text-sm font-semibold text-accent">
        <span>Ask ✦</span>
        <button
          aria-label="Close ask panel"
          className="text-faint"
          onClick={onClose}
        >
          ✕
        </button>
      </div>
      <div className="flex-1 overflow-y-auto p-2">
        {turns.map((t, i) => (
          <div key={i} className="my-1.5">
            <AnswerView
              turn={t}
              streaming={streaming && i === lastIdx && t.role === "assistant"}
              onOpenNote={onOpenNote}
            />
          </div>
        ))}
        {error && (
          <div data-testid="ask-error" className="m-1.5 text-sm text-danger">
            ⚠ {error}
          </div>
        )}
      </div>
      <div className="flex gap-2 border-t border-border p-2">
        <input
          value={value}
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              submit();
            }
          }}
          placeholder="Ask a follow-up…"
          className="flex-1 rounded-md border border-border bg-bg px-2 py-1.5 text-sm text-text focus:border-accent focus:outline-none"
        />
        <button
          aria-label="Send"
          className="rounded-md bg-accent px-3 text-accent-fg disabled:opacity-40"
          onClick={submit}
          disabled={streaming}
        >
          ↑
        </button>
      </div>
    </aside>
  );
}
