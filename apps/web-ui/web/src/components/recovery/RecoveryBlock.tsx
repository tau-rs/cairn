import { Button } from "../ui/Button";
import { lineDiff, type DiffRow } from "../history/lineDiff";
import { blockLabel, type RecoveryItem } from "./recoveryModel";

const ROW_STYLE: Record<DiffRow["type"], string> = {
  add: "bg-success-bg text-success",
  del: "bg-danger-bg text-danger",
  ctx: "text-muted",
};
const ROW_SIGN: Record<DiffRow["type"], string> = {
  add: "+",
  del: "-",
  ctx: " ",
};

const KIND_BADGE: Record<RecoveryItem["kind"], string> = {
  deleted: "bg-danger-bg text-danger",
  overwritten: "bg-accent/15 text-accent",
};

function DiffRowLine({ row }: { row: DiffRow }) {
  return (
    <div
      data-diff-row={row.type}
      className={"flex whitespace-pre-wrap " + ROW_STYLE[row.type]}
    >
      <span
        aria-hidden
        className="w-10 shrink-0 select-none px-1 text-right text-faint"
      >
        {row.oldLine ?? ""}
      </span>
      <span
        aria-hidden
        className="w-10 shrink-0 select-none px-1 text-right text-faint"
      >
        {row.newLine ?? ""}
      </span>
      <span aria-hidden className="mr-2 select-none opacity-60">
        {ROW_SIGN[row.type]}
      </span>
      <span className="min-w-0 flex-1">{row.text}</span>
    </div>
  );
}

interface RecoveryBlockProps {
  item: RecoveryItem;
  /** Current note text = diff base. */
  currentText: string;
  layout: "split" | "unified";
  /** false → Restore disabled w/ tooltip. */
  restoreEnabled: boolean;
  restoring: boolean;
  onCopy(text: string): void;
  onRestore(versionIndex: number): void;
}

export function RecoveryBlock({
  item,
  currentText,
  layout,
  restoreEnabled,
  restoring,
  onCopy,
  onRestore,
}: RecoveryBlockProps) {
  const restoreTitle = restoreEnabled
    ? "Restore this block"
    : "needs collab session";

  return (
    <div className="rounded-md border border-border">
      <div className="flex items-center gap-2 border-b border-border px-3 py-2 text-xs">
        <span
          className={
            "rounded px-1.5 py-0.5 font-medium " + KIND_BADGE[item.kind]
          }
        >
          {item.kind}
        </span>
        <span className="font-mono text-faint">{blockLabel(item.id)}</span>
      </div>
      <div className="divide-y divide-border">
        {item.versions.map((version, index) => {
          const rows = lineDiff(version, currentText);
          return (
            <div key={index} className="p-3">
              <div className="mb-2 flex items-center justify-between">
                <span className="text-xs text-muted">Version {index + 1}</span>
                <div className="flex gap-2">
                  <Button variant="secondary" onClick={() => onCopy(version)}>
                    Copy
                  </Button>
                  <Button
                    variant="primary"
                    onClick={() => onRestore(index)}
                    disabled={!restoreEnabled || restoring}
                    title={restoreTitle}
                  >
                    Restore
                  </Button>
                </div>
              </div>
              {layout === "unified" ? (
                <div className="font-mono text-sm">
                  {rows.map((row, i) => (
                    <DiffRowLine key={i} row={row} />
                  ))}
                </div>
              ) : (
                <div className="grid grid-cols-2 gap-2 font-mono text-sm">
                  <div>
                    <div className="mb-1 text-xs text-faint">Lost version</div>
                    {rows
                      .filter((row) => row.type !== "add")
                      .map((row, i) => (
                        <DiffRowLine key={i} row={row} />
                      ))}
                  </div>
                  <div>
                    <div className="mb-1 text-xs text-faint">Current note</div>
                    {rows
                      .filter((row) => row.type !== "del")
                      .map((row, i) => (
                        <DiffRowLine key={i} row={row} />
                      ))}
                  </div>
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
