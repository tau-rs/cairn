import { describe, it, expect, beforeEach, vi } from "vitest";
import {
  DEFAULT_FORCE_SETTINGS,
  clampForceSettings,
  loadForceSettings,
  saveForceSettings,
} from "./forceSettings";

beforeEach(() => localStorage.clear());

describe("clampForceSettings", () => {
  it("clamps each numeric field to its range", () => {
    const c = clampForceSettings({
      center: 9,
      repel: 9999,
      linkForce: -1,
      linkDistance: 5,
      frozen: true,
    });
    expect(c.center).toBe(1);
    expect(c.repel).toBe(0);
    expect(c.linkForce).toBe(0);
    expect(c.linkDistance).toBe(10);
    expect(c.frozen).toBe(true);
  });
});

describe("loadForceSettings", () => {
  it("returns defaults when localStorage is empty", () => {
    expect(loadForceSettings()).toEqual(DEFAULT_FORCE_SETTINGS);
  });
  it("returns defaults when the stored value is corrupt JSON", () => {
    localStorage.setItem("cairn.graph.forces", "{not json");
    expect(loadForceSettings()).toEqual(DEFAULT_FORCE_SETTINGS);
  });
  it("merges partial stored values over defaults and clamps", () => {
    localStorage.setItem(
      "cairn.graph.forces",
      JSON.stringify({ repel: -9999, linkDistance: 120 }),
    );
    const s = loadForceSettings();
    expect(s.repel).toBe(-800); // clamped
    expect(s.linkDistance).toBe(120);
    expect(s.center).toBe(DEFAULT_FORCE_SETTINGS.center); // default
  });
  it("round-trips a saved settings object", () => {
    const s = { ...DEFAULT_FORCE_SETTINGS, repel: -300, frozen: true };
    saveForceSettings(s);
    expect(loadForceSettings()).toEqual(s);
  });
});

describe("saveForceSettings", () => {
  it("swallows storage errors", () => {
    const spy = vi
      .spyOn(Storage.prototype, "setItem")
      .mockImplementation(() => {
        throw new Error("quota");
      });
    expect(() => saveForceSettings(DEFAULT_FORCE_SETTINGS)).not.toThrow();
    spy.mockRestore();
  });
});
