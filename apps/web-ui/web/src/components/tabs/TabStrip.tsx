import { useRef } from "react";
import { stem } from "../../client/wikilink";

export interface TabView {
  path: string;
  preview: boolean;
  dirty: boolean;
}

export function TabStrip(props: {
  tabs: TabView[];
  activePath: string | null;
  onSelect: (path: string) => void;
  onPin: (path: string) => void;
  onClose: (path: string) => void;
  /** If provided, renders a split-pane button (Task 6). */
  onSplit?: () => void;
  /** If provided, renders a close-pane button (Task 6). */
  onClosePane?: () => void;
}) {
  const tabRefs = useRef<(HTMLDivElement | null)[]>([]);
  if (props.tabs.length === 0 && !props.onSplit && !props.onClosePane)
    return null;

  // WAI-ARIA tabs, manual activation: arrows just move focus between tabs
  // (wrapping); Enter/Space activate the focused tab. Manual (not automatic)
  // activation is the pattern's recommendation when activating a tab is
  // expensive — here it navigates and can trigger a note load. Roving tabindex
  // keeps a single tab in the Tab order.
  const onTabKeyDown = (e: React.KeyboardEvent, index: number) => {
    const tabs = props.tabs;
    const focusAt = (i: number) => tabRefs.current[i]?.focus();
    switch (e.key) {
      case "ArrowRight":
        e.preventDefault();
        focusAt((index + 1) % tabs.length);
        break;
      case "ArrowLeft":
        e.preventDefault();
        focusAt((index - 1 + tabs.length) % tabs.length);
        break;
      case "Home":
        e.preventDefault();
        focusAt(0);
        break;
      case "End":
        e.preventDefault();
        focusAt(tabs.length - 1);
        break;
      case "Enter":
      case " ":
        e.preventDefault();
        props.onSelect(tabs[index].path);
        break;
    }
  };

  return (
    <div
      role="tablist"
      className="flex h-9 shrink-0 items-stretch overflow-x-auto border-b border-border bg-surface"
    >
      {props.tabs.map((t, index) => {
        const active = t.path === props.activePath;
        const label = stem(t.path);
        return (
          <div
            key={t.path}
            ref={(el) => {
              tabRefs.current[index] = el;
            }}
            role="tab"
            tabIndex={active ? 0 : -1}
            aria-selected={active}
            aria-label={label}
            title={t.path}
            onClick={() => props.onSelect(t.path)}
            onDoubleClick={() => props.onPin(t.path)}
            onKeyDown={(e) => onTabKeyDown(e, index)}
            className={
              "relative flex cursor-pointer items-center gap-2 whitespace-nowrap border-r border-border px-3 text-xs " +
              (active
                ? "bg-surface-2 text-text"
                : "text-muted hover:bg-surface-2 hover:text-text")
            }
          >
            {active && (
              <span className="absolute inset-x-0 top-0 h-0.5 bg-accent" />
            )}
            <span className={t.preview ? "italic" : ""}>{label}</span>
            {t.dirty && <span className="h-1.5 w-1.5 rounded-full bg-accent" />}
            <button
              type="button"
              aria-label={`close ${label}`}
              className="text-faint hover:text-text"
              onClick={(e) => {
                e.stopPropagation();
                props.onClose(t.path);
              }}
            >
              ×
            </button>
          </div>
        );
      })}
      {(props.onSplit || props.onClosePane) && (
        <div className="ml-auto flex shrink-0 items-center gap-1 border-l border-border px-1.5">
          {props.onSplit && (
            <button
              type="button"
              aria-label="Split editor right"
              title="Split editor right"
              onClick={props.onSplit}
              className="flex h-6 w-6 items-center justify-center rounded text-faint hover:bg-surface-2 hover:text-text"
            >
              <svg
                width="15"
                height="15"
                viewBox="0 0 16 16"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.4"
              >
                <rect x="2" y="2.5" width="12" height="11" rx="1.5" />
                <line x1="8" y1="2.5" x2="8" y2="13.5" />
              </svg>
            </button>
          )}
          {props.onClosePane && (
            <button
              type="button"
              aria-label="Close pane"
              title="Close pane"
              onClick={props.onClosePane}
              className="flex h-6 w-6 items-center justify-center rounded text-faint hover:bg-surface-2 hover:text-text"
            >
              <svg
                width="14"
                height="14"
                viewBox="0 0 16 16"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.4"
              >
                <rect x="2.5" y="2.5" width="11" height="11" rx="1.5" />
                <line x1="6" y1="6" x2="10" y2="10" />
                <line x1="10" y1="6" x2="6" y2="10" />
              </svg>
            </button>
          )}
        </div>
      )}
    </div>
  );
}
