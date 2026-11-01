import * as Dialog from "@radix-ui/react-dialog";
import { useState } from "react";
import { AnswerView } from "./AnswerView";
import type { AskTurn } from "../../store/askReducer";

/** Props shared by both ask surfaces (bar + panel). */
export interface AskSurfaceProps {
  open: boolean;
  turns: AskTurn[];
  streaming: boolean;
  error: string | null;
  onSubmit: (q: string) => void;
  onClose: () => void;
  onOpenNote: (target: string) => void;
}

/** Slim, NON-modal prompt bar (modal={false}: does not block the editor).
 *  Shows the latest assistant turn inline; ⤢ promotes into the docked panel. */
export function AskBar(
  props: AskSurfaceProps & { onPromote: () => void; canPromote?: boolean },
) {
  const {
    open,
    turns,
    streaming,
    error,
    onSubmit,
    onClose,
    onPromote,
    onOpenNote,
    canPromote = true,
  } = props;
  const [value, setValue] = useState("");
  const last = turns[turns.length - 1];
  const answer = last && last.role === "assistant" ? last : null;

  const submit = () => {
    const q = value.trim();
    if (!q) return;
    onSubmit(q);
    setValue("");
  };

  return (
    <Dialog.Root
      open={open}
      onOpenChange={(o) => {
        if (!o) onClose();
      }}
      modal={false}
    >
      <Dialog.Portal>
        <Dialog.Content
          aria-describedby={undefined}
          onInteractOutside={() => onClose()}
          className="fixed left-1/2 top-[15%] z-50 w-[min(92vw,560px)] -translate-x-1/2 overflow-hidden rounded-xl border border-accent bg-surface-2 text-text shadow-2xl focus:outline-none"
        >
          <Dialog.Title className="sr-only">Ask</Dialog.Title>
          <div className="flex items-center gap-2 border-b border-border px-3 py-3">
            <span className="text-accent" aria-hidden="true">
              ✦
            </span>
            <input
              autoFocus
              disabled={streaming}
              value={value}
              onChange={(e) => setValue(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  submit();
                }
              }}
              placeholder="Ask about your notes…"
              className="w-full bg-transparent text-sm text-text placeholder:text-faint focus:outline-none"
            />
          </div>
          {(answer || error) && (
            <div className="max-h-72 overflow-y-auto px-3 py-2">
              {answer && (
                <AnswerView
                  turn={answer}
                  streaming={streaming}
                  onOpenNote={onOpenNote}
                />
              )}
              {error && (
                <div
                  data-testid="ask-error"
                  className="mt-1 text-sm text-danger"
                >
                  ⚠ {error}
                </div>
              )}
            </div>
          )}
          <div className="flex items-center justify-between border-t border-border px-3 py-2 text-xs text-faint">
            {canPromote ? (
              <button
                className="rounded border border-accent px-2 py-0.5 text-accent"
                onClick={onPromote}
              >
                ⤢ Continue in panel
              </button>
            ) : (
              <span />
            )}
            <span>esc to close</span>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
