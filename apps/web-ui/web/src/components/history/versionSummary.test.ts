import { describe, it, expect } from "vitest";
import { versionWordDelta } from "./versionSummary";

const base = {
  id: "c1",
  author: "a",
  timestamp_secs: 1,
  summary: null,
  name: null,
};

describe("versionWordDelta", () => {
  it("prefers the engine's structured summary", () => {
    expect(
      versionWordDelta({
        ...base,
        message: "x",
        summary: { files_changed: 1, words_added: 124, words_removed: 3 },
      }),
    ).toEqual({ added: 124, removed: 3 });
  });

  it("falls back to parsing the deterministic message", () => {
    expect(
      versionWordDelta({
        ...base,
        message: 'Edit "Q3 Roadmap" § Goals (+42/−3 words)',
      }),
    ).toEqual({ added: 42, removed: 3 });
    expect(
      versionWordDelta({ ...base, message: 'Edit "x" (+1/-2 words)' }),
    ).toEqual({ added: 1, removed: 2 });
  });

  it("parses the single-sided forms the engine emits when a side is zero", () => {
    // counts() elides a zero side: `(+N words)` / `(−M words)`.
    expect(
      versionWordDelta({ ...base, message: 'Edit "A" (+112 words)' }),
    ).toEqual({ added: 112, removed: 0 });
    expect(
      versionWordDelta({ ...base, message: 'Edit "A" (−8 words)' }),
    ).toEqual({ added: 0, removed: 8 });
    expect(
      versionWordDelta({ ...base, message: 'Add "A" (+540 words)' }),
    ).toEqual({ added: 540, removed: 0 });
  });

  it("returns null when nothing is derivable", () => {
    expect(
      versionWordDelta({ ...base, message: "cairn: update note.md" }),
    ).toBeNull();
    // Delete/Checkpoint carry no counts suffix at all.
    expect(versionWordDelta({ ...base, message: 'Delete "A"' })).toBeNull();
  });
});
