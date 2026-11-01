# Store Stale-Response Races + Autosave Refresh Storm — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:test-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stop slow async store responses from clobbering the current selection, and stop the user's own autosave from triggering a full index/search/graph refresh cascade.

**Architecture:** Two coupled fixes in `web/src/store/store.ts`, sharing the store closure:
1. **Stale-response guard** — a closure-scoped monotonic token per logical target (`backlinks`, `results`, `graph`). Each guarded async action bumps its token before awaiting and bails after the await if a newer request of the same kind has started. `runSearch` + `filterByTag` share the `results` token because both write the search overlay.
2. **Self-write echo suppression** — a closure-scoped `Map<string, number>` of pending self-writes. `saveNote` records a pending write for its path before sending `write_note`; the `note_changed` subscribe handler consumes one pending mark for that path and skips the refresh cascade. Only `saveNote` marks pending, so `createNote`/`deleteNote`/`applyRenames`/`invokePlugin` echoes still drive refreshes.

**Tech Stack:** TypeScript, Zustand vanilla store, Vitest + fake/real timers, MockClient.

---

### Task 1: Failing tests

**Files:**
- Test: `web/src/store/store.test.ts`

- [ ] **Step 1: Write failing test — stale backlinks for the previous note must not overwrite the current note**

```ts
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
```

- [ ] **Step 2: Write failing test — a self-write (autosave) must not trigger the refresh cascade**

```ts
it("a self-write (autosave) does not trigger the index/search/graph refresh cascade", async () => {
  vi.useRealTimers();
  const client = new MockClient({ "a.md": "orig [[b]]", "b.md": "B" });
  const store = createCairnStore(client);
  await store.getState().init();
  await store.getState().loadGraph(); // graph non-null -> would refresh
  await store.getState().openNote("a.md"); // activePath set -> would refresh backlinks
  const spy = vi.spyOn(client, "runQuery");

  store.getState().editBuffer("orig [[b]] more");
  await store.getState().saveActive(); // write_note -> echoed note_changed
  await Promise.resolve();
  await Promise.resolve();

  const cascade = spy.mock.calls.filter(([q]) =>
    ["list_notes", "list_tags", "get_backlinks", "get_graph"].includes(q.type),
  );
  expect(cascade).toEqual([]);
});
```

- [ ] **Step 3: Run both, verify they FAIL**

Run: `cd web && npx vitest run src/store/store.test.ts`
Expected: both new tests FAIL (stale backlinks overwritten to `[]`; cascade non-empty).

---

### Task 2: Implement the stale-response guard

**Files:**
- Modify: `web/src/store/store.ts` (closure declarations; `refreshBacklinks`, `runSearch`, `filterByTag`, `loadGraph`)

- [ ] **Step 1: Add a closure-scoped token record** beside `autosaves`:

```ts
// Monotonic request tokens: a slow response only applies if no newer request
// of the same kind has started since. runSearch + filterByTag share `results`
// because both write the search overlay.
const seq = { backlinks: 0, results: 0, graph: 0 };
```

- [ ] **Step 2: Guard each action** — bump the token before the await, bail after if superseded. e.g.:

```ts
async refreshBacklinks() {
  const path = get().activePath;
  if (!path) return set({ backlinks: [] });
  const token = ++seq.backlinks;
  try {
    const res = await client.runQuery({ type: "get_backlinks", path });
    if (token !== seq.backlinks) return; // superseded by a newer request
    if (res.type === "paths") set({ backlinks: res.paths });
  } catch (err) {
    if (token !== seq.backlinks) return;
    set({ error: errMsg(err) });
  }
}
```

Apply the same shape to `runSearch` (token `++seq.results`), `filterByTag` (token `++seq.results`), and `loadGraph` (token `++seq.graph`, guarding both the `get_graph` set and the `noteTags()` set).

- [ ] **Step 3: Run the stale-backlinks test, verify PASS**

Run: `cd web && npx vitest run src/store/store.test.ts -t "slow backlinks"`
Expected: PASS.

---

### Task 3: Implement self-write echo suppression

**Files:**
- Modify: `web/src/store/store.ts` (closure declaration; `init` subscribe handler; `saveNote`)

- [ ] **Step 1: Add a closure-scoped pending-write counter** beside `autosaves`:

```ts
// Paths the client itself just wrote, awaiting their note_changed echo. The echo
// of our own write must not trigger the refresh cascade (autosave storm).
const pendingSelfWrites = new Map<string, number>();
```

- [ ] **Step 2: In the subscribe handler, consume + skip self-write echoes** (only `note_changed`):

```ts
if (e.type === "note_changed" || e.type === "note_deleted") {
  if (e.type === "note_changed") {
    const pending = pendingSelfWrites.get(e.path) ?? 0;
    if (pending > 0) {
      if (pending === 1) pendingSelfWrites.delete(e.path);
      else pendingSelfWrites.set(e.path, pending - 1);
      return; // our own write — already reflected locally
    }
  }
  void get().refreshNotePaths();
  // ... unchanged cascade ...
}
```

- [ ] **Step 3: In `saveNote`, mark the pending write before sending; unmark on failure:**

```ts
setBuffer(path, { saving: true });
pendingSelfWrites.set(path, (pendingSelfWrites.get(path) ?? 0) + 1);
try {
  await client.sendCommand({ type: "write_note", path, contents: snapshot });
  // ... unchanged ...
} catch (err) {
  pendingSelfWrites.set(path, Math.max(0, (pendingSelfWrites.get(path) ?? 1) - 1));
  if (get().openNotes[path]) setBuffer(path, { saving: false });
  set({ error: errMsg(err) });
}
```

- [ ] **Step 4: Run the self-write test, verify PASS**

Run: `cd web && npx vitest run src/store/store.test.ts -t "self-write"`
Expected: PASS.

---

### Task 4: Verify + commit

- [ ] **Step 1: Full store + typecheck + lint**

Run: `cd web && npx vitest run && npx tsc --noEmit && npm run lint`
Expected: all green, no regressions.

- [ ] **Step 2: Commit** (Co-Authored-By: Claude Fable 5), push, open PR against `tau-rs/cairn-web-ui:main`, cite D1 + D2. No merge.
