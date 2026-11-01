# Observability & Resilience Polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the cairn-ui reactive pipeline and floating-promise failures observable, and make the global keydown less eager — five LOW findings (DG4, DG5, U4, D9, DX4) in one PR, no product-behavior change.

**Architecture:** Five localized, additive changes. A global `unhandledrejection` handler installed at app entry (DG4). A dev-only, injectable `RefreshTrace` threaded through the store's event handler so the index-refresh fan-out is traceable and timed in dev, a no-op in prod (DG5). A focus guard on the `useGlobalKeys` window listener that skips dispatch when focus is in an editable/CodeMirror/dialog target, with a palette/commit allowlist (U4). A thin typed wrapper module localizing the `react-force-graph` casts (D9). One store↔subscribe integration test driven by the injected tracer, doubling as DG5's regression guard (DX4).

**Tech Stack:** TypeScript, React, Zustand (vanilla store), Vitest + jsdom + Testing Library, Vite (`import.meta.env.DEV` for the dev flag), react-force-graph-2d.

---

## File Structure

- `web/src/app/globalErrorHandler.ts` — **create.** `installGlobalRejectionHandler()` — window `unhandledrejection` listener (DG4).
- `web/src/app/globalErrorHandler.test.ts` — **create.** Tests for the handler (DG4).
- `web/src/main.tsx` — **modify.** Install the handler before render (DG4).
- `web/src/store/trace.ts` — **create.** `RefreshTrace` interface + `noopTrace` + dev `console`-based tracer gated on `import.meta.env.DEV`; exported singleton `refreshTrace` (DG5).
- `web/src/store/trace.test.ts` — **create.** Unit tests for `noopTrace` / dev tracer behavior (DG5).
- `web/src/store/store.ts` — **modify.** Accept an injected `RefreshTrace` (3rd param, defaults to `refreshTrace`); record event→actions and time the fan-out in `onEvent` (DG5).
- `web/src/store/store.test.ts` — **modify.** Add the store↔subscribe integration test using a recording tracer (DX4 + DG5 regression guard).
- `web/src/components/shortcuts/useGlobalKeys.ts` — **modify.** Add `isEditableTarget` guard + palette/commit allowlist before dispatch (U4).
- `web/src/components/shortcuts/useGlobalKeys.test.ts` — **modify.** Add focus-guard + allowlist tests (U4).
- `web/src/components/graph/forceGraphTypes.ts` — **create.** Thin typed wrapper: `RFNode`, `RFGraphData`, `LinkForce`, `FG`, `asGraphData()`, `linkForce()` (D9).
- `web/src/components/graph/forceGraphTypes.test.ts` — **create.** Round-trips the data shape (D9).
- `web/src/components/GraphView.tsx` — **modify.** Import the wrapper; drop the inline `RFNode` interface and the two `as` casts (D9).

**Working dir for all commands:** `web/` (`cd web` first; that's where `package.json`/vitest live).

---

## Task 1: Global unhandledrejection handler (DG4)

**Files:**
- Create: `web/src/app/globalErrorHandler.ts`
- Create: `web/src/app/globalErrorHandler.test.ts`
- Modify: `web/src/main.tsx`

- [ ] **Step 1: Write the failing test**

Create `web/src/app/globalErrorHandler.test.ts`:

```ts
import { describe, it, expect, vi, afterEach } from "vitest";
import { installGlobalRejectionHandler } from "./globalErrorHandler";

describe("installGlobalRejectionHandler", () => {
  let dispose: (() => void) | null = null;
  afterEach(() => {
    dispose?.();
    dispose = null;
  });

  it("invokes the sink with the rejection reason", () => {
    const sink = vi.fn();
    dispose = installGlobalRejectionHandler(sink);
    const reason = new Error("escaped");
    window.dispatchEvent(
      new PromiseRejectionEvent("unhandledrejection", {
        promise: Promise.reject(reason).catch(() => {}) as Promise<unknown>,
        reason,
      }),
    );
    expect(sink).toHaveBeenCalledWith(reason);
  });

  it("logs a structured diagnostic by default", () => {
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    dispose = installGlobalRejectionHandler();
    const reason = new Error("escaped");
    window.dispatchEvent(
      new PromiseRejectionEvent("unhandledrejection", {
        promise: Promise.reject(reason).catch(() => {}) as Promise<unknown>,
        reason,
      }),
    );
    expect(spy).toHaveBeenCalledWith(
      "[cairn] unhandled promise rejection",
      expect.objectContaining({ error: reason }),
    );
    spy.mockRestore();
  });

  it("stops handling after dispose", () => {
    const sink = vi.fn();
    installGlobalRejectionHandler(sink)(); // install then immediately dispose
    window.dispatchEvent(
      new PromiseRejectionEvent("unhandledrejection", {
        promise: Promise.reject("x").catch(() => {}) as Promise<unknown>,
        reason: "x",
      }),
    );
    expect(sink).not.toHaveBeenCalled();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd web && npx vitest run src/app/globalErrorHandler.test.ts`
Expected: FAIL — `Cannot find module './globalErrorHandler'`.

- [ ] **Step 3: Write minimal implementation**

Create `web/src/app/globalErrorHandler.ts`:

```ts
/**
 * Install a window-level `unhandledrejection` handler so the codebase's liberal
 * `void promise` pattern fails loud: any rejection that escapes a store action's
 * own try/catch (a throw before its try block, or a future action that forgets
 * one) is reported instead of vanishing silently. Returns a disposer that
 * removes the listener.
 *
 * `sink` defaults to a structured `console.error`; callers may pass their own to
 * surface escaped rejections elsewhere.
 */
export function installGlobalRejectionHandler(
  sink: (reason: unknown) => void = (reason) =>
    console.error("[cairn] unhandled promise rejection", { error: reason }),
): () => void {
  const handler = (e: PromiseRejectionEvent) => sink(e.reason);
  window.addEventListener("unhandledrejection", handler);
  return () => window.removeEventListener("unhandledrejection", handler);
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd web && npx vitest run src/app/globalErrorHandler.test.ts`
Expected: PASS (3 tests).

- [ ] **Step 5: Wire it into the app entry**

In `web/src/main.tsx`, add the import after the existing imports and call it before `ReactDOM.createRoot(...)`:

```ts
import { installGlobalRejectionHandler } from "./app/globalErrorHandler";
```

```ts
installGlobalRejectionHandler();

ReactDOM.createRoot(document.getElementById("root")!).render(
```

- [ ] **Step 6: Run lint + the test file**

Run: `cd web && npx eslint src/app/globalErrorHandler.ts src/main.tsx && npx vitest run src/app/globalErrorHandler.test.ts`
Expected: no eslint errors; tests PASS.

- [ ] **Step 7: Commit**

```bash
git add web/src/app/globalErrorHandler.ts web/src/app/globalErrorHandler.test.ts web/src/main.tsx
git commit -m "feat(resilience): global unhandledrejection handler (DG4)"
```

---

## Task 2: Dev-flag refresh tracer (DG5)

**Files:**
- Create: `web/src/store/trace.ts`
- Create: `web/src/store/trace.test.ts`

- [ ] **Step 1: Write the failing test**

Create `web/src/store/trace.test.ts`:

```ts
import { describe, it, expect, vi } from "vitest";
import { noopTrace, makeConsoleTrace } from "./trace";

describe("noopTrace", () => {
  it("event is a no-op and time just runs the thunk", async () => {
    expect(noopTrace.event("note_changed", ["refreshNotePaths"])).toBeUndefined();
    const out = await noopTrace.time("loadTags", async () => 42);
    expect(out).toBe(42);
  });
});

describe("makeConsoleTrace", () => {
  it("logs the event type and dispatched actions", () => {
    const spy = vi.spyOn(console, "debug").mockImplementation(() => {});
    makeConsoleTrace().event("note_changed", ["refreshNotePaths", "loadTags"]);
    expect(spy).toHaveBeenCalledWith(
      "[cairn] refresh ← note_changed",
      ["refreshNotePaths", "loadTags"],
    );
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd web && npx vitest run src/store/trace.test.ts`
Expected: FAIL — `Cannot find module './trace'`.

- [ ] **Step 3: Write minimal implementation**

Create `web/src/store/trace.ts`:

```ts
/**
 * Observability seam for the index-refresh cascade. The subscribe handler fans
 * queries out on every `note_changed`; in dev we want to see which trigger
 * dispatched which actions and how long the backend calls took, so a refresh
 * storm (D1/D2) is visible. In production this is a no-op — no logging ships.
 */
export interface RefreshTrace {
  /** Record a refresh trigger: event type -> the actions it dispatched. */
  event(type: string, actions: string[]): void;
  /** Time an async backend call, returning its result unchanged. */
  time<T>(label: string, fn: () => Promise<T>): Promise<T>;
}

/** Production / test default: records nothing, just runs the thunk. */
export const noopTrace: RefreshTrace = {
  event: () => {},
  time: (_label, fn) => fn(),
};

/** Dev tracer: logs the fan-out and times each backend call via `console.debug`. */
export function makeConsoleTrace(): RefreshTrace {
  return {
    event: (type, actions) =>
      console.debug(`[cairn] refresh ← ${type}`, actions),
    time: async (label, fn) => {
      const t0 = performance.now();
      try {
        return await fn();
      } finally {
        console.debug(
          `[cairn] ${label} took ${(performance.now() - t0).toFixed(1)}ms`,
        );
      }
    },
  };
}

/** The singleton the store uses by default: dev tracer in dev, no-op in prod. */
export const refreshTrace: RefreshTrace = import.meta.env.DEV
  ? makeConsoleTrace()
  : noopTrace;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd web && npx vitest run src/store/trace.test.ts`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add web/src/store/trace.ts web/src/store/trace.test.ts
git commit -m "feat(observability): dev-flag refresh tracer (DG5)"
```

---

## Task 3: Thread the tracer through the store event handler (DG5)

**Files:**
- Modify: `web/src/store/store.ts:160-163` (signature), `:266-298` (`onEvent`)

- [ ] **Step 1: Add the tracer import**

In `web/src/store/store.ts`, add to the imports block (near the other local imports):

```ts
import { type RefreshTrace, refreshTrace } from "./trace";
```

- [ ] **Step 2: Accept the tracer as an injected param**

Change the `createCairnStore` signature (currently `client, host = alwaysOpenHost`) to add a third defaulted param:

```ts
export function createCairnStore(
  client: CairnClient,
  host: CairnHost = alwaysOpenHost,
  trace: RefreshTrace = refreshTrace,
): StoreApi<CairnState> {
```

- [ ] **Step 3: Trace the fan-out in `onEvent`**

Replace the body of the `if (e.type === "note_changed" || e.type === "note_deleted")` branch in `onEvent` (the block that currently does `void get().refreshNotePaths()` etc., store.ts:285-294) with a `fire` helper that records the action name and times it. The `selfWrite` detection above it is unchanged. New block:

```ts
        // Dispatch + observe the refresh fan-out. `fire` records each action
        // name (for the dev trace's event log) and times the backend call;
        // behavior is identical to the old fire-and-forget `void` dispatch.
        const dispatched: string[] = [];
        const fire = (name: string, run: () => Promise<void>) => {
          dispatched.push(name);
          void trace.time(name, run);
        };
        if (!selfWrite) {
          fire("refreshNotePaths", () => get().refreshNotePaths());
          fire("loadTags", () => get().loadTags());
          if (get().graph !== null) fire("loadGraph", () => get().loadGraph());
        }
        const tag = get().activeTag;
        if (tag) fire("filterByTag", () => get().filterByTag(tag));
        else if (get().searchResults !== null)
          fire("runSearch", () => get().runSearch(get().query));
        if (get().activePath)
          fire("refreshBacklinks", () => get().refreshBacklinks());
        trace.event(e.type, dispatched);
```

> Note: this preserves the exact set of actions and their fire-and-forget
> dispatch (`void`). Do NOT change which actions run or add awaits — DG5 only
> *observes* the cascade; the D1/D2 fixes are out of scope.

- [ ] **Step 4: Run the existing store tests to confirm no behavior change**

Run: `cd web && npx vitest run src/store/store.test.ts`
Expected: PASS (all existing tests green — the default `noopTrace` keeps behavior identical).

- [ ] **Step 5: Typecheck + lint**

Run: `cd web && npx tsc --noEmit && npx eslint src/store/store.ts`
Expected: no errors.

- [ ] **Step 6: Commit**

```bash
git add web/src/store/store.ts
git commit -m "feat(observability): trace refresh fan-out via injected tracer (DG5)"
```

---

## Task 4: Store↔subscribe integration test (DX4 + DG5 guard)

**Files:**
- Modify: `web/src/store/store.test.ts`

- [ ] **Step 1: Write the failing integration test**

Append inside the `describe("cairn store", ...)` block in `web/src/store/store.test.ts`. It drives a real external `note_changed` through the subscribe loop and asserts the injected tracer recorded the full fan-out:

```ts
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
    await vi.waitFor(() =>
      expect(events.some((e) => e.type === "note_changed")).toBe(true),
    );
    const echo = events.find((e) => e.type === "note_changed")!;
    expect(echo.actions).not.toContain("refreshNotePaths");
    expect(echo.actions).not.toContain("loadTags");
  });
```

- [ ] **Step 2: Run to verify both pass against the Task-3 implementation**

Run: `cd web && npx vitest run src/store/store.test.ts`
Expected: PASS — the new tests and all existing ones.

> If the self-write test is flaky on timing, confirm `DEFAULT_SETTINGS.autosaveMs`
> elapses under real timers via `vi.waitFor` (it polls up to its default 1s
> timeout; autosave is 1000ms). If needed, raise the `vi.waitFor` timeout to
> `{ timeout: 3000 }` rather than switching to fake timers (fake timers + the
> microtask-based emit deadlock, per the existing tests' comments).

- [ ] **Step 3: Commit**

```bash
git add web/src/store/store.test.ts
git commit -m "test(store): integration coverage for the subscribe fan-out (DX4)"
```

---

## Task 5: Focus guard on the global keydown (U4)

**Files:**
- Modify: `web/src/components/shortcuts/useGlobalKeys.ts`
- Modify: `web/src/components/shortcuts/useGlobalKeys.test.ts`

- [ ] **Step 1: Write the failing tests**

Append to `web/src/components/shortcuts/useGlobalKeys.test.ts` (inside the existing `describe`). Note `eventToChord` is already imported there.

```ts
  it("skips a non-allowlisted chord when focus is in a text input", () => {
    const run = vi.fn();
    const chord = eventToChord(
      new KeyboardEvent("keydown", { key: "e", ctrlKey: true }),
    )!;
    renderHook(() => useGlobalKeys({ [chord]: "toggle-editor-mode" }, run));
    const input = document.createElement("input");
    document.body.appendChild(input);
    input.dispatchEvent(
      new KeyboardEvent("keydown", { key: "e", ctrlKey: true, bubbles: true }),
    );
    expect(run).not.toHaveBeenCalled();
    input.remove();
  });

  it("still fires the palette/commit allowlist from inside an input", () => {
    const run = vi.fn();
    const chord = eventToChord(
      new KeyboardEvent("keydown", { key: "k", ctrlKey: true }),
    )!;
    renderHook(() => useGlobalKeys({ [chord]: "open-palette" }, run));
    const input = document.createElement("input");
    document.body.appendChild(input);
    input.dispatchEvent(
      new KeyboardEvent("keydown", { key: "k", ctrlKey: true, bubbles: true }),
    );
    expect(run).toHaveBeenCalledWith("open-palette");
    input.remove();
  });

  it("skips a chord when focus is inside CodeMirror", () => {
    const run = vi.fn();
    const chord = eventToChord(
      new KeyboardEvent("keydown", { key: "e", ctrlKey: true }),
    )!;
    renderHook(() => useGlobalKeys({ [chord]: "toggle-editor-mode" }, run));
    const cm = document.createElement("div");
    cm.className = "cm-editor";
    const inner = document.createElement("div");
    cm.appendChild(inner);
    document.body.appendChild(cm);
    inner.dispatchEvent(
      new KeyboardEvent("keydown", { key: "e", ctrlKey: true, bubbles: true }),
    );
    expect(run).not.toHaveBeenCalled();
    cm.remove();
  });

  it("does NOT fire tab navigation (Mod+1) from inside an input", () => {
    const run = vi.fn();
    const jump = vi.spyOn(cairnStore.getState(), "jumpToTab");
    renderHook(() => useGlobalKeys({}, run));
    const input = document.createElement("input");
    document.body.appendChild(input);
    input.dispatchEvent(
      new KeyboardEvent("keydown", { key: "1", ctrlKey: true, bubbles: true }),
    );
    expect(jump).not.toHaveBeenCalled();
    input.remove();
    jump.mockRestore();
  });
```

Add to the imports at the top of the test file:

```ts
import { cairnStore } from "../../app/cairnStore";
```

- [ ] **Step 2: Run to verify the new tests fail**

Run: `cd web && npx vitest run src/components/shortcuts/useGlobalKeys.test.ts`
Expected: FAIL — the skip tests fail because the current handler fires regardless of focus (`run`/`jumpToTab` called).

- [ ] **Step 3: Implement the focus guard**

Edit `web/src/components/shortcuts/useGlobalKeys.ts`. Add, above the `useGlobalKeys` function, the editable-target predicate and the allowlist:

```ts
/** Commands that must still fire even while focus is in an editable target —
 *  the global affordances a user reaches for mid-edit. */
const ALLOW_IN_EDITABLE = new Set(["open-palette", "commit"]);

/** True when a keydown target is somewhere typing/selection should win over a
 *  global chord: a form control, a contentEditable region, inside CodeMirror,
 *  or inside an open dialog. */
function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  const tag = target.tagName;
  if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return true;
  if (target.isContentEditable) return true;
  if (target.closest(".cm-editor")) return true; // CodeMirror
  if (target.closest('[role="dialog"]')) return true; // open dialog
  return false;
}
```

Then, inside the `onKey` handler, gate the dispatch. Replace the existing handler body:

```ts
    const onKey = (e: KeyboardEvent) => {
      const editable = isEditableTarget(e.target);
      const chord = eventToChord(e);
      const id = chord ? chordMap[chord] : undefined;
      if (id) {
        // In an editable target, only the allowlist (palette/commit) fires; a
        // bare Mod+E / Mod+W must not steal focus mid-edit.
        if (editable && !ALLOW_IN_EDITABLE.has(id)) return;
        e.preventDefault();
        runCommandRef.current(id);
        return;
      }
      // Built-in tab navigation (parameterized; not rebindable). Also suppressed
      // in an editable target so Mod+1..9 doesn't fire while typing in search.
      if (editable) return;
      const st = cairnStore.getState();
      if (e.ctrlKey && e.key === "Tab") {
        e.preventDefault();
        st.cycleTab(e.shiftKey ? -1 : 1);
      } else if ((e.metaKey || e.ctrlKey) && /^[1-9]$/.test(e.key)) {
        e.preventDefault();
        st.jumpToTab(Number(e.key));
      }
    };
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd web && npx vitest run src/components/shortcuts/useGlobalKeys.test.ts`
Expected: PASS — original 2 tests + 4 new ones. (The first original test dispatches on `window`, whose `target` is `window`, not an HTMLElement → not editable → still fires. Good.)

- [ ] **Step 5: Typecheck + lint**

Run: `cd web && npx tsc --noEmit && npx eslint src/components/shortcuts/useGlobalKeys.ts`
Expected: no errors.

- [ ] **Step 6: Commit**

```bash
git add web/src/components/shortcuts/useGlobalKeys.ts web/src/components/shortcuts/useGlobalKeys.test.ts
git commit -m "fix(shortcuts): guard global keydown on focus target (U4)"
```

---

## Task 6: Thin typed wrapper for react-force-graph (D9)

**Files:**
- Create: `web/src/components/graph/forceGraphTypes.ts`
- Create: `web/src/components/graph/forceGraphTypes.test.ts`
- Modify: `web/src/components/GraphView.tsx`

- [ ] **Step 1: Write the failing test**

Create `web/src/components/graph/forceGraphTypes.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import { asGraphData } from "./forceGraphTypes";
import { buildGraphData } from "./graphData";

describe("asGraphData", () => {
  it("round-trips the build output shape (same ids + link count)", () => {
    const built = buildGraphData(
      ["a.md", "b.md"],
      [{ from: "a.md", to: "b.md" }],
    );
    const rf = asGraphData(built);
    expect(rf.nodes.map((n) => n.id)).toEqual(["a.md", "b.md"]);
    expect(rf.links).toHaveLength(1);
  });

  it("is a typed view, not a copy (the lib mutates nodes/links in place)", () => {
    const built = buildGraphData(["a.md"], []);
    const rf = asGraphData(built);
    expect(rf).toBe(built);
    expect(rf.nodes).toBe(built.nodes);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd web && npx vitest run src/components/graph/forceGraphTypes.test.ts`
Expected: FAIL — `Cannot find module './forceGraphTypes'`.

- [ ] **Step 3: Write the wrapper**

Create `web/src/components/graph/forceGraphTypes.ts`:

```ts
import type { ForceGraphMethods } from "react-force-graph-2d";
import type { GLink, GNode } from "./graphData";

/**
 * react-force-graph mutates node objects in place (adds x/y/vx/vy) and rewrites
 * link.source/target from id strings into node references at runtime. RFNode is
 * the post-mutation node shape the canvas painters read. Centralizing it (and
 * the casts below) here means a lib upgrade breaks in one place, not silently.
 */
export interface RFNode {
  id: string;
  label: string;
  degree: number;
  x?: number;
  y?: number;
  fx?: number; // d3 pin (set to freeze, undefined to release)
  fy?: number;
}

/** The graphData prop shape ForceGraph2D consumes (and mutates in place). */
export interface RFGraphData {
  nodes: RFNode[];
  links: GLink[];
}

/** The imperative ForceGraph2D handle, typed to our node/link shapes. */
export type FG = ForceGraphMethods<RFNode, GLink>;

/** The subset of the d3 link force we configure. react-force-graph creates the
 *  force untyped; this interface is the single place that asserts its shape. */
export interface LinkForce {
  strength: (n: number) => unknown;
  distance: (n: number) => unknown;
}

/** Adapt a string-keyed build into the shape ForceGraph2D consumes. The lib
 *  mutates these arrays in place, so this is a typed view, not a copy. */
export function asGraphData(data: {
  nodes: GNode[];
  links: GLink[];
}): RFGraphData {
  return data as RFGraphData;
}

/** The d3 link force, typed. Returns undefined before the simulation exists. */
export function linkForce(fg: FG): LinkForce | undefined {
  return fg.d3Force("link") as LinkForce | undefined;
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd web && npx vitest run src/components/graph/forceGraphTypes.test.ts`
Expected: PASS (2 tests).

- [ ] **Step 5: Adopt the wrapper in GraphView**

Edit `web/src/components/GraphView.tsx`:

(a) Replace the `react-force-graph-2d` import line and the local `RFNode` interface. The current top imports `ForceGraph2D, { type ForceGraphMethods }` and defines `interface RFNode {...}` at lines 33-44. Change the import to drop `ForceGraphMethods`:

```ts
import ForceGraph2D from "react-force-graph-2d";
```

Delete the entire local `RFNode` interface (the comment block + `interface RFNode { ... }`, lines ~34-44) and instead import the wrapper. Add to the graph-module imports:

```ts
import {
  type RFNode,
  type FG,
  asGraphData,
  linkForce,
} from "./graph/forceGraphTypes";
```

Also drop the now-unused `type GLink` from the `./graph/graphData` import (it was only used in the `ForceGraphMethods<RFNode, GLink>` ref and the `graphData` cast). The graphData import becomes:

```ts
import {
  buildGraphData,
  buildAdjacency,
  nodeRadius,
  labelAlpha,
} from "./graph/graphData";
```

(b) Change the `fgRef` type (currently `useRef<ForceGraphMethods<RFNode, GLink>>`):

```ts
  const fgRef = useRef<FG | undefined>(undefined);
```

(c) Add a typed view right after `const data = localData ?? globalData;` (and the adjacency line):

```ts
  const data = localData ?? globalData;
  const adjacency = localAdj ?? globalAdj;
  const rfData = asGraphData(data);
```

(d) In the force-settings effect, replace the inline `d3Force("link") as {...}` cast with the helper:

```ts
    fg.d3Force("charge")?.strength(forces.repel);
    const link = linkForce(fg);
    link?.strength(forces.linkForce);
    link?.distance(forces.linkDistance);
    fg.d3Force("center")?.strength(forces.center);
```

(e) In the same effect, replace the freeze loop's `data.nodes as RFNode[]` cast with `rfData.nodes`:

```ts
    for (const n of rfData.nodes) {
```

(f) In the JSX, replace `graphData={data as { nodes: RFNode[]; links: GLink[] }}` with the typed view:

```ts
            graphData={rfData}
```

- [ ] **Step 6: Typecheck, lint, and run the GraphView test**

Run: `cd web && npx tsc --noEmit && npx eslint src/components/GraphView.tsx src/components/graph/forceGraphTypes.ts && npx vitest run src/components/GraphView.test.tsx src/components/graph/forceGraphTypes.test.ts`
Expected: no type/lint errors; tests PASS. (`tsc` is the real guard here — the wrapper must satisfy ForceGraph2D's prop types with zero `as` casts left in GraphView.)

- [ ] **Step 7: Commit**

```bash
git add web/src/components/graph/forceGraphTypes.ts web/src/components/graph/forceGraphTypes.test.ts web/src/components/GraphView.tsx
git commit -m "refactor(graph): localize react-force-graph casts in a typed wrapper (D9)"
```

---

## Task 7: Full verification (verification-before-completion)

**Files:** none (verification only).

- [ ] **Step 1: Full test suite with captured output**

Run: `cd web && npm test`
Expected: all suites PASS, including the new files. Capture the real summary line.

- [ ] **Step 2: Typecheck**

Run: `cd web && npm run typecheck`
Expected: no errors.

- [ ] **Step 3: Lint**

Run: `cd web && npm run lint`
Expected: no errors.

- [ ] **Step 4: Format check (easy to miss — eslint won't catch it)**

Run: `cd web && npm run format:check`
Expected: "All matched files use Prettier code style!" If it fails, run `npm run format` and re-commit the formatting.

- [ ] **Step 5: Build**

Run: `cd web && npm run build`
Expected: `tsc -b` + `vite build` succeed. This also confirms the dev tracer is stripped under a production build (`import.meta.env.DEV` is false).

- [ ] **Step 6: Run the repo-level gate if present**

Run (from repo root): `just web-check` or the documented local gate, if one exists; otherwise the four `web/` commands above are the gate.
Expected: green.

---

## Task 8: Review, PR (requesting-code-review, then STOP)

- [ ] **Step 1: Self-review the diff for scope creep**

Run: `git diff main...HEAD --stat` and read the full diff. Confirm: only the 11 files in the File Structure changed; no D1/D2 race/storm fix sneaked in; no keymap rewrite; no unrelated refactor. The U4 change is the smallest possible change to the dispatch condition.

- [ ] **Step 2: Invoke requesting-code-review** on the branch diff; address any findings (use receiving-code-review for judgment).

- [ ] **Step 3: Push and open the PR (do NOT merge)**

```bash
git push -u origin cairn-ui-observability-resilience
gh pr create -R tau-rs/cairn-web-ui --base main \
  --title "Observability & resilience polish (DG4, DG5, U4, D9, DX4)" \
  --body "<cite all five findings; note: dev-flag tracing only, no prod logging; no D1/D2 fix; U4 is a minimal dispatch-condition guard>"
```

Each commit footer (and the PR body) uses:
`Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`

STOP after the PR is opened — no merge.

---

## Self-Review (plan vs. spec)

**Spec coverage:**
- DG4 (global unhandledrejection handler) → Task 1. ✓
- DG5 (dev-flag trace of refresh trigger→actions + timing) → Tasks 2 + 3. ✓
- U4 (focus guard on global keydown, palette/commit allowlist) → Task 5. ✓ (handler now lives in `useGlobalKeys`, not `App.tsx:50-71` as the brief's stale line ref says — App decomposition already landed.)
- D9 (thin typed wrapper around react-force-graph data/`d3Force`) → Task 6. ✓
- DX4 (integration test at store↔subscribe seam, doubling as DG5 guard) → Task 4. ✓

**Constraints check:**
- One PR for the cluster. ✓ (Task 8)
- DG5 tracing behind a dev flag, no prod logging → `refreshTrace = import.meta.env.DEV ? makeConsoleTrace() : noopTrace`; verified by the build step. ✓
- U4 lands after session 64 (App decomposition) — already merged; the guard is the minimal dispatch-condition change, not a keymap rewrite. ✓
- Do NOT fix D1/D2 — Tasks 3/4 only observe + test; the fire-and-forget dispatch set is unchanged. ✓
- Match existing store/component/test style — tracer injected like `host`; tests mirror the existing mock-client + `vi.waitFor` real-timer pattern. ✓

**Placeholder scan:** none — every code step shows full content. ✓

**Type consistency:** `RefreshTrace.event(type, actions)` / `.time(label, fn)` used identically in trace.ts, store.ts, and both tests. `asGraphData` / `linkForce` / `RFNode` / `FG` names consistent across forceGraphTypes.ts and GraphView.tsx. ✓
