import { describe, it, expect } from "vitest";
import { createCairnStore } from "./store";
import { MockClient } from "../client/mock";

const make = () =>
  createCairnStore(new MockClient({ "draft.md": "# Draft\n" }));

describe("recoverySlice", () => {
  it("opens and loads blocks", async () => {
    const s = make();
    s.getState().openRecovery("draft.md");
    await new Promise((r) => setTimeout(r, 0));
    expect(s.getState().recovery.open).toBe(true);
    expect(s.getState().recovery.status).toBe("ready");
    expect(s.getState().recovery.blocks.length).toBeGreaterThan(0);
  });
  it("restoreVersion clears restoring and closes stay open", async () => {
    const s = make();
    s.getState().openRecovery("draft.md");
    await new Promise((r) => setTimeout(r, 0));
    const id = s.getState().recovery.blocks[0].id;
    s.getState().restoreVersion(id, 0);
    await new Promise((r) => setTimeout(r, 0));
    expect(s.getState().recovery.restoring).toBeNull();
  });
  it("closeRecovery resets", async () => {
    const s = make();
    s.getState().openRecovery("draft.md");
    await new Promise((r) => setTimeout(r, 0));
    s.getState().closeRecovery();
    expect(s.getState().recovery.open).toBe(false);
    expect(s.getState().recovery.note).toBeNull();
  });
  it("a restore in flight when closeRecovery fires does not clobber the reset state", async () => {
    const s = make();
    s.getState().openRecovery("draft.md");
    await new Promise((r) => setTimeout(r, 0));
    const id = s.getState().recovery.blocks[0].id;
    s.getState().restoreVersion(id, 0);
    s.getState().closeRecovery();
    await new Promise((r) => setTimeout(r, 0));
    expect(s.getState().recovery.open).toBe(false);
    expect(s.getState().recovery.restoring).toBeNull();
    expect(s.getState().recovery.error).toBeNull();
  });
});
