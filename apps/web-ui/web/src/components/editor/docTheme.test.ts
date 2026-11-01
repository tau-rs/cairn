import { describe, it, expect } from "vitest";
import { docTheme, docHighlightStyle, markdownCodeLanguages } from "./docTheme";

describe("docTheme", () => {
  it("exports a CodeMirror extension for the theme", () => {
    expect(docTheme).toBeTruthy();
  });
  it("exports a highlight style extension", () => {
    expect(docHighlightStyle).toBeTruthy();
  });
  it("exports a non-empty list of code languages for fenced highlighting", () => {
    expect(Array.isArray(markdownCodeLanguages)).toBe(true);
    expect(markdownCodeLanguages.length).toBeGreaterThan(0);
  });
});
