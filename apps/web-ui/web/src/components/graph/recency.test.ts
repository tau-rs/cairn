import { describe, it, expect, beforeEach } from "vitest";
import {
  recencyRing,
  DEFAULT_RECENCY,
  RECENCY_WINDOW_RANGE,
  loadRecency,
  saveRecency,
} from "./recency";

describe("recencyRing", () => {
  const now = 1_000_000; // secs
  const DAY = 86_400;
  it("full ring when edited now", () => {
    expect(recencyRing(now, now, 30)).toEqual({ alpha: 1, width: 4 });
  });
  it("null when older than the window", () => {
    expect(recencyRing(now - 31 * DAY, now, 30)).toBeNull();
  });
  it("ramps down linearly across the window", () => {
    const mid = recencyRing(now - 15 * DAY, now, 30)!;
    expect(mid.alpha).toBeCloseTo(0.5, 1);
    expect(mid.width).toBeGreaterThan(1);
    expect(mid.width).toBeLessThan(4);
  });
  it("future/negative age clamps to full, not >1", () => {
    expect(recencyRing(now + DAY, now, 30)!.alpha).toBe(1);
  });
  it("DEFAULT_RECENCY is off, 30 days", () => {
    expect(DEFAULT_RECENCY).toEqual({ enabled: false, windowDays: 30 });
  });
});

describe("loadRecency / saveRecency", () => {
  beforeEach(() => localStorage.clear());

  it("returns the default when nothing is stored", () => {
    expect(loadRecency()).toEqual(DEFAULT_RECENCY);
  });
  it("round-trips a saved setting", () => {
    saveRecency({ enabled: true, windowDays: 45 });
    expect(loadRecency()).toEqual({ enabled: true, windowDays: 45 });
  });
  it("coerces enabled and clamps windowDays into range", () => {
    saveRecency({
      enabled: 1 as unknown as boolean,
      windowDays: 9999,
    });
    expect(loadRecency()).toEqual({
      enabled: true,
      windowDays: RECENCY_WINDOW_RANGE.max,
    });
    saveRecency({ enabled: false, windowDays: -5 });
    expect(loadRecency()).toEqual({
      enabled: false,
      windowDays: RECENCY_WINDOW_RANGE.min,
    });
  });
  it("falls back to the default window when windowDays is not a number", () => {
    localStorage.setItem(
      "cairn.graph.recency",
      JSON.stringify({ enabled: true, windowDays: "soon" }),
    );
    expect(loadRecency()).toEqual({
      enabled: true,
      windowDays: DEFAULT_RECENCY.windowDays,
    });
  });
  it("returns the default on malformed JSON", () => {
    localStorage.setItem("cairn.graph.recency", "{not json");
    expect(loadRecency()).toEqual(DEFAULT_RECENCY);
  });
});
