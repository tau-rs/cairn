import { describe, it, expect } from "vitest";
import {
  COMMAND_DEFS,
  effectiveBinding,
  chordToId,
  findConflict,
} from "./commands";

describe("COMMAND_DEFS", () => {
  it("includes the 13 commands with unique default chords", () => {
    expect(COMMAND_DEFS).toHaveLength(13);
    const chords = COMMAND_DEFS.map((c) => c.defaultBinding);
    const bound = chords.filter((c): c is string => c !== null);
    expect(new Set(bound).size).toBe(bound.length); // unique among bound chords
  });
});

describe("effectiveBinding", () => {
  it("returns the default with no override", () => {
    expect(effectiveBinding("new-note", {})).toBe("Mod+N");
  });
  it("an override beats the default; null unbinds", () => {
    expect(effectiveBinding("new-note", { "new-note": "Mod+Shift+N" })).toBe(
      "Mod+Shift+N",
    );
    expect(effectiveBinding("new-note", { "new-note": null })).toBeNull();
  });
});

describe("chordToId", () => {
  it("inverts effective bindings and skips unbound", () => {
    const m = chordToId({ "close-tab": null });
    expect(m["Mod+K"]).toBe("open-palette");
    expect(m["Mod+W"]).toBeUndefined();
  });
  it("reflects an override", () => {
    const m = chordToId({ "new-note": "Mod+J" });
    expect(m["Mod+J"]).toBe("new-note");
    expect(m["Mod+N"]).toBeUndefined();
  });
});

describe("findConflict", () => {
  it("finds the command holding a chord, ignoring exceptId", () => {
    expect(findConflict({}, "Mod+K", "new-note")).toBe("open-palette");
    expect(findConflict({}, "Mod+K", "open-palette")).toBeNull();
    expect(findConflict({}, "Mod+Shift+Z", "new-note")).toBeNull();
  });
});

it("includes a show-history command bound to Mod+Shift+H", () => {
  const def = COMMAND_DEFS.find((d) => d.id === "show-history");
  expect(def).toBeDefined();
  expect(def?.defaultBinding).toBe("Mod+Shift+H");
});
