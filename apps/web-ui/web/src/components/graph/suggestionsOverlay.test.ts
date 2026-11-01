import { describe, it, expect, beforeEach } from "vitest";
import {
  DEFAULT_SUGGESTIONS_SETTINGS,
  loadSuggestionsSettings,
  saveSuggestionsSettings,
  suggestionScopeFor,
} from "./suggestionsOverlay";

describe("suggestionScopeFor", () => {
  it("returns null when the overlay is disabled", () => {
    expect(suggestionScopeFor(false, true, "a.md")).toBeNull();
  });

  it("returns a note scope in local mode with an active note", () => {
    expect(suggestionScopeFor(true, true, "a.md")).toEqual({
      type: "note",
      path: "a.md",
    });
  });

  it("returns vault scope in global mode", () => {
    expect(suggestionScopeFor(true, false, "a.md")).toEqual({ type: "vault" });
  });

  it("returns vault scope in local mode when no note is active", () => {
    expect(suggestionScopeFor(true, true, null)).toEqual({ type: "vault" });
  });
});

describe("suggestions settings persistence", () => {
  beforeEach(() => localStorage.clear());

  it("defaults to disabled when nothing is stored", () => {
    expect(loadSuggestionsSettings()).toEqual(DEFAULT_SUGGESTIONS_SETTINGS);
  });

  it("round-trips a saved setting", () => {
    saveSuggestionsSettings({ enabled: true });
    expect(loadSuggestionsSettings()).toEqual({ enabled: true });
  });

  it("falls back to default on malformed storage", () => {
    localStorage.setItem("cairn.graph.suggestions", "not json");
    expect(loadSuggestionsSettings()).toEqual(DEFAULT_SUGGESTIONS_SETTINGS);
  });
});
