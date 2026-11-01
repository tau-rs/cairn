import type { ReactNode } from "react";
import type { AskTurn } from "../../store/askReducer";
import { stem } from "../../client/wikilink";

const CITE = /\[\[([^\]]+)\]\]/g;

/** Render plain text, turning `[[target]]` wikilinks into clickable buttons. */
function renderText(
  text: string,
  onOpenNote: (target: string) => void,
): ReactNode[] {
  const parts: ReactNode[] = [];
  let last = 0;
  let key = 0;
  let m: RegExpExecArray | null;
  CITE.lastIndex = 0;
  while ((m = CITE.exec(text)) !== null) {
    if (m.index > last) parts.push(text.slice(last, m.index));
    const target = m[1].split("|")[0].trim();
    parts.push(
      <button
        key={`c${key++}`}
        className="text-accent underline underline-offset-2"
        onClick={() => onOpenNote(target)}
      >
        {target}
      </button>,
    );
    last = m.index + m[0].length;
  }
  if (last < text.length) parts.push(text.slice(last));
  return parts;
}

export function AnswerView(props: {
  turn: AskTurn;
  /** Is this the live, streaming turn? Controls the caret. */
  streaming: boolean;
  onOpenNote: (target: string) => void;
}) {
  const { turn, streaming, onOpenNote } = props;
  const isUser = turn.role === "user";
  return (
    <div
      className={
        isUser
          ? "ml-8 rounded-lg bg-accent/15 px-3 py-2 text-sm text-text"
          : "rounded-lg bg-surface-2 px-3 py-2 text-sm text-text"
      }
    >
      {!isUser &&
        turn.tools.map((t, i) => (
          <div
            key={i}
            data-testid="tool"
            className="mb-1 flex items-center gap-2 text-xs text-accent"
          >
            <span className="inline-block h-1.5 w-1.5 rounded-full bg-current" />
            {t.ok === null
              ? `running ${t.tool}…`
              : t.ok
                ? `${t.tool} ✓`
                : `${t.tool} ✗`}
          </div>
        ))}
      <span className="whitespace-pre-wrap">
        {isUser ? turn.text : renderText(turn.text, onOpenNote)}
        {streaming && (
          <span
            data-testid="caret"
            className="ml-0.5 inline-block h-3.5 w-1 animate-pulse bg-accent align-text-bottom"
          />
        )}
      </span>
      {!isUser && turn.citations.length > 0 && (
        <div
          data-testid="sources"
          className="mt-1 border-t border-border pt-1 text-xs text-faint"
        >
          Sources:{" "}
          {turn.citations.map((c, i) => (
            <button
              key={c}
              className="mr-2 text-accent underline"
              onClick={() => onOpenNote(c)}
            >
              {i + 1} {stem(c)}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
