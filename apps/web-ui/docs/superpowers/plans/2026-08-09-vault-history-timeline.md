# Vault-history Timeline Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the graph's temporal scrubber a legible, vault-wide "jump to a point in time & see what changed" tool.

**Architecture:** Swap the scrubber's *source* from one note's history (`note_history`) to the whole vault (`vault_history` — already in the contract + engine `057bf5e`), and redesign the scrubber UI for legibility (state banner, explicit Browse/Compare modes, activity-density histogram). The reconstruction path (`graph_at`/`graph_diff`/`buildCompareGraphData`) is reused unchanged. Pure UI — no engine work.

**Tech Stack:** React + TypeScript, Zustand store, Vitest + @testing-library/react, Tailwind. Runner: `pnpm` via `just`.

## Global Constraints

- **No engine changes, no contract edits.** `web/src/contract/*` is generated ts-rs — never hand-author. `vault_history`, `graph_at`, `graph_diff` already exist.
- **Reuse unchanged:** `buildCompareGraphData` (graphData.ts), `selectionToRequest`/`TemporalSelection`/`TemporalRequest` (temporalControls.ts), `debounce` (util/timer.ts), the store's `loadSnapshot`/`loadDiff`/`clearTemporal` and `seq.timeline`/`seq.temporalData` token guards.
- **Timeline is newest-first** (`Revision[]`, index 0 = newest). Display renders oldest→newest (left→right); convert with `tlIdx = n - 1 - displayIdx`.
- **Scope:** vault-only timeline (per-note temporal timeline is intentionally dropped — see spec "Core decision"). Structural-marker filtering is Phase 2, out of scope.
- **Gate:** full `just web-ci` including `prettier --check`. PR `--base main`, merge queue only.
- Run every command from the repo root `/Users/titouanlebocq/conductor/workspaces/cairn-ui/astana`. Web tests: `pnpm -C web test`.

---

### Task 1: Vault-timeline source — mock support + `loadVaultTimeline` store action

**Files:**
- Modify: `web/src/client/mock.ts` (constructor + `runQuery` switch)
- Modify: `web/src/store/store.ts` (interface near line 239; impl near `loadTimeline` at ~1110)
- Test: `web/src/store/store.test.ts` (`setup()` helper ~line 31; `temporal graph` describe ~line 1210)

**Interfaces:**
- Consumes: `client.runQuery`, `seq.timeline`, `set`, `unexpected`, `pushError` (existing in store.ts).
- Produces:
  - `MockClient` constructor gains a 4th param `vaultRevisions: Revision[] = []` and answers `{type:"vault_history"}` with `{type:"history", revisions: vaultRevisions}`.
  - `setup()` test helper accepts `opts.vaultRevisions`.
  - Store action `loadVaultTimeline(): Promise<void>` — runs `{type:"vault_history", limit:null}`, writes `temporal.timeline` on a `history` response.

- [ ] **Step 1: Write the failing test** (append inside the `temporal graph` describe in `store.test.ts`)

```ts
it("loadVaultTimeline populates the timeline from vault_history", async () => {
  const vaultRevisions = [
    { id: "r3", message: "c", timestamp_secs: 30n, author: "x" },
    { id: "r2", message: "b", timestamp_secs: 20n, author: "x" },
    { id: "r1", message: "a", timestamp_secs: 10n, author: "x" },
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

it("loadVaultTimeline ignores a stale response (token guard)", async () => {
  const { store } = setup({
    vaultRevisions: [{ id: "r1", message: "a", timestamp_secs: 10n, author: "x" }],
  });
  await store.getState().init();
  const p1 = store.getState().loadVaultTimeline();
  const p2 = store.getState().loadVaultTimeline();
  await Promise.all([p1, p2]);
  expect(store.getState().temporal.timeline?.map((r) => r.id)).toEqual(["r1"]);
});
```

Also update the `setup()` helper to thread the new option:

```ts
function setup(
  opts: {
    history?: Record<string, HistoryFixture>;
    vaultSnapshots?: Record<string, VaultSnapshot>;
    vaultRevisions?: Revision[];
  } = {},
) {
  const client = new MockClient(
    { "a.md": "links to [[b]]", "b.md": "target note" },
    opts.history ?? {},
    opts.vaultSnapshots ?? {},
    opts.vaultRevisions ?? [],
  );
  const store = createCairnStore(client);
  return { client, store };
}
```

Add `Revision` to the store.test.ts contract import: `import type { Query, QueryResponse, SuggestedEdge, Revision } from "../contract";`

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm -C web test -- store.test.ts -t "loadVaultTimeline"`
Expected: FAIL — `loadVaultTimeline is not a function` (and MockClient ctor arity / unhandled `vault_history`).

- [ ] **Step 3: Add mock support in `web/src/client/mock.ts`**

Add the constructor param (append after the existing `vaultSnapshots` param) and store it:

```ts
    vaultSnapshots: Record<string, VaultSnapshot> = {},
    vaultRevisions: Revision[] = [],
  ) {
    // ...existing assignments...
    this.vaultRevisions = vaultRevisions;
```

Add the field declaration next to the other private fields (near `private history`):

```ts
  private vaultRevisions: Revision[];
```

Add the case in the `runQuery` switch (next to `note_history`, ~line 484):

```ts
      case "vault_history":
        return { type: "history", revisions: this.vaultRevisions };
```

Ensure `Revision` is imported in mock.ts (add to the existing `../contract` type import if absent).

- [ ] **Step 4: Add the store action in `web/src/store/store.ts`**

Interface (add next to `loadTimeline` at ~line 239):

```ts
  loadTimeline(path: string): Promise<void>;
  loadVaultTimeline(): Promise<void>;
```

Implementation (add immediately after `loadTimeline`'s closing brace at ~line 1123):

```ts
      async loadVaultTimeline() {
        const token = ++seq.timeline;
        try {
          const res = await client.runQuery({ type: "vault_history", limit: null });
          if (token !== seq.timeline) return;
          if (res.type === "history")
            set((s) => ({
              temporal: { ...s.temporal, timeline: res.revisions },
            }));
          else unexpected("Load vault timeline", res);
        } catch (err) {
          if (token === seq.timeline) pushError("Load vault timeline", err);
        }
      },
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `pnpm -C web test -- store.test.ts -t "loadVaultTimeline"`
Expected: PASS (both tests).

- [ ] **Step 6: Commit**

```bash
git add web/src/client/mock.ts web/src/store/store.ts web/src/store/store.test.ts
git commit -m "feat(graph): add loadVaultTimeline store action + mock vault_history"
```

---

### Task 2: Pure timeline helpers (density histogram + banner text)

**Files:**
- Create: `web/src/components/graph/timelineDensity.ts`
- Test: `web/src/components/graph/timelineDensity.test.ts`

**Interfaces:**
- Consumes: `Revision` (contract), `TemporalSelection` (temporalControls.ts).
- Produces:
  - `timelineBuckets(revisions: Revision[], bucketCount?: number): { count: number }[]` — time-proportional histogram; `[]` for empty input.
  - `describeSelection(selection: TemporalSelection, timeline: Revision[]): { state: string; detail: string }` — banner text (no counts); falls back to Live when indices are out of range.

- [ ] **Step 1: Write the failing test** (`timelineDensity.test.ts`)

```ts
import { describe, it, expect } from "vitest";
import { timelineBuckets, describeSelection } from "./timelineDensity";
import type { Revision } from "../../contract";

const rev = (id: string, t: bigint, msg = id): Revision => ({
  id,
  message: msg,
  timestamp_secs: t,
  author: "x",
});

describe("timelineBuckets", () => {
  it("returns [] for an empty timeline", () => {
    expect(timelineBuckets([])).toEqual([]);
  });

  it("distributes revisions across buckets by timestamp; counts sum to N", () => {
    const revs = [rev("a", 0n), rev("b", 50n), rev("c", 100n)];
    const buckets = timelineBuckets(revs, 10);
    expect(buckets).toHaveLength(10);
    expect(buckets.reduce((s, b) => s + b.count, 0)).toBe(3);
    expect(buckets[0].count).toBe(1); // oldest
    expect(buckets[9].count).toBe(1); // newest lands in the last bucket
  });

  it("puts everything in bucket 0 when all timestamps are equal (no div-by-zero)", () => {
    const revs = [rev("a", 5n), rev("b", 5n)];
    const buckets = timelineBuckets(revs, 4);
    expect(buckets).toHaveLength(4);
    expect(buckets[0].count).toBe(2);
  });
});

describe("describeSelection", () => {
  const tl = [rev("r2", 20n, "add b"), rev("r1", 10n, "init")]; // newest-first

  it("describes live", () => {
    expect(describeSelection({ kind: "live" }, tl)).toEqual({
      state: "Live",
      detail: "current vault",
    });
  });

  it("describes a snapshot with date + message", () => {
    const d = describeSelection({ kind: "snapshot", at: 1 }, tl); // r1
    expect(d.state).toBe("Viewing vault as of");
    expect(d.detail).toContain("init");
    expect(d.detail).toContain("1970-01-01");
  });

  it("describes a compare range", () => {
    const d = describeSelection({ kind: "compare", from: 1, to: 0 }, tl);
    expect(d.state).toBe("Comparing");
    expect(d.detail).toContain("→");
  });

  it("falls back to Live on an out-of-range index", () => {
    expect(describeSelection({ kind: "snapshot", at: 9 }, tl).state).toBe("Live");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm -C web test -- timelineDensity.test.ts`
Expected: FAIL — module not found.

- [ ] **Step 3: Create `web/src/components/graph/timelineDensity.ts`**

```ts
import type { Revision } from "../../contract";
import type { TemporalSelection } from "./temporalControls";

/** Time-proportional activity histogram: bucket `revisions` into `bucketCount`
 *  equal time intervals spanning the oldest→newest timestamp. Fixed length
 *  regardless of commit count, so it never overflows the track. */
export function timelineBuckets(
  revisions: Revision[],
  bucketCount = 12,
): { count: number }[] {
  if (revisions.length === 0) return [];
  const times = revisions.map((r) => Number(r.timestamp_secs));
  const min = Math.min(...times);
  const max = Math.max(...times);
  const span = max - min;
  const buckets = Array.from({ length: bucketCount }, () => ({ count: 0 }));
  for (const t of times) {
    const frac = span === 0 ? 0 : (t - min) / span;
    const idx = Math.min(bucketCount - 1, Math.floor(frac * bucketCount));
    buckets[idx].count += 1;
  }
  return buckets;
}

function fmtDate(secs: bigint): string {
  // UTC ISO date — deterministic across locales.
  return new Date(Number(secs) * 1000).toISOString().slice(0, 10);
}

/** Human-readable "where am I" text for the scrubber banner. Out-of-range
 *  indices degrade to Live (the safe default when scrubber and data desync). */
export function describeSelection(
  selection: TemporalSelection,
  timeline: Revision[],
): { state: string; detail: string } {
  const at = (i: number): Revision | null =>
    i >= 0 && i < timeline.length ? timeline[i] : null;
  const live = { state: "Live", detail: "current vault" };
  if (selection.kind === "live") return live;
  if (selection.kind === "snapshot") {
    const r = at(selection.at);
    return r
      ? { state: "Viewing vault as of", detail: `${fmtDate(r.timestamp_secs)} — ${r.message}` }
      : live;
  }
  const from = at(selection.from);
  const to = at(selection.to);
  return from && to
    ? { state: "Comparing", detail: `${fmtDate(from.timestamp_secs)} → ${fmtDate(to.timestamp_secs)}` }
    : live;
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm -C web test -- timelineDensity.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/graph/timelineDensity.ts web/src/components/graph/timelineDensity.test.ts
git commit -m "feat(graph): pure timeline density + banner-text helpers"
```

---

### Task 3: Redesign `TemporalScrubber` — banner, Browse/Compare, histogram

**Files:**
- Modify: `web/src/components/graph/TemporalScrubber.tsx` (full redesign)
- Test: `web/src/components/graph/TemporalScrubber.test.tsx` (rewrite)

**Interfaces:**
- Consumes: `timelineBuckets`, `describeSelection` (Task 2); `TemporalSelection` (temporalControls.ts).
- Produces: new props contract consumed by Task 4:
  ```ts
  TemporalScrubber(props: {
    timeline: Revision[];
    selection: TemporalSelection;
    onSelect: (s: TemporalSelection) => void;
    counts: { notes: number; links: number } | null;
    delta: { added: number; removed: number } | null;
  })
  ```
  Accessible controls: buttons "Live" / "Browse" / "Compare"; range inputs labelled "Timeline position" (browse), "Compare from" / "Compare to" (compare).

- [ ] **Step 1: Write the failing test** (`TemporalScrubber.test.tsx`, replace file contents)

```ts
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { TemporalScrubber } from "./TemporalScrubber";
import type { Revision } from "../../contract";

const tl: Revision[] = [
  { id: "c3", message: "third", timestamp_secs: 30n, author: "a" },
  { id: "c2", message: "second", timestamp_secs: 20n, author: "a" },
  { id: "c1", message: "first", timestamp_secs: 10n, author: "a" },
];

function renderScrubber(overrides = {}) {
  const props = {
    timeline: tl,
    selection: { kind: "live" } as const,
    onSelect: vi.fn(),
    counts: { notes: 3, links: 2 },
    delta: null,
    ...overrides,
  };
  render(<TemporalScrubber {...props} />);
  return props;
}

describe("TemporalScrubber", () => {
  it("renders Live, Browse and Compare controls", () => {
    renderScrubber();
    expect(screen.getByRole("button", { name: /live/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^browse$/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^compare$/i })).toBeInTheDocument();
  });

  it("Browse: moving to the leftmost position selects the oldest snapshot", () => {
    const { onSelect } = renderScrubber();
    // leftmost display index 0 = oldest (c1) → timeline index 2
    fireEvent.change(screen.getByLabelText(/timeline position/i), {
      target: { value: "0" },
    });
    expect(onSelect).toHaveBeenCalledWith({ kind: "snapshot", at: 2 });
  });

  it("Compare: setting from=oldest and to=newest emits a compare selection", async () => {
    const { onSelect } = renderScrubber();
    await userEvent.click(screen.getByRole("button", { name: /^compare$/i }));
    fireEvent.change(screen.getByLabelText(/compare from/i), {
      target: { value: "0" }, // oldest → tl index 2
    });
    fireEvent.change(screen.getByLabelText(/compare to/i), {
      target: { value: "2" }, // newest → tl index 0
    });
    expect(onSelect).toHaveBeenLastCalledWith({
      kind: "compare",
      from: 2,
      to: 0,
    });
  });

  it("banner shows the snapshot date + message for a snapshot selection", () => {
    renderScrubber({ selection: { kind: "snapshot", at: 2 } }); // c1 "first"
    expect(screen.getByText(/viewing vault as of/i)).toBeInTheDocument();
    expect(screen.getByText(/first/)).toBeInTheDocument();
  });

  it("renders a long timeline without a per-revision DOM blowup", () => {
    const long: Revision[] = Array.from({ length: 120 }, (_, i) => ({
      id: `r${i}`,
      message: `m${i}`,
      timestamp_secs: BigInt(i),
      author: "a",
    }));
    renderScrubber({ timeline: long });
    // histogram is fixed-bucket; no 120 buttons
    expect(screen.getAllByRole("button").length).toBeLessThan(20);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm -C web test -- TemporalScrubber.test.tsx`
Expected: FAIL — old component has no Browse/Compare/counts.

- [ ] **Step 3: Rewrite `web/src/components/graph/TemporalScrubber.tsx`**

```tsx
import { useMemo, useState } from "react";
import type { Revision } from "../../contract";
import type { TemporalSelection } from "./temporalControls";
import { timelineBuckets, describeSelection } from "./timelineDensity";

/** Vault-history scrubber. `timeline` is newest-first; the UI renders
 *  oldest→newest (display index 0 = oldest = left). Two explicit modes:
 *  Browse (jump to a point) and Compare (diff two points). */
export function TemporalScrubber(props: {
  timeline: Revision[];
  selection: TemporalSelection;
  onSelect: (s: TemporalSelection) => void;
  counts: { notes: number; links: number } | null;
  delta: { added: number; removed: number } | null;
}) {
  const { timeline, selection, onSelect, counts, delta } = props;
  const n = timeline.length;
  const [mode, setMode] = useState<"browse" | "compare">(
    selection.kind === "compare" ? "compare" : "browse",
  );
  // compare endpoints as DISPLAY indices (0 = oldest); default full span.
  const [cmp, setCmp] = useState<{ from: number; to: number }>({
    from: 0,
    to: Math.max(0, n - 1),
  });

  const buckets = useMemo(() => timelineBuckets(timeline), [timeline]);
  const maxBar = Math.max(1, ...buckets.map((b) => b.count));
  const { state, detail } = describeSelection(selection, timeline);

  const toTl = (d: number) => n - 1 - d; // display idx → newest-first idx

  const browseDisplay =
    selection.kind === "snapshot" ? n - 1 - selection.at : n - 1;

  const setLive = () => {
    setMode("browse");
    onSelect({ kind: "live" });
  };
  const onBrowse = (displayIdx: number) =>
    onSelect({ kind: "snapshot", at: toTl(displayIdx) });

  const emitCompare = (fromD: number, toD: number) => {
    const from = Math.max(toTl(fromD), toTl(toD)); // older = higher tl index
    const to = Math.min(toTl(fromD), toTl(toD));
    if (from === to) onSelect({ kind: "snapshot", at: to });
    else onSelect({ kind: "compare", from, to });
  };
  const enterCompare = () => {
    setMode("compare");
    emitCompare(cmp.from, cmp.to);
  };

  const segBtn = (active: boolean) =>
    "rounded px-2 py-0.5 text-[11px] " +
    (active ? "bg-accent text-accent-fg" : "text-muted hover:text-text");

  return (
    <div className="pointer-events-auto absolute inset-x-2 bottom-2 z-10 flex flex-col gap-1.5 rounded-md border border-border bg-surface/90 px-2 py-1.5">
      {/* banner — "where am I" */}
      <div className="flex items-center gap-2 text-[11px]">
        <span className="font-semibold text-text">{state}</span>
        <span className="text-muted">{detail}</span>
        {counts && (
          <span className="ml-auto rounded-full border border-border px-2 py-0.5 text-muted">
            {counts.notes} notes · {counts.links} links
          </span>
        )}
        {delta && (
          <span className="text-[11px] text-muted">
            <span className="text-emerald-400">+{delta.added}</span>
            {" / "}
            <span className="text-rose-400">−{delta.removed}</span>
          </span>
        )}
      </div>

      {/* controls */}
      <div className="flex items-center gap-2">
        <div className="flex overflow-hidden rounded border border-border">
          <button type="button" className={segBtn(mode === "browse")} onClick={() => setMode("browse")}>
            Browse
          </button>
          <button type="button" className={segBtn(mode === "compare")} onClick={enterCompare}>
            Compare
          </button>
        </div>
        <button
          type="button"
          aria-pressed={selection.kind === "live"}
          className={segBtn(selection.kind === "live")}
          onClick={setLive}
        >
          Live
        </button>

        {/* histogram backdrop + range control(s) */}
        <div className="relative flex flex-1 flex-col justify-end">
          <div className="flex h-5 items-end gap-[2px]" aria-hidden="true">
            {buckets.map((b, i) => (
              <div
                key={i}
                className="flex-1 rounded-t-sm bg-border"
                style={{ height: `${8 + (b.count / maxBar) * 92}%` }}
              />
            ))}
          </div>
          {mode === "browse" ? (
            <input
              type="range"
              aria-label="Timeline position"
              className="w-full accent-accent"
              min={0}
              max={Math.max(0, n - 1)}
              step={1}
              value={browseDisplay}
              onChange={(e) => onBrowse(Number(e.target.value))}
            />
          ) : (
            <div className="flex gap-2">
              <input
                type="range"
                aria-label="Compare from"
                className="w-full accent-accent"
                min={0}
                max={Math.max(0, n - 1)}
                step={1}
                value={cmp.from}
                onChange={(e) => {
                  const from = Number(e.target.value);
                  setCmp((c) => ({ ...c, from }));
                  emitCompare(from, cmp.to);
                }}
              />
              <input
                type="range"
                aria-label="Compare to"
                className="w-full accent-accent"
                min={0}
                max={Math.max(0, n - 1)}
                step={1}
                value={cmp.to}
                onChange={(e) => {
                  const to = Number(e.target.value);
                  setCmp((c) => ({ ...c, to }));
                  emitCompare(cmp.from, to);
                }}
              />
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm -C web test -- TemporalScrubber.test.tsx`
Expected: PASS (all 5).

- [ ] **Step 5: Commit**

```bash
git add web/src/components/graph/TemporalScrubber.tsx web/src/components/graph/TemporalScrubber.test.tsx
git commit -m "feat(graph): redesign TemporalScrubber (banner + browse/compare + histogram)"
```

---

### Task 4: Rewire `useTemporalGraph` + `GraphView` to the vault source

**Files:**
- Modify: `web/src/components/graph/useTemporalGraph.ts`
- Modify: `web/src/components/GraphView.tsx` (call site ~line 86; history button ~531–540; scrubber mount ~631–637)
- Test: `web/src/components/graph/useTemporalGraph.test.tsx` (rewrite)

**Interfaces:**
- Consumes: `actions.loadVaultTimeline` (Task 1), redesigned `TemporalScrubber` props (Task 3), `debounce` (util/timer.ts), `selectionToRequest` (unchanged).
- Produces: `useTemporalGraph()` (no args) returning `{ timeline, selection, setSelection, open, setOpen, mode, source, diff }` — **`disabled` removed**.

- [ ] **Step 1: Write the failing test** (`useTemporalGraph.test.tsx`, replace file)

```ts
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { useTemporalGraph } from "./useTemporalGraph";
import { cairnStore } from "../../app/cairnStore";
import type { Revision } from "../../contract";

const TL: Revision[] = [
  { id: "r2", message: "add b", timestamp_secs: 20n, author: "x" },
  { id: "r1", message: "init", timestamp_secs: 10n, author: "x" },
];

describe("useTemporalGraph", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.spyOn(cairnStore.getState(), "loadVaultTimeline").mockResolvedValue();
    vi.spyOn(cairnStore.getState(), "loadSnapshot").mockResolvedValue();
    vi.spyOn(cairnStore.getState(), "loadDiff").mockResolvedValue();
    vi.spyOn(cairnStore.getState(), "clearTemporal").mockImplementation(() => {});
    cairnStore.setState({ temporal: { timeline: TL, snapshot: null, diff: null } });
  });
  afterEach(() => vi.useRealTimers());

  it("loads the whole-vault timeline on mount and starts live", () => {
    const { result } = renderHook(() => useTemporalGraph());
    expect(cairnStore.getState().loadVaultTimeline).toHaveBeenCalled();
    expect(result.current.mode).toBe("live");
    expect(result.current.source).toBeNull();
  });

  it("debounces loadSnapshot until the delay elapses on a snapshot selection", () => {
    const { result } = renderHook(() => useTemporalGraph());
    act(() => result.current.setSelection({ kind: "snapshot", at: 0 })); // r2
    expect(cairnStore.getState().loadSnapshot).not.toHaveBeenCalled();
    act(() => vi.advanceTimersByTime(150));
    expect(cairnStore.getState().loadSnapshot).toHaveBeenCalledWith("r2");
    expect(result.current.mode).toBe("snapshot");
  });

  it("clears temporal immediately (no debounce) when returning to live", () => {
    const { result } = renderHook(() => useTemporalGraph());
    act(() => result.current.setSelection({ kind: "snapshot", at: 0 }));
    act(() => vi.advanceTimersByTime(150));
    act(() => result.current.setSelection({ kind: "live" }));
    expect(cairnStore.getState().clearTemporal).toHaveBeenCalled();
  });

  it("dispatches loadDiff on a compare selection after the delay", () => {
    const { result } = renderHook(() => useTemporalGraph());
    act(() => result.current.setSelection({ kind: "compare", from: 1, to: 0 }));
    act(() => vi.advanceTimersByTime(150));
    expect(cairnStore.getState().loadDiff).toHaveBeenCalledWith("r1", "r2");
    expect(result.current.mode).toBe("compare");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm -C web test -- useTemporalGraph.test.tsx`
Expected: FAIL — `useTemporalGraph` still requires an arg / calls `loadTimeline` / no debounce.

- [ ] **Step 3: Rewrite `web/src/components/graph/useTemporalGraph.ts`**

```ts
import { useEffect, useMemo, useRef, useState } from "react";
import { useCairn, useActions } from "../../app/cairnStore";
import {
  selectionToRequest,
  loadTemporalOpen,
  saveTemporalOpen,
  type TemporalSelection,
} from "./temporalControls";
import { debounce } from "../../util/timer";

const SNAPSHOT_DEBOUNCE_MS = 150;

/** Wires the vault-history scrubber to the store's temporal data. Loads the
 *  whole-vault timeline once on mount; snapshot/diff loads are debounced so
 *  dragging the scrubber doesn't fire a full-vault graph_at per tick. Returns
 *  the effective source (null in live mode → caller uses the live graph). */
export function useTemporalGraph() {
  const temporal = useCairn((s) => s.temporal);
  const actions = useActions();
  const [selection, setSelection] = useState<TemporalSelection>({ kind: "live" });
  const [open, setOpenState] = useState(loadTemporalOpen);
  const setOpen = (o: boolean) => {
    setOpenState(o);
    saveTemporalOpen(o);
  };

  // Load the vault-wide timeline once.
  useEffect(() => {
    void actions.loadVaultTimeline();
  }, [actions]);

  const request = useMemo(
    () => selectionToRequest(selection, temporal.timeline),
    [selection, temporal.timeline],
  );

  const requestRef = useRef(request);
  requestRef.current = request;
  const dispatch = useMemo(
    () =>
      debounce(() => {
        const r = requestRef.current;
        if (r.mode === "snapshot") void actions.loadSnapshot(r.revision);
        else if (r.mode === "compare") void actions.loadDiff(r.from, r.to);
      }, SNAPSHOT_DEBOUNCE_MS),
    [actions],
  );

  useEffect(() => {
    if (request.mode === "live") {
      dispatch.cancel();
      actions.clearTemporal();
    } else {
      dispatch();
    }
    return () => dispatch.cancel();
  }, [request, dispatch, actions]);

  const mode = request.mode;
  const source = mode === "live" ? null : temporal.snapshot;
  const diff = mode === "compare" ? temporal.diff : null;

  return { timeline: temporal.timeline, selection, setSelection, open, setOpen, mode, source, diff };
}
```

- [ ] **Step 4: Run the hook test to verify it passes**

Run: `pnpm -C web test -- useTemporalGraph.test.tsx`
Expected: PASS.

- [ ] **Step 5: Update `web/src/components/GraphView.tsx`**

(a) Call site (~line 86):

```tsx
  const temporal = useTemporalGraph();
```

(b) Add scrubber banner data. Immediately after `srcNodes`/`srcEdges` are defined (the `useMemo`s at ~lines 90–97), add:

```tsx
  const scrubberCounts = useMemo(
    () => ({ notes: srcNodes.length, links: srcEdges.length }),
    [srcNodes, srcEdges],
  );
  const scrubberDelta = useMemo(
    () =>
      temporal.diff
        ? {
            added:
              temporal.diff.nodes_added.length + temporal.diff.edges_added.length,
            removed:
              temporal.diff.nodes_removed.length +
              temporal.diff.edges_removed.length,
          }
        : null,
    [temporal.diff],
  );
```

(c) History `IconButton` (~lines 531–540) — remove the disabled coupling:

```tsx
        <IconButton
          label="Graph history"
          className="border border-border bg-surface"
          title="Graph history"
          onClick={() => temporal.setOpen(!temporal.open)}
        >
```

(Delete the `disabled={temporal.disabled}` prop and the `title={temporal.disabled ? ... : ...}` ternary.)

(d) Scrubber mount (~lines 631–637):

```tsx
      {temporal.open && temporal.timeline && (
        <TemporalScrubber
          timeline={temporal.timeline}
          selection={temporal.selection}
          onSelect={temporal.setSelection}
          counts={scrubberCounts}
          delta={scrubberDelta}
        />
      )}
```

- [ ] **Step 6: Full gate**

Run: `just web-ci`
Expected: PASS — typecheck (no `temporal.disabled` / `useTemporalGraph(arg)` references remain), lint, `prettier --check`, all tests.
If typecheck flags a leftover `temporal.disabled` use elsewhere, grep `rg "temporal.disabled" web/src` and remove each.

- [ ] **Step 7: Commit**

```bash
git add web/src/components/graph/useTemporalGraph.ts web/src/components/graph/useTemporalGraph.test.tsx web/src/components/GraphView.tsx
git commit -m "feat(graph): drive the scrubber from vault_history (always-on, debounced)"
```

---

## Manual verification (after Task 4)

- [ ] Run the app; open the graph; click the history (clock) button **with no note open** → scrubber appears (was disabled before).
- [ ] Browse: drag the timeline slider → banner updates ("Viewing vault as of …"), graph rewinds.
- [ ] Compare: switch mode, set From/To → diff-styled graph + banner "Comparing … +/−".
- [ ] Live → snaps back to the current graph.

## Self-review notes (author)

- **Spec coverage:** vault source (T1) ✓ · legibility redesign banner/Browse/Compare/histogram (T2+T3) ✓ · drop activePath / always-on (T4) ✓ · debounce (T4) ✓ · reuse graph_at/graph_diff/buildCompareGraphData (unchanged, verified) ✓ · Phase-2 structural filter explicitly out of scope ✓.
- **Type consistency:** `loadVaultTimeline` name identical in T1 interface, T1 impl, T4 hook, T4 test. Scrubber prop names (`counts`, `delta`) identical in T3 contract and T4 mount. `describeSelection`/`timelineBuckets` identical in T2 and T3.
- **Deferred polish (not Phase 1):** pointer-drag "playhead" over the histogram (range inputs used instead — accessible + testable); time-proportional marker positions; per-day/week zoom.
```
