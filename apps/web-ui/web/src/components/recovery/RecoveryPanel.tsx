import { useEffect, useRef, useState } from "react";
import { RecoveryBlock } from "./RecoveryBlock";
import { toRecoveryItems, blockLabel } from "./recoveryModel";
import type { WireRecoverableBlock, WireBlockId } from "../../contract";

interface RecoveryPanelProps {
  open: boolean;
  note: string | null;
  status: "idle" | "loading" | "ready" | "error";
  blocks: WireRecoverableBlock[];
  error: string | null;
  restoring: string | null;
  currentText: string;
  restoreEnabled: boolean;
  onCopy(text: string): void;
  onRestore(id: WireBlockId, versionIndex: number): void;
  onClose(): void;
}

const SPLIT_MIN_WIDTH = 520;

export function RecoveryPanel(props: RecoveryPanelProps) {
  const {
    open,
    note,
    status,
    blocks,
    error,
    restoring,
    currentText,
    restoreEnabled,
    onCopy,
    onRestore,
    onClose,
  } = props;

  const rootRef = useRef<HTMLDivElement | null>(null);
  const [layout, setLayout] = useState<"split" | "unified">("unified");

  useEffect(() => {
    if (typeof ResizeObserver === "undefined") return;
    const node = rootRef.current;
    if (!node) return;
    const observer = new ResizeObserver((entries) => {
      const width = entries[0]?.contentRect.width ?? 0;
      setLayout(width >= SPLIT_MIN_WIDTH ? "split" : "unified");
    });
    observer.observe(node);
    return () => observer.disconnect();
  }, [open]);

  if (!open) return null;

  const items = toRecoveryItems(blocks);
  const deleted = items.filter((i) => i.kind === "deleted");
  const overwritten = items.filter((i) => i.kind === "overwritten");

  return (
    <aside
      ref={rootRef}
      data-testid="recovery-panel"
      className="flex flex-col border-l border-border bg-surface"
    >
      <div className="flex items-center justify-between border-b border-border px-3 py-2 text-sm font-semibold text-accent">
        <span>Recovery{note ? ` — ${note}` : ""}</span>
        <button
          aria-label="Close recovery"
          className="text-faint"
          onClick={onClose}
        >
          ✕
        </button>
      </div>
      <div className="flex-1 overflow-y-auto p-2">
        {status === "loading" && (
          <div className="p-2 text-sm text-muted">Loading…</div>
        )}
        {status === "error" && (
          <div className="p-2 text-sm text-danger">{error}</div>
        )}
        {status === "ready" && items.length === 0 && (
          <div className="p-2 text-sm text-muted">
            No recoverable content for this note.
          </div>
        )}
        {status === "ready" && items.length > 0 && (
          <div className="flex flex-col gap-4">
            {deleted.length > 0 && (
              <div className="flex flex-col gap-2">
                <div className="text-xs font-medium text-faint">
                  Deleted ({deleted.length})
                </div>
                {deleted.map((item) => (
                  <RecoveryBlock
                    key={blockLabel(item.id)}
                    item={item}
                    currentText={currentText}
                    layout={layout}
                    restoreEnabled={restoreEnabled}
                    restoring={restoring === blockLabel(item.id)}
                    onCopy={onCopy}
                    onRestore={(versionIndex) =>
                      onRestore(item.id, versionIndex)
                    }
                  />
                ))}
              </div>
            )}
            {overwritten.length > 0 && (
              <div className="flex flex-col gap-2">
                <div className="text-xs font-medium text-faint">
                  Overwritten ({overwritten.length})
                </div>
                {overwritten.map((item) => (
                  <RecoveryBlock
                    key={blockLabel(item.id)}
                    item={item}
                    currentText={currentText}
                    layout={layout}
                    restoreEnabled={restoreEnabled}
                    restoring={restoring === blockLabel(item.id)}
                    onCopy={onCopy}
                    onRestore={(versionIndex) =>
                      onRestore(item.id, versionIndex)
                    }
                  />
                ))}
              </div>
            )}
          </div>
        )}
      </div>
    </aside>
  );
}
