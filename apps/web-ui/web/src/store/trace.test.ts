import { describe, it, expect, vi } from "vitest";
import { noopTrace, makeConsoleTrace } from "./trace";

describe("noopTrace", () => {
  it("event is a no-op and time just runs the thunk", async () => {
    expect(
      noopTrace.event("note_changed", ["refreshNotePaths"]),
    ).toBeUndefined();
    const out = await noopTrace.time("loadTags", async () => 42);
    expect(out).toBe(42);
  });
});

describe("makeConsoleTrace", () => {
  it("logs the event type and dispatched actions", () => {
    const spy = vi.spyOn(console, "debug").mockImplementation(() => {});
    makeConsoleTrace().event("note_changed", ["refreshNotePaths", "loadTags"]);
    expect(spy).toHaveBeenCalledWith("[cairn] refresh ← note_changed", [
      "refreshNotePaths",
      "loadTags",
    ]);
    spy.mockRestore();
  });

  it("time returns the thunk's value and logs a timing line", async () => {
    const spy = vi.spyOn(console, "debug").mockImplementation(() => {});
    const out = await makeConsoleTrace().time("loadGraph", async () => "done");
    expect(out).toBe("done");
    expect(spy).toHaveBeenCalledWith(
      expect.stringContaining("[cairn] loadGraph took"),
    );
    spy.mockRestore();
  });
});
