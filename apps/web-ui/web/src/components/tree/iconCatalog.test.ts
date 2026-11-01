import { describe, it, expect } from "vitest";
import { ICON_CATALOG, iconByName, searchIcons } from "./iconCatalog";
import { FileText } from "lucide-react";

describe("iconCatalog", () => {
  it("has a non-empty catalog of named icons", () => {
    expect(ICON_CATALOG.length).toBeGreaterThan(10);
    expect(ICON_CATALOG.every((i) => i.name && i.Component)).toBe(true);
  });

  it("looks up an icon component by name", () => {
    expect(iconByName("star")).toBe(
      ICON_CATALOG.find((i) => i.name === "star")!.Component,
    );
  });

  it("falls back to FileText for an unknown name", () => {
    expect(iconByName("does-not-exist")).toBe(FileText);
  });

  it("searches by name and keyword (case-insensitive)", () => {
    const byName = searchIcons("star").map((i) => i.name);
    expect(byName).toContain("star");
    const byKeyword = searchIcons("favorite").map((i) => i.name);
    expect(byKeyword).toContain("star");
  });

  it("returns the full catalog for an empty query", () => {
    expect(searchIcons("").length).toBe(ICON_CATALOG.length);
  });
});
