import { useEffect, useRef } from "react";

/** A positioned right-click menu for a tree row (note or folder). Closes on
 *  Escape, on an outside click, or after any item runs. Items are filtered by
 *  `kind`: notes can be opened/deleted, folders can spawn a new note. */
export function TreeContextMenu(props: {
  kind: "folder" | "note";
  x: number;
  y: number;
  onSetIcon: () => void;
  onOpen: () => void;
  onOpenToSide: () => void;
  onNewNote: () => void;
  onRename: () => void;
  onDelete: () => void;
  onRecover: () => void;
  onClose: () => void;
}) {
  const ref = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    ref.current?.focus();
    const onDocDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node))
        props.onClose();
    };
    document.addEventListener("mousedown", onDocDown);
    return () => document.removeEventListener("mousedown", onDocDown);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const run = (fn: () => void) => () => {
    fn();
    props.onClose();
  };

  const item = (
    label: string,
    fn: () => void,
    danger = false,
    hint?: string,
  ) => (
    <button
      type="button"
      role="menuitem"
      onClick={run(fn)}
      className={
        "flex w-full items-center justify-between gap-6 rounded px-2.5 py-1.5 text-left text-xs hover:bg-surface-2 " +
        (danger ? "text-danger" : "text-text")
      }
    >
      <span>{label}</span>
      {hint && <span className="text-faint">{hint}</span>}
    </button>
  );

  const isNote = props.kind === "note";

  return (
    <div
      ref={ref}
      role="menu"
      tabIndex={-1}
      onKeyDown={(e) => {
        if (e.key === "Escape") props.onClose();
      }}
      style={{ left: props.x, top: props.y }}
      className="fixed z-50 w-48 rounded-lg border border-border bg-surface p-1 shadow-xl outline-none"
    >
      {isNote && item("Open", props.onOpen, false, "↵")}
      {isNote && item("Open to the side", props.onOpenToSide, false, "⌘↵")}
      {!isNote && item("New note here", props.onNewNote)}
      <div className="my-1 h-px bg-border" />
      {item("Set icon…", props.onSetIcon)}
      {item("Rename", props.onRename, false, "F2")}
      {isNote && item("Recover lost work…", props.onRecover)}
      {isNote && item("Delete", props.onDelete, true)}
    </div>
  );
}
