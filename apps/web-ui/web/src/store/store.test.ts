import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { createCairnStore, DEFAULT_SETTINGS, ERROR_TOAST_MS } from "./store";
import {
  MockClient,
  type HistoryFixture,
  type VaultSnapshot,
} from "../client/mock";
import {
  loadOverrides,
  saveOverrides,
} from "../components/shortcuts/keybindingPersistence";
import type {
  Query,
  QueryResponse,
  SuggestedEdge,
  Revision,
} from "../contract";
import { saveTabs } from "../components/tabs/tabsPersistence";
import type { Event } from "../contract";
import * as timer from "../util/timer";

// Stub `subscribe` so it immediately reports an attach failure and delivers no
// events — simulating a channel that never came up (a dead push stream).
const failingSubscribe = (
  _cb: (e: Event) => void,
  onError?: (err: unknown) => void,
) => {
  onError?.(new Error("attach failed"));
  return () => {};
};

beforeEach(() => vi.useFakeTimers());
beforeEach(() => localStorage.clear());
afterEach(() => vi.useRealTimers());

function setup(
  opts: {
    history?: Record<string, HistoryFixture>;
    vaultSnapshots?: Record<string, VaultSnapshot>;
    vaultRevisions?: Revision[];
    structuralRevisions?: Revision[];
  } = {},
) {
  const client = new MockClient(
    { "a.md": "links to [[b]]", "b.md": "target note" },
    opts.history ?? {},
    opts.vaultSnapshots ?? {},
    opts.vaultRevisions ?? [],
    opts.structuralRevisions ?? [],
  );
  const store = createCairnStore(client);
  return { client, store };
}

describe("cairn store", () => {
  it("init loads the note list", async () => {
    const { store } = setup();
    await store.getState().init();
    expect(store.getState().notePaths).toEqual(["a.md", "b.md"]);
  });

  it("openNote loads contents and backlinks", async () => {
    const { store } = setup();
    await store.getState().init();
    await store.getState().openNote("b.md");
    expect(store.getState().activePath).toBe("b.md");
    expect(store.getState().activeContents).toBe("target note");
    expect(store.getState().backlinks).toEqual(["a.md"]);
  });

  it("editBuffer schedules a debounced autosave that writes the note", async () => {
    const { client, store } = setup();
    await store.getState().init();
    await store.getState().openNote("a.md");
    store.getState().editBuffer("edited body [[b]]");
    expect(store.getState().dirty).toBe(true);
    await vi.advanceTimersByTimeAsync(DEFAULT_SETTINGS.autosaveMs);
    const res = await client.runQuery({ type: "get_note", path: "a.md" });
    expect(res).toEqual({ type: "note", contents: "edited body [[b]]" });
    expect(store.getState().dirty).toBe(false);
  });

  it("runSearch populates results; closeSearch clears them", async () => {
    const { store } = setup();
    await store.getState().init();
    await store.getState().runSearch("target");
    expect(store.getState().searchResults).toEqual(["b.md"]);
    store.getState().closeSearch();
    expect(store.getState().searchResults).toBeNull();
  });

  it("sealNow seals a version, records the id and refreshes lastVersion", async () => {
    const { client, store } = setup();
    await store.getState().init();
    await client.sendCommand({
      type: "write_note",
      path: "a.md",
      contents: "hello world",
    });
    store.setState({ uncommitted: true });
    await store.getState().sealNow();
    expect(store.getState().lastCommit).toBe("c0001");
    expect(store.getState().uncommitted).toBe(false);
    expect(store.getState().lastVersion?.id).toBe("c0001");
  });

  it("sealNow flushes a pending autosave so the last keystrokes make the version", async () => {
    // The hint fires on note switch / blur, both of which beat the ~1s
    // debounce; an unflushed tail would land in the engine's next idle seal.
    const { client, store } = setup();
    await store.getState().init();
    await store.getState().openNote("a.md");
    store.getState().editBuffer("tail keystrokes [[b]]");
    expect(store.getState().dirty).toBe(true);
    await store.getState().sealNow(); // no timer advance: debounce still pending
    expect(store.getState().dirty).toBe(false);
    const note = await client.runQuery({ type: "get_note", path: "a.md" });
    expect(note).toEqual({ type: "note", contents: "tail keystrokes [[b]]" });
    expect(store.getState().lastCommit).toBe("c0001");
    // The version must contain the tail, not the pre-edit text.
    const hist = await client.runQuery({ type: "vault_history", limit: 1 });
    if (hist.type !== "history") throw new Error("bad response");
    expect(hist.revisions[0].summary?.files_changed).toBe(1);
  });

  it("sealNow is a no-op when nothing is uncommitted", async () => {
    const { client, store } = setup();
    await store.getState().init();
    const spy = vi.spyOn(client, "sendCommand");
    await store.getState().sealNow();
    expect(spy).not.toHaveBeenCalled();
  });

  it("concurrent sealNow calls seal once (the guard is claimed before any await)", async () => {
    // A window blur landing on top of a note switch fires both hints; the
    // async autosave flush must not let them interleave into two seals.
    const { client, store } = setup();
    await store.getState().init();
    await store.getState().openNote("a.md");
    store.getState().editBuffer("racing keystrokes [[b]]");
    const spy = vi.spyOn(client, "sendCommand");
    await Promise.all([store.getState().sealNow(), store.getState().sealNow()]);
    const commits = spy.mock.calls.filter(([c]) => c.type === "commit");
    expect(commits).toHaveLength(1);
    expect(store.getState().committing).toBe(false);
  });

  it("sealNow treats nothing_to_commit as benign success (no toast)", async () => {
    // The engine's idle auto-commit routinely seals before the UI's note-switch
    // hint lands, so this is the NORMAL path — it must stay silent.
    const { client, store } = setup();
    await store.getState().init();
    store.setState({ uncommitted: true });
    const spy = vi.spyOn(client, "sendCommand");
    await store.getState().sealNow();
    expect(spy).toHaveBeenCalledWith({ type: "commit", message: null });
    await expect(spy.mock.results[0].value).resolves.toEqual({
      type: "nothing_to_commit",
    });
    expect(store.getState().errors).toHaveLength(0);
    expect(store.getState().uncommitted).toBe(false);
  });

  it("sealNow failures are silent (no toast)", async () => {
    const { client, store } = setup();
    await store.getState().init();
    vi.spyOn(client, "sendCommand").mockRejectedValueOnce(new Error("boom"));
    vi.spyOn(console, "warn").mockImplementation(() => {});
    store.setState({ uncommitted: true });
    await store.getState().sealNow();
    expect(store.getState().errors).toHaveLength(0);
    expect(store.getState().uncommitted).toBe(true); // still sealable later
  });

  it("nameVersion sends the C0 command and surfaces errors as toasts", async () => {
    const { client, store } = setup();
    await store.getState().init();
    const spy = vi.spyOn(client, "sendCommand");
    await store.getState().nameVersion("c0001", "Draft 1");
    expect(spy).toHaveBeenCalledWith({
      type: "name_version",
      commit: "c0001",
      name: "Draft 1",
    });
    vi.spyOn(client, "sendCommand").mockRejectedValueOnce(new Error("boom"));
    await store.getState().nameVersion("c0001", "x");
    expect(store.getState().errors[0].message).toContain("boom");
  });

  it("reacts to a note_changed event by refreshing the note list", async () => {
    // Real timers here: vi.waitFor polls on real timers and the mock emits via
    // queueMicrotask, so mixing fake timers would hang.
    vi.useRealTimers();
    const { client, store } = setup();
    await store.getState().init();
    await client.sendCommand({
      type: "write_note",
      path: "c.md",
      contents: "hi",
    });
    await vi.waitFor(() =>
      expect(store.getState().notePaths).toContain("c.md"),
    );
  });

  it("surfaces errors from a failing command", async () => {
    const { client, store } = setup();
    vi.spyOn(client, "sendCommand").mockRejectedValueOnce(new Error("boom"));
    await store.getState().init();
    await store.getState().nameVersion("c1", "x");
    expect(store.getState().errors[0].message).toContain("boom");
  });

  it("surfaces a degraded state when the event channel fails to attach", async () => {
    const { client, store } = setup();
    vi.spyOn(client, "subscribe").mockImplementation(failingSubscribe);
    await store.getState().init();
    expect(store.getState().liveUpdates).toBe("down");
  });

  it("maps onStatus reconnecting/live onto liveUpdates", async () => {
    const { client, store } = setup();
    let statusCb: ((s: "reconnecting" | "live") => void) | undefined;
    vi.spyOn(client, "subscribe").mockImplementation(
      (
        _cb: (e: Event) => void,
        _onError?: (err: unknown) => void,
        onStatus?: (s: "reconnecting" | "live") => void,
      ) => {
        statusCb = onStatus;
        return () => {};
      },
    );
    await store.getState().init();
    expect(store.getState().liveUpdates).toBe("ok");
    statusCb!("reconnecting");
    expect(store.getState().liveUpdates).toBe("reconnecting");
    statusCb!("live");
    expect(store.getState().liveUpdates).toBe("ok");
  });

  it("escalates reconnecting to down, then recovers on live", async () => {
    const { client, store } = setup();
    let errorCb: ((err: unknown) => void) | undefined;
    let statusCb: ((s: "reconnecting" | "live") => void) | undefined;
    vi.spyOn(client, "subscribe").mockImplementation(
      (
        _cb: (e: Event) => void,
        onError?: (err: unknown) => void,
        onStatus?: (s: "reconnecting" | "live") => void,
      ) => {
        errorCb = onError;
        statusCb = onStatus;
        return () => {};
      },
    );
    await store.getState().init();
    statusCb!("reconnecting");
    expect(store.getState().liveUpdates).toBe("reconnecting");
    errorCb!(new Error("sustained failure"));
    expect(store.getState().liveUpdates).toBe("down");
    statusCb!("live");
    expect(store.getState().liveUpdates).toBe("ok");
  });

  it("refreshAll re-pulls note paths and clears the degraded state", async () => {
    vi.useRealTimers();
    const { client, store } = setup();
    // Stub the channel so pushed events never reach the store (a dead stream):
    // the note list goes stale until a manual refresh.
    vi.spyOn(client, "subscribe").mockImplementation(failingSubscribe);
    await store.getState().init();
    await client.sendCommand({
      type: "write_note",
      path: "c.md",
      contents: "hi",
    });
    expect(store.getState().notePaths).not.toContain("c.md"); // stale: no live event
    await store.getState().refreshAll();
    expect(store.getState().notePaths).toContain("c.md");
    expect(store.getState().liveUpdates).toBe("ok");
  });

  it("refreshAll reconciles the active tag filter (matching the live path)", async () => {
    vi.useRealTimers();
    const client = new MockClient({
      "a.md": "---\ntags: [keep]\n---\nfirst",
    });
    const store = createCairnStore(client);
    vi.spyOn(client, "subscribe").mockImplementation(failingSubscribe);
    await store.getState().init();
    await store.getState().filterByTag("keep");
    expect(store.getState().searchResults).toEqual(["a.md"]);
    // A second tagged note arrives, but the dead stream delivers no event.
    await client.sendCommand({
      type: "write_note",
      path: "b.md",
      contents: "---\ntags: [keep]\n---\nsecond",
    });
    expect(store.getState().searchResults).toEqual(["a.md"]); // stale filter view
    await store.getState().refreshAll();
    expect(store.getState().searchResults).toEqual(["a.md", "b.md"]);
  });

  it("surfaces an error if loading the note list fails", async () => {
    const { client, store } = setup();
    vi.spyOn(client, "runQuery").mockRejectedValueOnce({
      type: "internal",
      message: "boom",
    });
    await store.getState().init();
    expect(store.getState().errors[0].message).toContain("boom");
  });

  it("queues multiple errors instead of clobbering", async () => {
    const { client, store } = setup();
    vi.spyOn(client, "sendCommand").mockRejectedValue(new Error("boom"));
    await store.getState().init();
    await store.getState().nameVersion("c1", "one");
    await store.getState().nameVersion("c2", "two");
    expect(store.getState().errors).toHaveLength(2);
  });

  it("auto-dismisses a queued error after the timeout", async () => {
    const { client, store } = setup();
    vi.spyOn(client, "sendCommand").mockRejectedValue(new Error("boom"));
    await store.getState().init();
    await store.getState().nameVersion("c1", "x");
    expect(store.getState().errors).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(ERROR_TOAST_MS);
    expect(store.getState().errors).toHaveLength(0);
  });

  it("logs caught errors to console.error with operation context", async () => {
    const { client, store } = setup();
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    vi.spyOn(client, "runQuery").mockRejectedValueOnce({
      type: "internal",
      message: "boom",
    });
    await store.getState().init();
    expect(spy).toHaveBeenCalledWith(
      expect.stringContaining("List notes"),
      expect.objectContaining({
        operation: "List notes",
        error: expect.objectContaining({ type: "internal" }),
      }),
    );
    expect(store.getState().errors[0].message).toContain("List notes");
    spy.mockRestore();
  });

  it("keeps the buffer dirty if edited while a slow save is in flight", async () => {
    vi.useRealTimers();
    const client = new MockClient({ "a.md": "v0" });
    let release = () => {};
    const gate = new Promise<void>((r) => (release = r));
    const orig = client.sendCommand.bind(client);
    vi.spyOn(client, "sendCommand").mockImplementation(async (c) => {
      await gate;
      return orig(c);
    });
    const store = createCairnStore(client);
    await store.getState().init();
    await store.getState().openNote("a.md");
    store.getState().editBuffer("v1");
    const saving = store.getState().saveActive();
    store.getState().editBuffer("v2"); // user types while the save is gated
    release();
    await saving;
    expect(store.getState().dirty).toBe(true); // v2 is not yet persisted
  });

  it("init is idempotent — a single event triggers one note-list refresh", async () => {
    vi.useRealTimers();
    const client = new MockClient({});
    const store = createCairnStore(client);
    await store.getState().init();
    await store.getState().init(); // second call must be a no-op (no double subscription)
    const spy = vi.spyOn(client, "runQuery");
    await client.sendCommand({
      type: "write_note",
      path: "a.md",
      contents: "x",
    });
    await vi.waitFor(() =>
      expect(store.getState().notePaths).toContain("a.md"),
    );
    const listCalls = spy.mock.calls.filter(
      ([q]) => q.type === "list_notes",
    ).length;
    expect(listCalls).toBe(1);
  });

  it("loadGraph populates the graph from get_graph", async () => {
    const { store } = setup();
    await store.getState().init();
    await store.getState().loadGraph();
    const g = store.getState().graph;
    expect(g).not.toBeNull();
    expect(g!.nodes.map((n) => n.path).sort()).toEqual(["a.md", "b.md"]);
    expect(g!.edges).toEqual([{ from: "a.md", to: "b.md" }]);
  });

  it("refreshes the graph on a note event when it is loaded", async () => {
    vi.useRealTimers();
    const { client, store } = setup();
    await store.getState().init();
    await store.getState().loadGraph();
    await client.sendCommand({
      type: "write_note",
      path: "c.md",
      contents: "x",
    });
    await vi.waitFor(() =>
      expect(store.getState().graph!.nodes.map((n) => n.path)).toContain(
        "c.md",
      ),
    );
  });

  it("defaults the editor to live preview", () => {
    expect(DEFAULT_SETTINGS.editorMode).toBe("livepreview");
  });

  describe("loadSuggestions", () => {
    const EDGE: SuggestedEdge = {
      from: "a.md",
      to: "b.md",
      weight: 0.7,
      why: "shared: x",
    };

    it("populates suggestions on a suggestions response", async () => {
      const { client, store } = setup();
      vi.spyOn(client, "runQuery").mockResolvedValueOnce({
        type: "suggestions",
        suggestions: [EDGE],
      });
      await store.getState().loadSuggestions({ type: "vault" });
      expect(store.getState().suggestions).toEqual([EDGE]);
    });

    it("passes the given scope through to runQuery", async () => {
      const { client, store } = setup();
      const spy = vi
        .spyOn(client, "runQuery")
        .mockResolvedValue({ type: "suggestions", suggestions: [] });
      await store.getState().loadSuggestions({ type: "note", path: "a.md" });
      expect(spy).toHaveBeenCalledWith({
        type: "get_suggestions",
        scope: { type: "note", path: "a.md" },
      } satisfies Query);
    });

    it("leaves state untouched and reports an error on an unexpected variant", async () => {
      const { client, store } = setup();
      vi.spyOn(client, "runQuery").mockResolvedValueOnce({
        type: "paths",
        paths: [],
      });
      await store.getState().loadSuggestions({ type: "vault" });
      expect(store.getState().suggestions).toBeNull();
      expect(store.getState().errors.length).toBeGreaterThan(0);
    });
  });

  it("defaults loadRemoteImages to off (no auto-fetch of remote images)", () => {
    expect(DEFAULT_SETTINGS.loadRemoteImages).toBe(false);
  });

  it("defaults to an always-open cairn (mock) and sets cairnPath on init", async () => {
    const { store } = setup();
    await store.getState().init();
    expect(store.getState().cairnPath).toBe("(fixture)");
    expect(store.getState().notePaths.length).toBeGreaterThan(0);
  });

  it("openCairn sets cairnPath and loads notes via the host", async () => {
    vi.useRealTimers();
    const client = new MockClient({ "x.md": "hi" });
    const host = {
      currentCairn: () => Promise.resolve<string | null>(null),
      openCairn: () => Promise.resolve<string | null>("/tmp/mycairn"),
      assetUrl: (p: string) => p,
      setPluginUiRoots: () => Promise.resolve(),
    };
    const store = createCairnStore(client, host);
    await store.getState().init();
    expect(store.getState().cairnPath).toBeNull();
    await store.getState().openCairn();
    expect(store.getState().cairnPath).toBe("/tmp/mycairn");
    expect(store.getState().notePaths).toContain("x.md");
  });

  it("openCairn reloads tags + plugins, restores tabs, and resets the graph", async () => {
    vi.useRealTimers();
    localStorage.clear();
    const client = new MockClient({
      "a.md": "---\ntags: [rust]\n---\nlinks [[b]]",
      "b.md": "B",
    });
    const host = {
      currentCairn: () => Promise.resolve<string | null>(null),
      openCairn: () => Promise.resolve<string | null>("/tmp/second"),
      assetUrl: (p: string) => p,
      setPluginUiRoots: () => Promise.resolve(),
    };
    // A pinned tab persisted from a prior session of this cairn.
    saveTabs({ tabs: [{ path: "b.md", preview: false }], activePath: "b.md" });
    const store = createCairnStore(client, host);
    await store.getState().init(); // currentCairn null -> nothing loaded yet
    await store.getState().loadGraph(); // graph now non-null (cairn-specific)

    await store.getState().openCairn();

    // Tags + plugins must reload, not stay empty until an unrelated event.
    expect(store.getState().tags).toEqual([{ tag: "rust", count: 1 }]);
    expect(store.getState().plugins.map((p) => p.id)).toEqual([
      "demo",
      "bare",
      "wordcount",
    ]);
    // Persisted pinned tabs restore for the freshly opened cairn.
    const { panes: p1, activePane: ap1 } = store.getState();
    expect(p1[ap1].tabs.map((t) => t.path)).toEqual(["b.md"]);
    expect(store.getState().activePath).toBe("b.md");
    // Derived graph is reset consistently (will reload when its panel asks).
    expect(store.getState().graph).toBeNull();
  });

  it("keeps each open note's buffer when switching tabs", async () => {
    const { store } = setup();
    await store.getState().init();
    await store.getState().openNote("a.md");
    store.getState().editBuffer("edited A [[b]]"); // pins a.md, marks dirty
    await store.getState().openNote("b.md");
    expect(store.getState().activeContents).toBe("target note");
    await store.getState().openNote("a.md"); // back to A
    expect(store.getState().activeContents).toBe("edited A [[b]]");
    expect(store.getState().dirty).toBe(true);
  });

  it("editing pins the preview tab", async () => {
    const { store } = setup();
    await store.getState().init();
    await store.getState().openNote("a.md");
    const tabs1 = () => {
      const s = store.getState();
      return s.panes[s.activePane].tabs;
    };
    expect(tabs1()).toEqual([{ path: "a.md", preview: true }]);
    store.getState().editBuffer("x [[b]]");
    expect(tabs1()).toEqual([{ path: "a.md", preview: false }]);
  });

  it("browsing notes (preview) does not write to disk", async () => {
    const { client, store } = setup();
    const spy = vi.spyOn(client, "sendCommand");
    await store.getState().init();
    await store.getState().openNote("a.md");
    await store.getState().openNote("b.md"); // replaces the preview tab
    await vi.advanceTimersByTimeAsync(DEFAULT_SETTINGS.autosaveMs);
    expect(spy.mock.calls.some(([c]) => c.type === "write_note")).toBe(false);
    const { panes: bp, activePane: bap } = store.getState();
    expect(bp[bap].tabs).toEqual([{ path: "b.md", preview: true }]);
  });

  it("closeTab focuses a neighbour; closing the last clears the editor", async () => {
    const { store } = setup();
    await store.getState().init();
    await store.getState().openNote("a.md");
    store.getState().pinTab("a.md");
    await store.getState().openNote("b.md");
    store.getState().pinTab("b.md");
    store.getState().selectTab("a.md");
    store.getState().closeTab("a.md");
    expect(store.getState().activePath).toBe("b.md"); // neighbour focused
    store.getState().closeTab("b.md");
    expect(store.getState().activePath).toBeNull();
    expect(store.getState().activeContents).toBe("");
    const { panes: ep, activePane: eap } = store.getState();
    expect(ep[eap].tabs).toEqual([]);
  });

  it("deleteNote closes the note's tab", async () => {
    vi.useRealTimers();
    const client = new MockClient({ "a.md": "A", "b.md": "B" });
    const store = createCairnStore(client);
    await store.getState().init();
    await store.getState().openNote("a.md");
    store.getState().pinTab("a.md");
    await store.getState().openNote("b.md");
    store.getState().pinTab("b.md");
    await store.getState().deleteNote("b.md");
    const { panes: dp, activePane: dap } = store.getState();
    expect(dp[dap].tabs.map((t) => t.path)).toEqual(["a.md"]);
    expect(store.getState().activePath).toBe("a.md");
  });

  it("restores persisted pinned tabs on init, skipping missing notes", async () => {
    vi.useRealTimers();
    localStorage.clear();
    // First store instance: open + pin two notes, which persists them.
    const c1 = new MockClient({ "a.md": "A", "b.md": "B" });
    const s1 = createCairnStore(c1);
    await s1.getState().init();
    await s1.getState().openNote("a.md");
    s1.getState().pinTab("a.md");
    await s1.getState().openNote("b.md");
    s1.getState().pinTab("b.md");
    s1.getState().selectTab("b.md");
    // Second instance with a fresh client missing b.md → only a.md restored.
    const c2 = new MockClient({ "a.md": "A" });
    const s2 = createCairnStore(c2);
    await s2.getState().init();
    const { panes: rp, activePane: rap } = s2.getState();
    expect(rp[rap].tabs.map((t) => t.path)).toEqual(["a.md"]);
    expect(s2.getState().activePath).toBe("a.md");
  });

  it("init flips ready true only after the restore completes", async () => {
    const { store } = setup();
    expect(store.getState().ready).toBe(false); // fresh store is not ready
    await store.getState().init();
    // ready is set last, so the persisted-tab restore has already run by now.
    expect(store.getState().ready).toBe(true);
  });

  it("flushes a dirty buffer to disk when its tab is closed", async () => {
    const { client, store } = setup();
    const spy = vi.spyOn(client, "sendCommand");
    await store.getState().init();
    await store.getState().openNote("a.md");
    store.getState().editBuffer("flushed [[b]]");
    store.getState().closeTab("a.md"); // close within the autosave window
    // Flush the in-flight write's microtasks (advance only the autosave window so
    // the recurring auto-commit interval armed by init() isn't tripped).
    await vi.advanceTimersByTimeAsync(DEFAULT_SETTINGS.autosaveMs);
    expect(
      spy.mock.calls.some(
        ([c]) => c.type === "write_note" && c.contents === "flushed [[b]]",
      ),
    ).toBe(true);
    const res = await client.runQuery({ type: "get_note", path: "a.md" });
    expect(res).toEqual({ type: "note", contents: "flushed [[b]]" });
  });

  it("does not resurrect a closed note's buffer; reopening shows saved contents", async () => {
    vi.useRealTimers();
    const client = new MockClient({ "a.md": "orig" });
    const store = createCairnStore(client);
    await store.getState().init();
    await store.getState().openNote("a.md");
    store.getState().editBuffer("changed");
    store.getState().closeTab("a.md");
    await vi.waitFor(async () => {
      const r = await client.runQuery({ type: "get_note", path: "a.md" });
      expect(r).toEqual({ type: "note", contents: "changed" });
    });
    expect(store.getState().openNotes["a.md"]).toBeUndefined(); // no phantom buffer
    await store.getState().openNote("a.md"); // reopen → fetched fresh
    expect(store.getState().activeContents).toBe("changed");
  });

  it("loadTags populates the tag list", async () => {
    vi.useRealTimers();
    const client = new MockClient({
      "a.md": "---\ntags: [rust, ideas]\n---\nx",
      "b.md": "---\ntags: [rust]\n---\ny",
    });
    const store = createCairnStore(client);
    await store.getState().init();
    expect(store.getState().tags).toEqual([
      { tag: "ideas", count: 1 },
      { tag: "rust", count: 2 },
    ]);
  });
  it("filterByTag fills the results overlay and sets activeTag", async () => {
    vi.useRealTimers();
    const client = new MockClient({
      "a.md": "---\ntags: [rust]\n---\nx",
      "b.md": "---\ntags: [ideas]\n---\ny",
    });
    const store = createCairnStore(client);
    await store.getState().init();
    await store.getState().filterByTag("rust");
    expect(store.getState().searchResults).toEqual(["a.md"]);
    expect(store.getState().activeTag).toBe("rust");
  });
  it("a search clears activeTag; closeSearch clears both", async () => {
    vi.useRealTimers();
    const client = new MockClient({ "a.md": "---\ntags: [rust]\n---\nx" });
    const store = createCairnStore(client);
    await store.getState().init();
    await store.getState().filterByTag("rust");
    await store.getState().runSearch("a");
    expect(store.getState().activeTag).toBeNull();
    store.getState().closeSearch();
    expect(store.getState().searchResults).toBeNull();
    expect(store.getState().activeTag).toBeNull();
  });
  it("runSearch stores ranked paths + snippets keyed by path", async () => {
    vi.useRealTimers();
    const client = new MockClient({ "a.md": "the quick brown fox" });
    const store = createCairnStore(client);
    await store.getState().init();
    await store.getState().runSearch("quick");
    expect(store.getState().searchResults).toEqual(["a.md"]);
    expect(store.getState().searchSnippets?.["a.md"].snippet).toContain(
      "quick",
    );
    expect(store.getState().searchSnippets?.["a.md"].highlights.length).toBe(1);
  });
  it("filterByTag leaves searchSnippets null; closeSearch clears it", async () => {
    vi.useRealTimers();
    const client = new MockClient({ "a.md": "---\ntags: [rust]\n---\nx" });
    const store = createCairnStore(client);
    await store.getState().init();
    await store.getState().filterByTag("rust");
    expect(store.getState().searchSnippets).toBeNull();
    await store.getState().runSearch("x");
    store.getState().closeSearch();
    expect(store.getState().searchSnippets).toBeNull();
    expect(store.getState().searchResults).toBeNull();
  });
  it("refreshes the tag list on a note event", async () => {
    vi.useRealTimers();
    const client = new MockClient({ "a.md": "---\ntags: [rust]\n---\nx" });
    const store = createCairnStore(client);
    await store.getState().init();
    expect(store.getState().tags).toEqual([{ tag: "rust", count: 1 }]);
    await client.sendCommand({
      type: "write_note",
      path: "b.md",
      contents: "---\ntags: [rust, ideas]\n---\ny",
    });
    await vi.waitFor(() =>
      expect(store.getState().tags).toEqual([
        { tag: "ideas", count: 1 },
        { tag: "rust", count: 2 },
      ]),
    );
  });
  it("applyRenames moves an open note's tab + activePath to the new path", async () => {
    vi.useRealTimers();
    const client = new MockClient({ "a.md": "A", "b.md": "B" });
    const store = createCairnStore(client);
    await store.getState().init();
    await store.getState().openNote("a.md");
    store.getState().pinTab("a.md");
    await store.getState().applyRenames([{ from: "a.md", to: "c.md" }]);
    expect(store.getState().activePath).toBe("c.md");
    const { panes: np, activePane: nap } = store.getState();
    expect(np[nap].tabs.map((t) => t.path)).toContain("c.md");
    expect(np[nap].tabs.map((t) => t.path)).not.toContain("a.md");
    expect(store.getState().openNotes["c.md"]).toBeDefined();
    expect(store.getState().openNotes["a.md"]).toBeUndefined();
  });
  it("applyRenames stops on the first error (no further commands)", async () => {
    vi.useRealTimers();
    const client = new MockClient({ "a.md": "A" });
    const spy = vi.spyOn(client, "sendCommand");
    const store = createCairnStore(client);
    await store.getState().init();
    await store.getState().applyRenames([
      { from: "missing.md", to: "z.md" },
      { from: "a.md", to: "y.md" },
    ]);
    expect(store.getState().errors.length).toBeGreaterThan(0);
    const renameCalls = spy.mock.calls.filter(
      ([cmd]) => cmd.type === "rename_note",
    );
    expect(renameCalls.length).toBe(1);
    expect(store.getState().notePaths).toContain("a.md");
  });
  it("loadPlugins populates the plugin list on init", async () => {
    vi.useRealTimers();
    const store = createCairnStore(new MockClient({}));
    await store.getState().init();
    expect(store.getState().plugins.map((p) => p.id)).toEqual([
      "demo",
      "bare",
      "wordcount",
    ]);
  });
  it("invokePlugin sets a notice and applies the side effect", async () => {
    vi.useRealTimers();
    const store = createCairnStore(new MockClient({}));
    await store.getState().init();
    await store.getState().invokePlugin("demo", "stamp");
    expect(store.getState().notice).toBe("stamp.md");
    await vi.waitFor(() =>
      expect(store.getState().notePaths).toContain("stamp.md"),
    );
    store.getState().dismissNotice();
    expect(store.getState().notice).toBeNull();
  });

  it("a slow backlinks response for the previous note does not overwrite the current note", async () => {
    vi.useRealTimers();
    const client = new MockClient({
      "a.md": "x", // no backlinks
      "b.md": "x",
      "links-to-b.md": "see [[b]]", // b.md has one backlink
    });
    let releaseA = () => {};
    const gateA = new Promise<void>((r) => (releaseA = r));
    const orig = client.runQuery.bind(client);
    vi.spyOn(client, "runQuery").mockImplementation(async (q) => {
      if (q.type === "get_backlinks" && q.path === "a.md") await gateA;
      return orig(q);
    });
    const store = createCairnStore(client);
    await store.getState().init();

    const openA = store.getState().openNote("a.md"); // backlinks query gated
    const openB = store.getState().openNote("b.md"); // resolves immediately
    await openB;
    expect(store.getState().activePath).toBe("b.md");
    expect(store.getState().backlinks).toEqual(["links-to-b.md"]);

    releaseA();
    await openA;
    // a.md's stale (empty) backlinks must NOT have overwritten b.md's.
    expect(store.getState().backlinks).toEqual(["links-to-b.md"]);
  });

  it("a self-write (autosave) does not trigger the expensive index-wide refresh storm", async () => {
    vi.useRealTimers();
    const client = new MockClient({ "a.md": "orig [[b]]", "b.md": "B" });
    const store = createCairnStore(client);
    await store.getState().init();
    await store.getState().loadGraph(); // graph non-null -> would rebuild
    await store.getState().openNote("a.md"); // activePath set
    const spy = vi.spyOn(client, "runQuery");

    store.getState().editBuffer("orig [[b]] more");
    await store.getState().saveActive(); // write_note -> echoed note_changed
    await Promise.resolve();
    await Promise.resolve();

    // Guard against a trivial pass: the write must actually have happened
    // (saveNote only sets uncommitted on a real successful write).
    expect(store.getState().uncommitted).toBe(true);
    // The storm — the index-wide list/tags refresh and the whole-graph rebuild —
    // must NOT fire for our own write.
    const storm = spy.mock.calls.filter(([q]) =>
      ["list_notes", "list_tags", "get_graph"].includes(q.type),
    );
    expect(storm).toEqual([]);
  });

  it("a self-write still refreshes the active note's targeted views (backlinks + open search)", async () => {
    vi.useRealTimers();
    // b.md does not yet match "needle" nor link to a.md.
    const client = new MockClient({ "a.md": "A", "b.md": "B" });
    const store = createCairnStore(client);
    await store.getState().init();
    await store.getState().openNote("a.md"); // a.md is active
    await store.getState().runSearch("needle"); // open search, no matches yet
    expect(store.getState().searchResults).toEqual([]);

    // Edit a.md so it now links to b.md AND contains the search term, then save.
    store.getState().editBuffer("A now links [[b]] and has a needle");
    await store.getState().saveActive(); // self-write -> echoed note_changed
    await Promise.resolve();
    await Promise.resolve();
    await vi.waitFor(() => {
      // The active search re-ran off the self-write and now finds a.md...
      expect(store.getState().searchResults).toContain("a.md");
    });
    // ...and b.md's backlinks would reflect a.md (targeted refresh still runs).
    await store.getState().openNote("b.md");
    expect(store.getState().backlinks).toContain("a.md");
  });

  it("starts with all loading flags false", () => {
    const { store } = setup();
    expect(store.getState().loading).toEqual({
      search: false,
      graph: false,
      backlinks: false,
      note: false,
      suggestions: false,
    });
  });

  it("exposes loading.search while a search is in flight, clearing when it lands", async () => {
    vi.useRealTimers();
    const { client, store } = setup();
    await store.getState().init();
    let resolve!: (v: QueryResponse) => void;
    vi.spyOn(client, "runQuery").mockReturnValueOnce(
      new Promise<QueryResponse>((r) => (resolve = r)),
    );
    const p = store.getState().runSearch("x");
    expect(store.getState().loading.search).toBe(true);
    resolve({ type: "search_results", results: [] });
    await p;
    expect(store.getState().loading.search).toBe(false);
  });

  it("exposes loading.note while a note's contents load", async () => {
    vi.useRealTimers();
    const { client, store } = setup();
    await store.getState().init();
    let resolve!: (v: QueryResponse) => void;
    vi.spyOn(client, "runQuery").mockReturnValueOnce(
      new Promise<QueryResponse>((r) => (resolve = r)),
    );
    const p = store.getState().openNote("a.md");
    expect(store.getState().loading.note).toBe(true);
    resolve({ type: "note", contents: "hi" });
    await p;
    expect(store.getState().loading.note).toBe(false);
  });

  it("surfaces an error (not a silent no-op) when a query returns an unexpected variant", async () => {
    const { client, store } = setup();
    await store.getState().init();
    const before = store.getState().notePaths;
    // A valid response shape, but the wrong variant for list_notes. The old code
    // silently dropped it, leaving the list stale with no diagnostic.
    vi.spyOn(client, "runQuery").mockResolvedValueOnce({
      type: "tags",
      tags: [],
    });
    await store.getState().refreshNotePaths();
    expect(store.getState().notePaths).toEqual(before); // not silently mutated
    expect(
      store.getState().errors.some((e) => e.message.includes("List notes")),
    ).toBe(true);
  });

  it("logs an unexpected query variant through the existing error channel", async () => {
    const { client, store } = setup();
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    await store.getState().init();
    vi.spyOn(client, "runQuery").mockResolvedValueOnce({
      type: "notes",
      notes: [],
    });
    await store.getState().loadTags(); // expects "tags", gets "notes"
    expect(spy).toHaveBeenCalledWith(
      expect.stringContaining("Load tags"),
      expect.objectContaining({ operation: "Load tags" }),
    );
    spy.mockRestore();
  });

  it("reuses one persistent autosave debounce across rapid keystrokes, saving once", async () => {
    const { client, store } = setup();
    await store.getState().init();
    await store.getState().openNote("a.md");
    const construct = vi.spyOn(timer, "debounce");
    const send = vi.spyOn(client, "sendCommand");
    store.getState().editBuffer("a1 [[b]]");
    store.getState().editBuffer("a2 [[b]]");
    store.getState().editBuffer("a3 [[b]]");
    // One persistent per-path debounce, merely re-triggered — not one per keystroke.
    expect(construct).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(DEFAULT_SETTINGS.autosaveMs);
    const writes = send.mock.calls.filter(([c]) => c.type === "write_note");
    expect(writes).toHaveLength(1);
    expect(writes[0][0]).toMatchObject({ path: "a.md", contents: "a3 [[b]]" });
  });

  it("exposes loading.graph while the graph loads", async () => {
    vi.useRealTimers();
    const { client, store } = setup();
    await store.getState().init();
    let resolve!: (v: QueryResponse) => void;
    vi.spyOn(client, "runQuery").mockReturnValueOnce(
      new Promise<QueryResponse>((r) => (resolve = r)),
    );
    const p = store.getState().loadGraph();
    expect(store.getState().loading.graph).toBe(true);
    resolve({ type: "graph", nodes: [], edges: [] });
    await p;
    expect(store.getState().loading.graph).toBe(false);
  });

  it("traces the refresh fan-out for an external note_changed (DG5/DX4)", async () => {
    // Real timers: the mock emits via queueMicrotask and vi.waitFor polls on
    // real timers (mirrors the note_changed test above).
    vi.useRealTimers();
    const events: { type: string; actions: string[] }[] = [];
    const trace = {
      event: (type: string, actions: string[]) =>
        events.push({ type, actions }),
      time: <T>(_label: string, fn: () => Promise<T>) => fn(),
    };
    const client = new MockClient({ "a.md": "links to [[b]]", "b.md": "x" });
    const store = createCairnStore(client, undefined, trace);
    await store.getState().init();
    // An external write (straight to the client, not via store.saveNote) is not
    // a self-write, so it triggers the full index-wide fan-out.
    await client.sendCommand({
      type: "write_note",
      path: "c.md",
      contents: "hi",
    });
    await vi.waitFor(() =>
      expect(events.some((e) => e.type === "note_changed")).toBe(true),
    );
    const fanout = events.find((e) => e.type === "note_changed")!;
    expect(fanout.actions).toContain("refreshNotePaths");
    expect(fanout.actions).toContain("loadTags");
  });

  it("skips the index-wide fan-out for a self-write echo (DG5/DX4)", async () => {
    vi.useRealTimers();
    const events: { type: string; actions: string[] }[] = [];
    const trace = {
      event: (type: string, actions: string[]) =>
        events.push({ type, actions }),
      time: <T>(_label: string, fn: () => Promise<T>) => fn(),
    };
    const client = new MockClient({ "a.md": "body" });
    const store = createCairnStore(client, undefined, trace);
    await store.getState().init();
    await store.getState().openNote("a.md");
    events.length = 0; // ignore the open's backlinks refresh
    store.getState().editBuffer("body edited");
    // The debounced autosave writes via the store, marking pendingSelfWrites, so
    // the echoed note_changed is a self-write: no refreshNotePaths/loadTags.
    await vi.waitFor(
      () => expect(events.some((e) => e.type === "note_changed")).toBe(true),
      { timeout: 3000 },
    );
    const echo = events.find((e) => e.type === "note_changed")!;
    expect(echo.actions).not.toContain("refreshNotePaths");
    expect(echo.actions).not.toContain("loadTags");
  });
});

describe("treeStyles", () => {
  it("setTreeStyle stores and persists a style", () => {
    const { store } = setup();
    store
      .getState()
      .setTreeStyle("a.md", { icon: { kind: "emoji", value: "📚" } });
    expect(store.getState().treeStyles["a.md"]).toEqual({
      icon: { kind: "emoji", value: "📚" },
    });
    expect(
      JSON.parse(localStorage.getItem("cairn.treeIcons")!)["a.md"],
    ).toEqual({
      icon: { kind: "emoji", value: "📚" },
    });
  });

  it("setTreeStyle with an empty style deletes the key", () => {
    const { store } = setup();
    store
      .getState()
      .setTreeStyle("a.md", { icon: { kind: "emoji", value: "📚" } });
    store.getState().setTreeStyle("a.md", {});
    expect(store.getState().treeStyles["a.md"]).toBeUndefined();
  });

  it("applyRenames remaps a note's style key to the new path", async () => {
    const { store } = setup();
    await store.getState().init();
    store
      .getState()
      .setTreeStyle("a.md", { icon: { kind: "emoji", value: "📚" } });
    await store.getState().applyRenames([{ from: "a.md", to: "c.md" }]);
    expect(store.getState().treeStyles["a.md"]).toBeUndefined();
    expect(store.getState().treeStyles["c.md"]).toEqual({
      icon: { kind: "emoji", value: "📚" },
    });
    expect(
      JSON.parse(localStorage.getItem("cairn.treeIcons")!)["c.md"],
    ).toEqual({ icon: { kind: "emoji", value: "📚" } });
  });

  it("deleteNote drops the note's style key", async () => {
    const { store } = setup();
    await store.getState().init();
    store
      .getState()
      .setTreeStyle("a.md", { icon: { kind: "emoji", value: "📚" } });
    await store.getState().deleteNote("a.md");
    expect(store.getState().treeStyles["a.md"]).toBeUndefined();
  });

  it("remapFolderStyles moves a note-less folder's color (and persists)", () => {
    const { store } = setup();
    store.getState().setTreeStyle("empty", { folderColor: "#30a46c" });
    store.getState().remapFolderStyles("empty", "moved");
    expect(store.getState().treeStyles["empty"]).toBeUndefined();
    expect(store.getState().treeStyles["moved"]).toEqual({
      folderColor: "#30a46c",
    });
    expect(
      JSON.parse(localStorage.getItem("cairn.treeIcons")!)["moved"],
    ).toEqual({ folderColor: "#30a46c" });
  });
});

describe("ui slice", () => {
  it("setUi patches ui flags without touching others", () => {
    const { store } = setup();
    store.getState().setUi({ settingsOpen: true });
    expect(store.getState().ui.settingsOpen).toBe(true);
    store.getState().setUi({ newNoteOpen: true, newNoteInitial: "folder/" });
    expect(store.getState().ui.newNoteOpen).toBe(true);
    expect(store.getState().ui.newNoteInitial).toBe("folder/");
    expect(store.getState().ui.settingsOpen).toBe(true); // untouched
  });

  it("setKeybindingOverrides updates state and persists", () => {
    const { store } = setup();
    store.getState().setKeybindingOverrides({ "new-note": "Mod+Shift+N" });
    expect(store.getState().ui.keybindingOverrides).toEqual({
      "new-note": "Mod+Shift+N",
    });
    expect(loadOverrides()).toEqual({ "new-note": "Mod+Shift+N" });
  });

  it("init seeds keybindingOverrides from persistence", async () => {
    saveOverrides({ commit: null });
    const { store } = setup();
    await store.getState().init();
    expect(store.getState().ui.keybindingOverrides).toEqual({ commit: null });
  });
});

describe("split panes", () => {
  async function ready() {
    const { store } = setup();
    await store.getState().init();
    return store;
  }

  it("splitPane duplicates the focused note into a new focused pane", async () => {
    const store = await ready();
    await store.getState().openNote("a.md");
    store.getState().splitPane();
    const s = store.getState();
    expect(s.panes).toHaveLength(2);
    expect(s.activePane).toBe(1);
    expect(s.panes[1].activePath).toBe("a.md");
    expect(s.activePath).toBe("a.md");
  });

  it("splitPane is a no-op with no active note", async () => {
    const store = await ready();
    store.getState().splitPane();
    expect(store.getState().panes).toHaveLength(1);
  });

  it("openToSide opens a chosen note in the other pane and focuses it", async () => {
    const store = await ready();
    await store.getState().openNote("a.md");
    await store.getState().openToSide("b.md");
    const s = store.getState();
    expect(s.panes).toHaveLength(2);
    expect(s.activePane).toBe(1);
    expect(s.panes[0].activePath).toBe("a.md");
    expect(s.panes[1].activePath).toBe("b.md");
    expect(s.activePath).toBe("b.md");
  });

  it("openToSide on an existing split targets the non-focused pane", async () => {
    const store = await ready();
    await store.getState().openNote("a.md");
    await store.getState().openToSide("b.md"); // panes [a][b], focus 1
    store.getState().focusPane(0);
    await store.getState().openToSide("a.md"); // into pane 1 (non-focused)
    const s = store.getState();
    expect(s.panes).toHaveLength(2);
    expect(s.activePane).toBe(1);
    expect(s.panes[1].activePath).toBe("a.md");
  });

  it("focusPane moves the mirror to the target pane", async () => {
    const store = await ready();
    await store.getState().openNote("a.md");
    await store.getState().openToSide("b.md");
    store.getState().focusPane(0);
    expect(store.getState().activePath).toBe("a.md");
  });

  it("closePane collapses to a single pane", async () => {
    const store = await ready();
    await store.getState().openNote("a.md");
    await store.getState().openToSide("b.md");
    store.getState().closePane(1);
    const s = store.getState();
    expect(s.panes).toHaveLength(1);
    expect(s.activePane).toBe(0);
    expect(s.activePath).toBe("a.md");
  });

  it("setSplitRatio clamps to [0.2, 0.8]", async () => {
    const store = await ready();
    store.getState().setSplitRatio(0.95);
    expect(store.getState().splitRatio).toBe(0.8);
    store.getState().setSplitRatio(0.05);
    expect(store.getState().splitRatio).toBe(0.2);
  });

  it("deleteNote removes the tab from every pane that holds it", async () => {
    const store = await ready();
    await store.getState().openNote("a.md");
    await store.getState().openToSide("a.md"); // a.md now open in both panes
    await store.getState().deleteNote("a.md");
    const s = store.getState();
    for (const pane of s.panes) {
      expect(pane.tabs.some((t) => t.path === "a.md")).toBe(false);
    }
  });

  it("loadPlugins groups contributions by slot, tagged with an epoch", async () => {
    vi.useRealTimers();
    const store = createCairnStore(new MockClient({}));
    await store.getState().init();
    const s = store.getState();
    expect(s.pluginEpoch).toBeGreaterThan(0);
    const sidebar = s.pluginContributions["sidebar.section"];
    // wordcount (order:100) sorts before demo (order:null); find demo by plugin id.
    const demoEntry = sidebar.find((e) => e.plugin === "demo");
    expect(demoEntry).toBeDefined();
    expect(demoEntry!.epoch).toBe(s.pluginEpoch);
  });

  it("invokePlugin forwards args to the plugin (mock echoes args.n)", async () => {
    vi.useRealTimers();
    const store = createCairnStore(new MockClient({}));
    await store.getState().init();
    await store.getState().invokePlugin("demo", "stamp", { n: 1 });
    expect(store.getState().notice).toContain("1");
  });

  it("loadPlugins bumps the epoch monotonically on each call", async () => {
    vi.useRealTimers();
    const store = createCairnStore(new MockClient({}));
    await store.getState().init();
    const first = store.getState().pluginEpoch;
    await store.getState().loadPlugins();
    expect(store.getState().pluginEpoch).toBeGreaterThan(first);
  });

  it("a plugin with no contributions sources no slot entries", async () => {
    vi.useRealTimers();
    const store = createCairnStore(new MockClient({}));
    await store.getState().init();
    const grouped = store.getState().pluginContributions;
    for (const entries of Object.values(grouped)) {
      for (const e of entries) expect(e.plugin).not.toBe("bare");
    }
  });

  it("loadPlugins registers ui-roots for plugins that declare uiRoot", async () => {
    vi.useRealTimers();
    const setPluginUiRoots = vi.fn(() => Promise.resolve());
    const host = {
      currentCairn: () => Promise.resolve<string | null>("(fixture)"),
      openCairn: () => Promise.resolve<string | null>("(fixture)"),
      assetUrl: (p: string) => p,
      setPluginUiRoots,
    };
    // The mock seeds `wordcount` with uiRoot "(mock)/wordcount/ui"; demo/bare
    // have no uiRoot and must be excluded from the registered map.
    const store = createCairnStore(new MockClient({}), host);
    await store.getState().init();
    expect(setPluginUiRoots).toHaveBeenCalledWith({
      wordcount: "(mock)/wordcount/ui",
    });
  });
});

describe("mobile ui state", () => {
  function freshStore() {
    return createCairnStore(new MockClient({}));
  }

  it("defaults mobileTab to editor and backlinksOpen to false", () => {
    const ui = freshStore().getState().ui;
    expect(ui.mobileTab).toBe("editor");
    expect(ui.backlinksOpen).toBe(false);
  });

  it("setUi patches mobile fields", () => {
    const store = freshStore();
    store.getState().setUi({ mobileTab: "files", backlinksOpen: true });
    expect(store.getState().ui.mobileTab).toBe("files");
    expect(store.getState().ui.backlinksOpen).toBe(true);
  });
});

describe("temporal graph", () => {
  const snaps = {
    r1: { notes: { "a.md": "lone" } },
    r2: { notes: { "a.md": "links [[b]]", "b.md": "hi" } },
  };

  it("loadTimeline populates the timeline from note_history", async () => {
    const history = {
      "a.md": {
        revisions: [
          {
            id: "r2",
            message: "add b",
            timestamp_secs: 20,
            author: "x",
            summary: null,
            name: null,
          },
          {
            id: "r1",
            message: "init",
            timestamp_secs: 10,
            author: "x",
            summary: null,
            name: null,
          },
        ],
        contents: { r1: "lone", r2: "links [[b]]" },
      },
    };
    const { store } = setup({ history });
    await store.getState().init();
    await store.getState().loadTimeline("a.md");
    expect(store.getState().temporal.timeline?.map((r) => r.id)).toEqual([
      "r2",
      "r1",
    ]);
  });

  it("loadVaultTimeline populates the timeline from vault_history", async () => {
    const vaultRevisions = [
      {
        id: "r3",
        message: "c",
        timestamp_secs: 30,
        author: "x",
        summary: null,
        name: null,
      },
      {
        id: "r2",
        message: "b",
        timestamp_secs: 20,
        author: "x",
        summary: null,
        name: null,
      },
      {
        id: "r1",
        message: "a",
        timestamp_secs: 10,
        author: "x",
        summary: null,
        name: null,
      },
    ];
    const { client, store } = setup({ vaultRevisions });
    const spy = vi.spyOn(client, "runQuery");
    await store.getState().init();
    await store.getState().loadVaultTimeline();
    expect(spy).toHaveBeenCalledWith({ type: "vault_history", limit: null });
    expect(store.getState().temporal.timeline?.map((r) => r.id)).toEqual([
      "r3",
      "r2",
      "r1",
    ]);
  });

  it("loadVaultTimeline(true) swaps the source to structural_revisions", async () => {
    // Distinct fixtures per source so the assertion proves the toggle swaps the
    // data source (engine pre-filters; the UI does NO client-side filtering).
    const vaultRevisions = [
      {
        id: "v2",
        message: "tag edit",
        timestamp_secs: 20,
        author: "x",
        summary: null,
        name: null,
      },
      {
        id: "v1",
        message: "init",
        timestamp_secs: 10,
        author: "x",
        summary: null,
        name: null,
      },
    ];
    const structuralRevisions = [
      {
        id: "v1",
        message: "init",
        timestamp_secs: 10,
        author: "x",
        summary: null,
        name: null,
      },
    ];
    const { client, store } = setup({ vaultRevisions, structuralRevisions });
    const spy = vi.spyOn(client, "runQuery");
    await store.getState().init();
    await store.getState().loadVaultTimeline(true);
    expect(spy).toHaveBeenCalledWith({
      type: "structural_revisions",
      limit: 500,
    });
    expect(store.getState().temporal.timeline?.map((r) => r.id)).toEqual([
      "v1",
    ]);
  });

  it("loadVaultTimeline ignores a stale response (token guard)", async () => {
    const { client, store } = setup();
    await store.getState().init();
    // Two in-flight loads with DISTINCT payloads; the first-issued (stale) call
    // resolves LAST. Without the token guard its response would clobber the
    // fresher one — so the payloads must differ for the test to discriminate.
    let resolveStale!: (r: QueryResponse) => void;
    let resolveFresh!: (r: QueryResponse) => void;
    vi.spyOn(client, "runQuery")
      .mockReturnValueOnce(
        new Promise<QueryResponse>((r) => (resolveStale = r)),
      )
      .mockReturnValueOnce(
        new Promise<QueryResponse>((r) => (resolveFresh = r)),
      );
    const p1 = store.getState().loadVaultTimeline(); // token 1 — stale
    const p2 = store.getState().loadVaultTimeline(); // token 2 — fresh, wins
    resolveFresh({
      type: "history",
      revisions: [
        {
          id: "fresh",
          message: "b",
          timestamp_secs: 20,
          author: "x",
          summary: null,
          name: null,
        },
      ],
    });
    resolveStale({
      type: "history",
      revisions: [
        {
          id: "stale",
          message: "a",
          timestamp_secs: 10,
          author: "x",
          summary: null,
          name: null,
        },
      ],
    });
    await Promise.all([p1, p2]);
    expect(store.getState().temporal.timeline?.map((r) => r.id)).toEqual([
      "fresh",
    ]);
  });

  it("loadSnapshot fills snapshot from graph_at and clears diff", async () => {
    const { store } = setup({ vaultSnapshots: snaps });
    await store.getState().init();
    await store.getState().loadSnapshot("r2");
    const t = store.getState().temporal;
    expect(t.snapshot?.nodes.map((n) => n.path)).toEqual(["a.md", "b.md"]);
    expect(t.diff).toBeNull();
  });

  it("loadDiff fills base snapshot + deltas", async () => {
    const { store } = setup({ vaultSnapshots: snaps });
    await store.getState().init();
    await store.getState().loadDiff("r1", "r2");
    const t = store.getState().temporal;
    expect(t.snapshot?.nodes.map((n) => n.path)).toEqual(["a.md", "b.md"]);
    expect(t.diff?.nodes_added.map((n) => n.path)).toEqual(["b.md"]);
  });

  it("clearTemporal drops snapshot and diff but keeps the timeline", async () => {
    const { store } = setup({ vaultSnapshots: snaps });
    await store.getState().init();
    await store.getState().loadSnapshot("r2");
    store.getState().clearTemporal();
    expect(store.getState().temporal.snapshot).toBeNull();
    expect(store.getState().temporal.diff).toBeNull();
  });

  it("a slow loadSnapshot resolving after loadDiff does not clobber the compare view", async () => {
    const { client, store } = setup({ vaultSnapshots: snaps });
    await store.getState().init();

    // Gate the snapshot's own graph_at("r1") call so it resolves AFTER the
    // subsequent loadDiff — simulating click 1 (snapshot r1) being slower
    // than click 2 (compare r1..r2).
    let resolveGate: () => void = () => {};
    const gate = new Promise<void>((resolve) => {
      resolveGate = resolve;
    });
    const original = client.runQuery.bind(client);
    vi.spyOn(client, "runQuery").mockImplementation(async (q) => {
      if (q.type === "graph_at" && q.revision === "r1") await gate;
      return original(q);
    });

    const snapshotPromise = store.getState().loadSnapshot("r1");
    await store.getState().loadDiff("r1", "r2");
    const afterDiff = store.getState().temporal;
    expect(afterDiff.diff?.nodes_added.map((n) => n.path)).toEqual(["b.md"]);

    // Now let the slow snapshot resolve — it must be superseded (shared
    // temporalData token) and must NOT overwrite the diff/snapshot the
    // compare view is showing.
    resolveGate();
    await snapshotPromise;

    const afterSnapshot = store.getState().temporal;
    expect(afterSnapshot.diff).toEqual(afterDiff.diff);
    expect(afterSnapshot.snapshot).toEqual(afterDiff.snapshot);
  });

  it("clearTemporal supersedes an in-flight loadSnapshot so it can't repopulate after going live", async () => {
    const { client, store } = setup({ vaultSnapshots: snaps });
    await store.getState().init();

    let resolveGate: () => void = () => {};
    const gate = new Promise<void>((resolve) => {
      resolveGate = resolve;
    });
    const original = client.runQuery.bind(client);
    vi.spyOn(client, "runQuery").mockImplementation(async (q) => {
      if (q.type === "graph_at" && q.revision === "r2") await gate;
      return original(q);
    });

    const pending = store.getState().loadSnapshot("r2");
    store.getState().clearTemporal(); // user returns to Live before it resolves
    resolveGate();
    await pending;

    const t = store.getState().temporal;
    expect(t.snapshot).toBeNull();
    expect(t.diff).toBeNull();
  });

  it("reloadNoteBuffer re-fetches a clean buffer after an external change", async () => {
    const { client, store } = setup();
    await store.getState().init();
    await store.getState().openNote("a.md");
    expect(store.getState().openNotes["a.md"].contents).toBe("links to [[b]]");

    // Simulate an out-of-band disk write (e.g. a restore) that the mock's
    // note_changed event does NOT propagate into the open buffer.
    await client.sendCommand({
      type: "write_note",
      path: "a.md",
      contents: "NEW",
    });
    expect(store.getState().openNotes["a.md"].contents).toBe("links to [[b]]"); // unchanged by the watcher alone

    await store.getState().reloadNoteBuffer("a.md");
    expect(store.getState().openNotes["a.md"].contents).toBe("NEW");
    expect(store.getState().openNotes["a.md"].dirty).toBe(false);
  });

  it("reloadNoteBuffer is a no-op when the buffer is dirty (never clobbers unsaved edits)", async () => {
    const { client, store } = setup();
    await store.getState().init();
    await store.getState().openNote("a.md");
    store.getState().editBuffer("unsaved edit"); // marks dirty, schedules autosave

    await client.sendCommand({
      type: "write_note",
      path: "a.md",
      contents: "EXTERNAL",
    });

    await store.getState().reloadNoteBuffer("a.md");
    expect(store.getState().openNotes["a.md"].contents).toBe("unsaved edit");
    expect(store.getState().openNotes["a.md"].dirty).toBe(true);
  });
});
