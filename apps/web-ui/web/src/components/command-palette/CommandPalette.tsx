import * as Dialog from "@radix-ui/react-dialog";
import { useEffect, useMemo, useState } from "react";
import { stem } from "../../client/wikilink";
import { filterItems } from "./fuzzy";

export interface PaletteCommand {
  id: string;
  label: string;
  hint?: string;
}

type Item =
  | { kind: "command"; id: string; label: string; hint?: string }
  | { kind: "note"; id: string; label: string; path: string };

const EMPTY_NOTE_CAP = 6;

export function CommandPalette(props: {
  open: boolean;
  onClose: () => void;
  commands: PaletteCommand[];
  notes: string[];
  onRunCommand: (id: string) => void;
  onOpenNote: (path: string) => void;
}) {
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);

  // Reset query + highlight whenever the palette opens.
  useEffect(() => {
    if (props.open) {
      setQuery("");
      setIndex(0);
    }
  }, [props.open]);

  const results = useMemo<Item[]>(() => {
    const cmdItems: Item[] = props.commands.map((c) => ({
      kind: "command",
      id: c.id,
      label: c.label,
      hint: c.hint,
    }));
    const noteItems: Item[] = props.notes.map((p) => ({
      kind: "note",
      id: p,
      label: stem(p),
      path: p,
    }));
    const text = (i: Item) =>
      i.kind === "note" ? `${i.label} ${i.path}` : i.label;
    if (query.trim() === "") {
      return [...cmdItems, ...noteItems.slice(0, EMPTY_NOTE_CAP)];
    }
    return filterItems([...cmdItems, ...noteItems], query, text);
  }, [props.commands, props.notes, query]);

  // Keep the highlight in range when results shrink.
  useEffect(() => {
    setIndex((i) => Math.min(i, Math.max(0, results.length - 1)));
  }, [results.length]);

  const run = (item: Item | undefined) => {
    if (!item) return;
    if (item.kind === "command") props.onRunCommand(item.id);
    else props.onOpenNote(item.path);
    props.onClose();
  };

  const cmds = results.filter((r) => r.kind === "command");
  const noteRes = results.filter((r) => r.kind === "note");

  return (
    <Dialog.Root
      open={props.open}
      onOpenChange={(o) => {
        if (!o) props.onClose();
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-40 bg-black/50" />
        <Dialog.Content
          aria-describedby={undefined}
          className="fixed left-1/2 top-[15%] z-50 w-[min(92vw,520px)] -translate-x-1/2 overflow-hidden rounded-xl border border-border bg-surface text-text shadow-2xl focus:outline-none"
        >
          <Dialog.Title className="sr-only">Command palette</Dialog.Title>
          <input
            autoFocus
            className="w-full border-b border-border bg-transparent px-4 py-3 text-sm text-text placeholder:text-faint focus:outline-none"
            placeholder="Type a command or search notes…"
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setIndex(0);
            }}
            onKeyDown={(e) => {
              if (e.key === "ArrowDown") {
                e.preventDefault();
                setIndex((i) => Math.min(i + 1, results.length - 1));
              } else if (e.key === "ArrowUp") {
                e.preventDefault();
                setIndex((i) => Math.max(i - 1, 0));
              } else if (e.key === "Enter") {
                e.preventDefault();
                run(results[index]);
              }
            }}
          />
          <div className="max-h-80 overflow-y-auto py-1">
            {results.length === 0 && (
              <div className="px-4 py-3 text-xs text-faint">No matches</div>
            )}
            {cmds.length > 0 && (
              <Group label="Commands">
                {cmds.map((item) => (
                  <Row
                    key={item.id}
                    selected={results[index] === item}
                    onClick={() => run(item)}
                  >
                    {item.label}
                    <span className="ml-auto font-mono text-[11px] text-faint">
                      {item.kind === "command" ? (item.hint ?? "") : ""}
                    </span>
                  </Row>
                ))}
              </Group>
            )}
            {noteRes.length > 0 && (
              <Group label="Notes">
                {noteRes.map((item) => (
                  <Row
                    key={item.id}
                    selected={results[index] === item}
                    onClick={() => run(item)}
                  >
                    {item.label}
                    <span className="ml-auto text-[11px] text-faint">
                      {item.kind === "note" ? item.path : ""}
                    </span>
                  </Row>
                ))}
              </Group>
            )}
          </div>
          <div className="flex gap-4 border-t border-border px-4 py-2 text-[10px] text-faint">
            <span>↑↓ navigate</span>
            <span>↵ run / open</span>
            <span>esc close</span>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

function Group(props: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <div className="px-4 pb-1 pt-2 text-[9px] uppercase tracking-wide text-faint">
        {props.label}
      </div>
      {props.children}
    </div>
  );
}

function Row(props: {
  selected: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      className={
        "flex w-full items-center gap-2 px-4 py-1.5 text-left text-[13px] " +
        (props.selected
          ? "bg-surface-2 text-text"
          : "text-muted hover:bg-surface-2")
      }
      onMouseDown={(e) => {
        // run on mousedown so the input doesn't blur-close first
        e.preventDefault();
        props.onClick();
      }}
    >
      {props.children}
    </button>
  );
}
