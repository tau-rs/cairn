import { describe, it, expect, beforeEach } from "vitest";
import {
  applyFilters,
  DEFAULT_FILTER,
  loadFilter,
  saveFilter,
} from "./graphFilter";
import type { ColorGroup } from "./colorGroups";
import type { GraphNode } from "../../contract";

const n = (path: string, degree: number, tags: string[] = []): GraphNode => ({
  path,
  title: path,
  degree,
  tags,
  mtime_secs: 0,
});
const groups: ColorGroup[] = [
  { kind: "tag", query: "topic", color: "#6366f1" },
];

describe("applyFilters", () => {
  it("passthrough on defaults", () => {
    const nodes = [n("a", 1, ["topic"]), n("b", 0)];
    const edges = [{ from: "a", to: "b" }];
    expect(applyFilters(nodes, edges, DEFAULT_FILTER, groups)).toEqual({
      nodes,
      edges,
    });
  });
  it("min-degree hides low-degree nodes and their edges", () => {
    const nodes = [n("a", 2, ["topic"]), n("b", 1)];
    const edges = [{ from: "a", to: "b" }];
    const r = applyFilters(
      nodes,
      edges,
      { ...DEFAULT_FILTER, minDegree: 2 },
      groups,
    );
    expect(r.nodes.map((x) => x.path)).toEqual(["a"]);
    expect(r.edges).toEqual([]);
  });
  it("hidden group query hides matching nodes", () => {
    const nodes = [n("a", 1, ["topic"]), n("b", 1, ["other"])];
    const r = applyFilters(
      nodes,
      [],
      { ...DEFAULT_FILTER, hiddenGroupQueries: ["topic"] },
      groups,
    );
    expect(r.nodes.map((x) => x.path)).toEqual(["b"]);
  });
  it("hideUngrouped hides nodes matching no group", () => {
    const nodes = [n("a", 1, ["topic"]), n("b", 1, [])];
    const r = applyFilters(
      nodes,
      [],
      { ...DEFAULT_FILTER, hideUngrouped: true },
      groups,
    );
    expect(r.nodes.map((x) => x.path)).toEqual(["a"]);
  });
});

describe("loadFilter / saveFilter", () => {
  beforeEach(() => localStorage.clear());

  it("returns the default when nothing is stored", () => {
    expect(loadFilter()).toEqual(DEFAULT_FILTER);
  });
  it("round-trips a saved setting", () => {
    const f = {
      minDegree: 3,
      hiddenGroupQueries: ["topic"],
      hideUngrouped: true,
    };
    saveFilter(f);
    expect(loadFilter()).toEqual(f);
  });
  it("floors minDegree at 0, coerces hideUngrouped, and sanitizes queries", () => {
    localStorage.setItem(
      "cairn.graph.filter",
      JSON.stringify({
        minDegree: -4,
        hiddenGroupQueries: ["ok", 5, null],
        hideUngrouped: 1,
      }),
    );
    expect(loadFilter()).toEqual({
      minDegree: 0,
      hiddenGroupQueries: ["ok"],
      hideUngrouped: true,
    });
  });
  it("falls back to defaults for wrong-typed fields", () => {
    localStorage.setItem(
      "cairn.graph.filter",
      JSON.stringify({ minDegree: "lots", hiddenGroupQueries: "nope" }),
    );
    expect(loadFilter()).toEqual({
      minDegree: 0,
      hiddenGroupQueries: [],
      hideUngrouped: false,
    });
  });
  it("returns the default on malformed JSON", () => {
    localStorage.setItem("cairn.graph.filter", "{bad");
    expect(loadFilter()).toEqual(DEFAULT_FILTER);
  });
});
