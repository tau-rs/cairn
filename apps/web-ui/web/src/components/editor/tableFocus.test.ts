import { describe, it, expect } from "vitest";
import { EditorState } from "@codemirror/state";
import { tableFocus, setTableFocus, readTableFocus } from "./tableFocus";

describe("tableFocus field", () => {
  it("stores and reads a focus target", () => {
    let state = EditorState.create({ doc: "x", extensions: [tableFocus] });
    expect(readTableFocus(state)).toBeNull();
    state = state.update({
      effects: setTableFocus.of({ pos: 0, row: 1, col: 2 }),
    }).state;
    expect(readTableFocus(state)).toEqual({ pos: 0, row: 1, col: 2 });
  });
  it("clears the target when set to null", () => {
    let state = EditorState.create({ doc: "x", extensions: [tableFocus] });
    state = state.update({
      effects: setTableFocus.of({ pos: 0, row: 0, col: 0 }),
    }).state;
    state = state.update({ effects: setTableFocus.of(null) }).state;
    expect(readTableFocus(state)).toBeNull();
  });
});
