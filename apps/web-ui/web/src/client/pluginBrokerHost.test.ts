import { describe, expect, it, vi } from "vitest";
import { createStoreBrokerHost } from "./pluginBrokerHost";
import type { CairnClient } from "./types";

function fakeStore(state: Record<string, unknown>) {
  return {
    getState: () => state,
    subscribe: vi.fn(() => () => {}),
  } as never;
}

type State = Record<string, unknown>;

/** A fake store that actually drives subscribers, for change-detection tests.
 *  `set(patch)` merges state and notifies listeners, like Zustand. */
function reactiveStore(initial: State) {
  let state = initial;
  const listeners = new Set<(s: State) => void>();
  const set = (patch: State) => {
    state = { ...state, ...patch };
    listeners.forEach((l) => l(state));
  };
  const api = {
    getState: () => state,
    setState: set,
    subscribe: (l: (s: State) => void) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
  } as never;
  return { api, set };
}

const stubClient = (over: Partial<CairnClient> = {}): CairnClient =>
  ({
    runQuery: vi.fn(),
    runCommand: vi.fn(),
    subscribe: vi.fn(() => () => {}),
    ...over,
  }) as unknown as CairnClient;

describe("store broker host", () => {
  it("reads the active note from open buffers", () => {
    const store = fakeStore({
      activePath: "a.md",
      openNotes: { "a.md": { contents: "hello" } },
    });
    const host = createStoreBrokerHost(store, stubClient());
    expect(host.activeNote()).toEqual({
      path: "a.md",
      title: "a",
      text: "hello",
    });
  });

  it("returns null active note when none open", () => {
    const host = createStoreBrokerHost(
      fakeStore({ activePath: null, openNotes: {} }),
      stubClient(),
    );
    expect(host.activeNote()).toBeNull();
  });

  it("writeActiveNote routes through editBuffer", () => {
    const editBuffer = vi.fn();
    const host = createStoreBrokerHost(fakeStore({ editBuffer }), stubClient());
    host.writeActiveNote("new text");
    expect(editBuffer).toHaveBeenCalledWith("new text");
  });

  it("readNote queries get_note", async () => {
    const runQuery = vi.fn().mockResolvedValue({ type: "note", contents: "X" });
    const host = createStoreBrokerHost(fakeStore({}), stubClient({ runQuery }));
    expect(await host.readNote("b.md")).toEqual({ path: "b.md", text: "X" });
    expect(runQuery).toHaveBeenCalledWith({ type: "get_note", path: "b.md" });
  });

  it("search queries search and returns paths", async () => {
    const runQuery = vi.fn().mockResolvedValue({
      type: "search_results",
      results: [{ path: "a.md" }, { path: "b.md" }],
    });
    const host = createStoreBrokerHost(fakeStore({}), stubClient({ runQuery }));
    expect(await host.search("q")).toEqual([
      { path: "a.md" },
      { path: "b.md" },
    ]);
  });

  it("info reports appVersion and theme but NOT the active path (silent method)", () => {
    const host = createStoreBrokerHost(
      fakeStore({ activePath: "a.md" }),
      stubClient(),
    );
    const info = host.info();
    expect(info).not.toHaveProperty("activePath"); // silent method must not leak the path
    expect(typeof info.appVersion).toBe("string");
    expect(typeof info.theme).toBe("string");
  });

  it("subscribeActiveNote fires on path/content change, not on unrelated change", () => {
    const { api, set } = reactiveStore({
      activePath: "a.md",
      openNotes: { "a.md": { contents: "one" } },
      query: "",
    });
    const cb = vi.fn();
    const unsub = createStoreBrokerHost(api, stubClient()).subscribeActiveNote(
      cb,
    );

    set({ query: "unrelated" }); // no active-note change → no fire
    expect(cb).toHaveBeenCalledTimes(0);

    set({ openNotes: { "a.md": { contents: "two" } } }); // contents change → fire
    expect(cb).toHaveBeenCalledTimes(1);

    set({ activePath: "b.md" }); // path change → fire
    expect(cb).toHaveBeenCalledTimes(2);

    unsub();
    set({ activePath: "c.md" }); // after unsubscribe → no fire
    expect(cb).toHaveBeenCalledTimes(2);
  });
});
