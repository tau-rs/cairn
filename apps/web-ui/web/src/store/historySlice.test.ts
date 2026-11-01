import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { createCairnStore } from "./store";
import { MockClient } from "../client/mock";
import type { Revision } from "../contract";

beforeEach(() => vi.useFakeTimers());
beforeEach(() => localStorage.clear());
afterEach(() => vi.useRealTimers());

const REVS: Revision[] = [
  {
    id: "r2",
    message: "second",
    timestamp_secs: 2,
    author: "tau",
    summary: null,
    name: null,
  },
  {
    id: "r1",
    message: "first",
    timestamp_secs: 1,
    author: "tau",
    summary: null,
    name: null,
  },
];

function setup() {
  const client = new MockClient(
    { "n.md": "current", "x.md": "no history" },
    { "n.md": { revisions: REVS, contents: { r2: "v2", r1: "v1" } } },
  );
  const store = createCairnStore(client);
  return { client, store };
}

describe("history slice — loadHistory", () => {
  it("defaults to no history and the backlinks tab", () => {
    const { store } = setup();
    expect(store.getState().history).toBeNull();
    expect(store.getState().rightTab).toBe("backlinks");
  });

  it("loadHistory populates revisions for the active note", async () => {
    const { store } = setup();
    await store.getState().init();
    await store.getState().openNote("n.md");
    await store.getState().loadHistory();
    expect(store.getState().history).toEqual(REVS);
    expect(store.getState().historyPath).toBe("n.md");
  });

  it("loadHistory is a no-op with no active note", async () => {
    const { store } = setup();
    await store.getState().init();
    await store.getState().loadHistory();
    expect(store.getState().history).toBeNull();
  });

  it("setRightTab switches the active aside tab", () => {
    const { store } = setup();
    store.getState().setRightTab("history");
    expect(store.getState().rightTab).toBe("history");
  });
});

describe("history slice — view revision", () => {
  it("viewRevision loads the note at a revision (read-only state)", async () => {
    const { store } = setup();
    await store.getState().init();
    await store.getState().openNote("n.md");
    await store.getState().viewRevision("r1");
    expect(store.getState().viewingRevision).toEqual({
      path: "n.md",
      revision: "r1",
      contents: "v1",
    });
  });

  it("exitRevisionView clears the viewing state", async () => {
    const { store } = setup();
    await store.getState().init();
    await store.getState().openNote("n.md");
    await store.getState().viewRevision("r1");
    store.getState().exitRevisionView();
    expect(store.getState().viewingRevision).toBeNull();
  });

  it("viewRevision surfaces an error toast for an unknown revision", async () => {
    const { store } = setup();
    await store.getState().init();
    await store.getState().openNote("n.md");
    await store.getState().viewRevision("nope");
    expect(store.getState().viewingRevision).toBeNull();
    expect(store.getState().errors.length).toBeGreaterThan(0);
  });
});

describe("history slice — restore", () => {
  it("restoreRevision overwrites the buffer, marks uncommitted, exits view", async () => {
    const { store } = setup();
    await store.getState().init();
    await store.getState().openNote("n.md");
    await store.getState().viewRevision("r1");
    await store.getState().restoreRevision("r1");
    expect(store.getState().activeContents).toBe("v1");
    expect(store.getState().uncommitted).toBe(true);
    expect(store.getState().viewingRevision).toBeNull();
  });

  it("restoreRevision surfaces an error for an unknown revision", async () => {
    const { store } = setup();
    await store.getState().init();
    await store.getState().openNote("n.md");
    await store.getState().restoreRevision("nope");
    expect(store.getState().errors.length).toBeGreaterThan(0);
  });
});

describe("history slice — showHistory", () => {
  it("showHistory selects the tab, opens the drawer, and loads history", async () => {
    const { store } = setup();
    await store.getState().init();
    await store.getState().openNote("n.md");
    store.getState().showHistory();
    expect(store.getState().rightTab).toBe("history");
    expect(store.getState().ui.backlinksOpen).toBe(true);
    await vi.waitFor(() => expect(store.getState().history).toEqual(REVS));
  });
});

describe("history slice — race guard", () => {
  it("a superseded (stale) loadHistory response never clobbers a newer one", async () => {
    const N1: Revision[] = [
      {
        id: "n1",
        message: "n",
        timestamp_secs: 1,
        author: "t",
        summary: null,
        name: null,
      },
    ];
    const X1: Revision[] = [
      {
        id: "x1",
        message: "x",
        timestamp_secs: 1,
        author: "t",
        summary: null,
        name: null,
      },
    ];
    const client = new MockClient(
      { "n.md": "n", "x.md": "x" },
      {
        "n.md": { revisions: N1, contents: {} },
        "x.md": { revisions: X1, contents: {} },
      },
    );
    // Hold n.md's note_history open until we release it, so it can resolve
    // AFTER x.md's — exercising the historySeq supersession guard.
    const orig = client.runQuery.bind(client);
    let releaseN: () => Promise<void> = async () => {};
    client.runQuery = (q) => {
      if (q.type === "note_history" && q.path === "n.md") {
        return new Promise((res) => {
          releaseN = async () => res(await orig(q));
        });
      }
      return orig(q);
    };
    const store = createCairnStore(client);
    await store.getState().init();

    await store.getState().openNote("n.md");
    const pN = store.getState().loadHistory(); // in-flight, blocked
    await store.getState().openNote("x.md");
    await store.getState().loadHistory(); // resolves first, with x.md
    expect(store.getState().history).toEqual(X1);

    await releaseN(); // the stale n.md response now resolves
    await pN;
    expect(store.getState().history).toEqual(X1); // not clobbered
  });
});
