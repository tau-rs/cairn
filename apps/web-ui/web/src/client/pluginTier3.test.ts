import { describe, expect, it } from "vitest";
import {
  CAPABILITY_OF,
  PLUGIN_CAPABILITY_VALUES,
  groupCapabilities,
  isCapability,
} from "./pluginTier3";

describe("pluginTier3 capability model", () => {
  it("maps every method to a capability or null (silent)", () => {
    expect(CAPABILITY_OF["activeNote.write"]).toBe("activeNote.write");
    expect(CAPABILITY_OF["notes.search"]).toBe("notes.search");
    expect(CAPABILITY_OF["host.info"]).toBeNull();
    expect(CAPABILITY_OF["ui.notice"]).toBeNull();
  });

  it("recognizes only known capability strings", () => {
    expect(isCapability("activeNote.write")).toBe(true);
    expect(isCapability("filesystem.format")).toBe(false);
    expect(isCapability(42)).toBe(false);
    expect(isCapability(null)).toBe(false);
    expect(isCapability(undefined)).toBe(false);
    // every PLUGIN_CAPABILITY_VALUES entry passes isCapability
    expect(PLUGIN_CAPABILITY_VALUES.every(isCapability)).toBe(true);
  });

  it("groups capabilities into plain-language risk rows, dropping silent ones", () => {
    const rows = groupCapabilities([
      "activeNote.write",
      "notes.read",
      "notes.search",
      "activeNote.read",
      "command.invoke",
    ]);
    const labels = rows.map((r) => r.label);
    expect(labels).toContain("Modify the current note");
    expect(labels).toContain("Read across your whole vault"); // notes.read+search collapse to one row
    expect(labels).toContain("Read the current note");
    expect(labels).not.toContain("command.invoke"); // silent → no row
    // notes.read + notes.search collapse into a single row (deduped)
    expect(
      rows.filter((r) => r.label === "Read across your whole vault"),
    ).toHaveLength(1);
  });

  it("orders rows HIGH severity first", () => {
    const rows = groupCapabilities(["activeNote.read", "activeNote.write"]);
    expect(rows[0].severity).toBe("high");
  });
});
