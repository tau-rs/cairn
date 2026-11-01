import { beforeEach, describe, expect, it } from "vitest";
import {
  GRANTS_KEY,
  loadGrants,
  needsConsent,
  persistGrants,
  saveGrant,
  removeGrant,
} from "./pluginGrantsSlice";

beforeEach(() => localStorage.clear());

describe("plugin grants", () => {
  it("needsConsent when no grant exists", () => {
    expect(needsConsent({}, "p", "1.0.0", ["activeNote.write"])).toBe(true);
  });

  it("no consent needed when grant covers requested set and version matches", () => {
    const grants = {
      p: {
        version: "1.0.0",
        granted: ["activeNote.write", "notes.read"] as const,
      },
    };
    expect(needsConsent(grants, "p", "1.0.0", ["activeNote.write"])).toBe(
      false,
    );
  });

  it("requires consent again when version changed", () => {
    const grants = {
      p: { version: "1.0.0", granted: ["activeNote.write"] as const },
    };
    expect(needsConsent(grants, "p", "2.0.0", ["activeNote.write"])).toBe(true);
  });

  it("requires consent when the requested set expands beyond granted", () => {
    const grants = {
      p: { version: "1.0.0", granted: ["activeNote.read"] as const },
    };
    expect(
      needsConsent(grants, "p", "1.0.0", [
        "activeNote.read",
        "activeNote.write",
      ]),
    ).toBe(true);
  });

  it("persistGrants writes a saved grant that loadGrants reads back", () => {
    const next = saveGrant({}, "p", "1.0.0", ["activeNote.write"]);
    persistGrants(next);
    expect(localStorage.getItem(GRANTS_KEY)).toContain("activeNote.write");
    expect(loadGrants()).toEqual(next);
  });

  it("removeGrant drops a plugin; persisting the result clears storage", () => {
    const seeded = saveGrant({}, "p", "1.0.0", ["activeNote.write"]);
    persistGrants(seeded);
    const after = removeGrant(seeded, "p");
    persistGrants(after);
    expect(after.p).toBeUndefined();
    expect(loadGrants()).toEqual({});
  });

  it("loadGrants tolerates corrupt storage", () => {
    localStorage.setItem(GRANTS_KEY, "{not json");
    expect(loadGrants()).toEqual({});
  });
});
