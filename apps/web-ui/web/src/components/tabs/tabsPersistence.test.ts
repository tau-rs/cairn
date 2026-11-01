import { describe, it, expect, beforeEach } from "vitest";
import { saveTabs, loadTabs, savePanes, loadPanes } from "./tabsPersistence";
import type { TabsState } from "./tabsModel";
import type { PanesState } from "./paneModel";

beforeEach(() => localStorage.clear());

const state: TabsState = {
  tabs: [
    { path: "a.md", preview: false },
    { path: "b.md", preview: false },
    { path: "scratch.md", preview: true }, // preview — must NOT persist
  ],
  activePath: "b.md",
};

describe("tabsPersistence", () => {
  it("round-trips pinned paths + active, excluding the preview tab", () => {
    saveTabs(state);
    expect(loadTabs(["a.md", "b.md", "scratch.md"])).toEqual({
      pinned: ["a.md", "b.md"],
      activePath: "b.md",
    });
  });
  it("drops persisted paths that no longer exist", () => {
    saveTabs(state);
    expect(loadTabs(["a.md"])).toEqual({
      pinned: ["a.md"],
      activePath: "a.md",
    });
  });
  it("returns empty when nothing is stored", () => {
    expect(loadTabs(["a.md"])).toEqual({ pinned: [], activePath: null });
  });
  it("returns empty on malformed storage", () => {
    localStorage.setItem("cairn.tabs", "{not json");
    expect(loadTabs(["a.md"])).toEqual({ pinned: [], activePath: null });
  });
});

describe("savePanes / loadPanes", () => {
  it("round-trips pinned tabs per pane, focused index, and ratio", () => {
    const state: PanesState = {
      panes: [
        {
          tabs: [
            { path: "a.md", preview: false },
            { path: "x.md", preview: true },
          ],
          activePath: "a.md",
        },
        { tabs: [{ path: "b.md", preview: false }], activePath: "b.md" },
      ],
      activePane: 1,
    };
    savePanes({ ...state, ratio: 0.6 });
    const r = loadPanes(["a.md", "b.md", "x.md"]);
    // preview tab x.md is NOT persisted (only pinned)
    expect(r.panes).toEqual([
      { pinned: ["a.md"], activePath: "a.md" },
      { pinned: ["b.md"], activePath: "b.md" },
    ]);
    expect(r.activePane).toBe(1);
    expect(r.ratio).toBe(0.6);
  });

  it("drops paths that no longer exist", () => {
    savePanes({
      panes: [
        { tabs: [{ path: "gone.md", preview: false }], activePath: "gone.md" },
      ],
      activePane: 0,
      ratio: 0.5,
    });
    const r = loadPanes(["a.md"]);
    expect(r.panes[0].pinned).toEqual([]);
    expect(r.panes[0].activePath).toBeNull();
  });

  it("migrates the legacy single-group format", () => {
    // write the OLD shape under the same key
    localStorage.setItem(
      "cairn.tabs",
      JSON.stringify({ pinned: ["a.md", "b.md"], activePath: "b.md" }),
    );
    const r = loadPanes(["a.md", "b.md"]);
    expect(r.panes).toEqual([{ pinned: ["a.md", "b.md"], activePath: "b.md" }]);
    expect(r.activePane).toBe(0);
    expect(r.ratio).toBe(0.5);
  });

  it("returns a single empty pane when nothing is stored", () => {
    const r = loadPanes(["a.md"]);
    expect(r.panes).toEqual([{ pinned: [], activePath: null }]);
    expect(r.activePane).toBe(0);
    expect(r.ratio).toBe(0.5);
  });
});
