import { describe, it, expect, vi } from "vitest";
import { createCairnStore } from "./store";
import { MockClient } from "../client/mock";
import type { AskRequest } from "../contract";

const make = () =>
  createCairnStore(new MockClient({ "store.md": "# Store\n" }));

describe("ask slice", () => {
  it("opens the bar", () => {
    const s = make();
    expect(s.getState().ask.mode).toBe("closed");
    s.getState().askOpen();
    expect(s.getState().ask.mode).toBe("bar");
  });

  it("submit pushes a user turn + assistant turn and streams to completion", async () => {
    const s = make();
    s.getState().askSubmit("how does it work?");
    expect(s.getState().ask.streaming).toBe(true);
    expect(s.getState().ask.turns.map((t) => t.role)).toEqual([
      "user",
      "assistant",
    ]);
    await vi.waitFor(() => expect(s.getState().ask.streaming).toBe(false));
    const ai = s.getState().ask.turns[1];
    expect(ai.text).toContain("grounded");
    expect(ai.citations).toEqual(["store.md"]);
  });

  it("forwards an AskRequest to client.ask", () => {
    const client = new MockClient({ "store.md": "# Store\n" });
    let captured: AskRequest | undefined;
    const ask = client.ask.bind(client);
    client.ask = (req, onEvent) => {
      captured = req;
      return ask(req, onEvent);
    };
    const s = createCairnStore(client);
    s.getState().askSubmit("how does it work?");
    expect(captured).toEqual({ query: "how does it work?", top_k: null });
  });

  it("promote flips bar -> panel without touching turns", async () => {
    const s = make();
    s.getState().askSubmit("hi");
    const before = s.getState().ask.turns;
    s.getState().askPromote();
    expect(s.getState().ask.mode).toBe("panel");
    expect(s.getState().ask.turns).toBe(before);
    await vi.waitFor(() => expect(s.getState().ask.streaming).toBe(false));
  });

  it("captures the failed path as an error", async () => {
    const s = make();
    s.getState().askSubmit("please fail");
    await vi.waitFor(() => expect(s.getState().ask.error).not.toBeNull());
    expect(s.getState().ask.streaming).toBe(false);
  });

  it("close cancels an in-flight run (stale events do not apply)", async () => {
    const s = make();
    s.getState().askSubmit("how does it work?");
    s.getState().askClose();
    expect(s.getState().ask.mode).toBe("closed");
    const turnsAtClose = s.getState().ask.turns;
    await new Promise((r) => setTimeout(r, 20));
    expect(s.getState().ask.turns).toBe(turnsAtClose);
    expect(s.getState().ask.streaming).toBe(false);
  });
});
