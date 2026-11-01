import { describe, it, expect } from "vitest";
import { eventToChord, isValidBinding, formatChord } from "./keybinding";

const ev = (o: Partial<KeyboardEvent>) => o as KeyboardEvent;

describe("eventToChord", () => {
  it("builds Mod+letter from meta or ctrl", () => {
    expect(eventToChord(ev({ key: "k", metaKey: true }))).toBe("Mod+K");
    expect(eventToChord(ev({ key: "k", ctrlKey: true }))).toBe("Mod+K");
  });
  it("includes Shift / Alt and named keys", () => {
    expect(eventToChord(ev({ key: "g", metaKey: true, shiftKey: true }))).toBe(
      "Mod+Shift+G",
    );
    expect(eventToChord(ev({ key: "Enter", metaKey: true }))).toBe("Mod+Enter");
    expect(eventToChord(ev({ key: ",", metaKey: true }))).toBe("Mod+,");
  });
  it("returns null for a pure modifier press", () => {
    expect(eventToChord(ev({ key: "Meta", metaKey: true }))).toBeNull();
    expect(eventToChord(ev({ key: "Shift", shiftKey: true }))).toBeNull();
  });
});

describe("isValidBinding", () => {
  it("requires the Mod modifier", () => {
    expect(isValidBinding("Mod+K")).toBe(true);
    expect(isValidBinding("Mod+Shift+G")).toBe(true);
    expect(isValidBinding("K")).toBe(false);
    expect(isValidBinding("Shift+K")).toBe(false);
  });
});

describe("formatChord", () => {
  it("renders mac glyphs", () => {
    expect(formatChord("Mod+Shift+G", true)).toBe("⌘⇧G");
    expect(formatChord("Mod+Enter", true)).toBe("⌘↵");
    expect(formatChord("Mod+,", true)).toBe("⌘,");
  });
  it("renders non-mac text", () => {
    expect(formatChord("Mod+K", false)).toBe("Ctrl+K");
    expect(formatChord("Mod+Shift+G", false)).toBe("Ctrl+Shift+G");
  });
});
