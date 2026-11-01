import { describe, it, expect } from "vitest";
import { applyAnswerEvent, emptyAssistantTurn } from "./askReducer";
import type { AnswerEvent } from "../contract";

const reduce = (events: AnswerEvent[]) =>
  events.reduce(applyAnswerEvent, emptyAssistantTurn());

describe("applyAnswerEvent", () => {
  it("accumulates text deltas in order", () => {
    const t = reduce([
      { type: "text_delta", text: "Hello " },
      { type: "text_delta", text: "world" },
    ]);
    expect(t.text).toBe("Hello world");
  });

  it("sets citations from a sources frame", () => {
    const t = applyAnswerEvent(emptyAssistantTurn(), {
      type: "sources",
      paths: ["a.md", "b.md"],
    });
    expect(t.citations).toEqual(["a.md", "b.md"]);
  });

  it("appends text only on text_delta (no citation scraping)", () => {
    let t = emptyAssistantTurn();
    t = applyAnswerEvent(t, { type: "text_delta", text: "see [[a]] " });
    t = applyAnswerEvent(t, { type: "text_delta", text: "and [[b]]" });
    expect(t.text).toBe("see [[a]] and [[b]]");
    expect(t.citations).toEqual([]);
  });

  it("tracks tool start then completion", () => {
    const t = reduce([
      { type: "tool_started", tool: "search_notes" },
      { type: "tool_completed", tool: "search_notes", ok: true },
    ]);
    expect(t.tools).toEqual([{ tool: "search_notes", ok: true }]);
  });

  it("ignores unknown event kinds (non-exhaustive safety)", () => {
    const before = emptyAssistantTurn();
    const after = applyAnswerEvent(before, {
      type: "mystery",
    } as unknown as AnswerEvent);
    expect(after).toEqual(before);
  });

  it("does not mutate the input turn", () => {
    const before = emptyAssistantTurn();
    applyAnswerEvent(before, { type: "text_delta", text: "x" });
    expect(before.text).toBe("");
  });
});
