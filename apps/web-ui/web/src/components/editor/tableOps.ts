import { EditorView } from "@codemirror/view";
import { setTableFocus } from "./tableFocus";
import {
  parseTable,
  serializeTable,
  insertRow,
  removeRow,
  moveRow,
  insertColumn,
  removeColumn,
  moveColumn,
  pasteBlock,
} from "./tableParse";

export type TableOp =
  | { kind: "insertRow"; index: number }
  | { kind: "removeRow"; index: number }
  | { kind: "moveRow"; from: number; to: number }
  | { kind: "insertColumn"; index: number }
  | { kind: "removeColumn"; index: number }
  | { kind: "moveColumn"; from: number; to: number }
  | { kind: "paste"; atRow: number; atCol: number; block: string[][] };

/** PURE: apply a structural op to a table's markdown source, returning new source.
 *  For "paste", (atRow, atCol) are BODY coordinates (atRow 0 = first body row). */
export function computeTableEdit(md: string, op: TableOp): string {
  const m = parseTable(md);
  switch (op.kind) {
    case "insertRow":
      return serializeTable(insertRow(m, op.index));
    case "removeRow":
      return serializeTable(removeRow(m, op.index));
    case "moveRow":
      return serializeTable(moveRow(m, op.from, op.to));
    case "insertColumn":
      return serializeTable(insertColumn(m, op.index));
    case "removeColumn":
      return serializeTable(removeColumn(m, op.index));
    case "moveColumn":
      return serializeTable(moveColumn(m, op.from, op.to));
    case "paste":
      return serializeTable(pasteBlock(m, op.atRow, op.atCol, op.block));
  }
}

/** Apply a structural op to the table block spanning [from, to] in the document.
 *  Dispatches a single replace, parks the cursor at the block start (keeping the
 *  table editable), and records which cell to refocus after the widget rebuilds.
 *  `currentMd` overrides the doc slice as the op's input — the editable widget
 *  passes its DOM-captured model so in-progress cell typing is not lost. */
export function applyTableOp(
  view: EditorView,
  from: number,
  to: number,
  op: TableOp,
  focus: { row: number; col: number },
  currentMd: string = view.state.sliceDoc(from, to),
): void {
  const base = serializeTable(parseTable(currentMd));
  const next = computeTableEdit(currentMd, op);
  if (next === base) return; // structural no-op → no transaction
  view.dispatch({
    changes: { from, to, insert: next },
    selection: { anchor: from },
    effects: setTableFocus.of({ pos: from, row: focus.row, col: focus.col }),
  });
}
