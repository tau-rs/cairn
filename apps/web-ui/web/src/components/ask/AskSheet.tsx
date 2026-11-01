import { useState } from "react";
import { Drawer } from "../ui/Drawer";
import { AnswerView } from "./AnswerView";
import type { AskSurfaceProps } from "./AskBar";

/** Tablet/mobile conversation surface: the full turn list + composer inside a
 *  slide-in Drawer. `side="bottom"` is the mobile sheet, `side="right"` the
 *  tablet side sheet. The Drawer's Radix dialog supplies scrim + Escape close. */
export function AskSheet(
  props: AskSurfaceProps & { side: "bottom" | "right" },
) {
  const { open, turns, streaming, error, onSubmit, onClose, onOpenNote, side } =
    props;
  const [value, setValue] = useState("");

  const submit = () => {
    const q = value.trim();
    if (!q) return;
    onSubmit(q);
    setValue("");
  };
  const lastIdx = turns.length - 1;

  return (
    <Drawer open={open} onClose={onClose} side={side} label="Ask">
      <div data-testid="ask-sheet" className="flex h-full flex-col">
        <div className="mb-2 flex items-center justify-between text-sm font-semibold text-accent">
          <span>Ask ✦</span>
          <button
            aria-label="Close ask"
            className="text-faint"
            onClick={onClose}
          >
            ✕
          </button>
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto">
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
        <div className="mt-2 flex gap-2 border-t border-border pt-2">
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
      </div>
    </Drawer>
  );
}
