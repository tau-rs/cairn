import { StateEffect, StateField, type EditorState } from "@codemirror/state";

export interface TableFocusTarget {
  /** Document position of the table block's start (its `from`). */
  pos: number;
  /** Body row index, or -1 for a header cell. */
  row: number;
  /** Column index. */
  col: number;
}

export const setTableFocus = StateEffect.define<TableFocusTarget | null>();

export const tableFocus = StateField.define<TableFocusTarget | null>({
  create: () => null,
  update(value, tr) {
    for (const e of tr.effects) if (e.is(setTableFocus)) return e.value;
    return value;
  },
});

export function readTableFocus(state: EditorState): TableFocusTarget | null {
  return state.field(tableFocus, false) ?? null;
}
