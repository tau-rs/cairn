import { describe, it, expect, beforeEach } from "vitest";
import {
  selectionToRequest,
  loadTemporalOpen,
  saveTemporalOpen,
} from "./temporalControls";
import type { Revision } from "../../contract";

const tl: Revision[] = [
  {
    id: "c3",
    message: "third",
    timestamp_secs: 30,
    author: "a",
    summary: null,
    name: null,
  }, // newest
  {
    id: "c2",
    message: "second",
    timestamp_secs: 20,
    author: "a",
    summary: null,
    name: null,
  },
  {
    id: "c1",
    message: "first",
    timestamp_secs: 10,
    author: "a",
    summary: null,
    name: null,
  }, // oldest
];

describe("selectionToRequest", () => {
  it("maps live to a live request", () => {
    expect(selectionToRequest({ kind: "live" }, tl)).toEqual({ mode: "live" });
  });

  it("maps snapshot to graph_at at that revision id", () => {
    expect(selectionToRequest({ kind: "snapshot", at: 1 }, tl)).toEqual({
      mode: "snapshot",
      revision: "c2",
    });
  });

  it("maps a range to compare with older=from, newer=to", () => {
    expect(selectionToRequest({ kind: "compare", from: 2, to: 0 }, tl)).toEqual(
      { mode: "compare", from: "c1", to: "c3" },
    );
  });

  it("degrades a collapsed range to snapshot", () => {
    expect(selectionToRequest({ kind: "compare", from: 1, to: 1 }, tl)).toEqual(
      { mode: "snapshot", revision: "c2" },
    );
  });

  it("falls back to live on empty timeline or out-of-range index", () => {
    expect(selectionToRequest({ kind: "snapshot", at: 0 }, null)).toEqual({
      mode: "live",
    });
    expect(selectionToRequest({ kind: "snapshot", at: 9 }, tl)).toEqual({
      mode: "live",
    });
    expect(selectionToRequest({ kind: "compare", from: 9, to: 0 }, tl)).toEqual(
      { mode: "live" },
    );
  });
});

describe("open persistence", () => {
  beforeEach(() => localStorage.clear());
  it("defaults to false and round-trips", () => {
    expect(loadTemporalOpen()).toBe(false);
    saveTemporalOpen(true);
    expect(loadTemporalOpen()).toBe(true);
  });
});
