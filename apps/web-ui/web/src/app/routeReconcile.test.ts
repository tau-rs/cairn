import { describe, it, expect } from "vitest";
import { urlToStore, storeToUrl } from "./routeReconcile";

const loc = (pathname: string) => ({ pathname });

describe("urlToStore", () => {
  it("opens the note named in the URL when it isn't already active", () => {
    expect(
      urlToStore({
        location: loc("/note/b.md"),
        activePath: "a.md",
        activeTag: null,
        searchActive: false,
      }),
    ).toEqual([{ kind: "openNote", path: "b.md" }]);
  });

  it("does nothing when the URL note is already active", () => {
    expect(
      urlToStore({
        location: loc("/note/a.md"),
        activePath: "a.md",
        activeTag: null,
        searchActive: false,
      }),
    ).toEqual([]);
  });

  it("filters by tag on a tag route (only when not already that tag)", () => {
    expect(
      urlToStore({
        location: loc("/tags/x"),
        activePath: null,
        activeTag: null,
        searchActive: false,
      }),
    ).toEqual([{ kind: "filterByTag", tag: "x" }]);
    expect(
      urlToStore({
        location: loc("/tags/x"),
        activePath: null,
        activeTag: "x",
        searchActive: false,
      }),
    ).toEqual([]);
  });

  it("clears an active filter/search, then opens, when navigating to a note", () => {
    expect(
      urlToStore({
        location: loc("/note/b.md"),
        activePath: "a.md",
        activeTag: "x",
        searchActive: false,
      }),
    ).toEqual([{ kind: "closeSearch" }, { kind: "openNote", path: "b.md" }]);
  });

  it("clears search then loads the graph on the graph route", () => {
    expect(
      urlToStore({
        location: loc("/graph"),
        activePath: "a.md",
        activeTag: null,
        searchActive: true,
      }),
    ).toEqual([{ kind: "closeSearch" }, { kind: "loadGraph" }]);
  });

  it("does nothing on the root route with nothing active", () => {
    expect(
      urlToStore({
        location: loc("/"),
        activePath: null,
        activeTag: null,
        searchActive: false,
      }),
    ).toEqual([]);
  });
});

describe("storeToUrl", () => {
  it("reflects a restored note when the URL is at root", () => {
    expect(
      storeToUrl({
        location: loc("/"),
        activePath: "r.md",
        prevActivePath: "r.md",
      }),
    ).toEqual({ kind: "navigate", to: "/note/r.md" });
  });

  it("follows a store-origin change (URL trailing the old note)", () => {
    expect(
      storeToUrl({
        location: loc("/note/a.md"),
        activePath: "b.md",
        prevActivePath: "a.md",
      }),
    ).toEqual({ kind: "navigate", to: "/note/b.md" });
  });

  it("goes to root when the last tab closes", () => {
    expect(
      storeToUrl({
        location: loc("/note/a.md"),
        activePath: null,
        prevActivePath: "a.md",
      }),
    ).toEqual({ kind: "navigate", to: "/" });
  });

  it("stays out when the URL leads to a fresh target (deep link / user nav)", () => {
    expect(
      storeToUrl({
        location: loc("/note/x.md"),
        activePath: "r.md",
        prevActivePath: "r.md",
      }),
    ).toEqual({ kind: "none" });
  });

  it("does nothing when already in sync", () => {
    expect(
      storeToUrl({
        location: loc("/note/a.md"),
        activePath: "a.md",
        prevActivePath: "a.md",
      }),
    ).toEqual({ kind: "none" });
  });

  it("never overrides the graph or tag routes", () => {
    expect(
      storeToUrl({
        location: loc("/graph"),
        activePath: "a.md",
        prevActivePath: "b.md",
      }),
    ).toEqual({ kind: "none" });
    expect(
      storeToUrl({
        location: loc("/tags/x"),
        activePath: "a.md",
        prevActivePath: "b.md",
      }),
    ).toEqual({ kind: "none" });
  });
});
