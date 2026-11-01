import type { DiffRow } from "./lineDiff";

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

export function DiffTable({ rows }: { rows: DiffRow[] }) {
  return (
    <div className="min-h-0 flex-1 overflow-auto p-3 font-mono text-sm">
      {rows.map((row, i) => (
        <div
          key={i}
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
      ))}
    </div>
  );
}
