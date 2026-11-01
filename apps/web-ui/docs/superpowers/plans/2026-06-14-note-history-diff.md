# Note history — diff-vs-current view Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Upgrade the read-only revision view into a diff-vs-current-working-copy view, defaulting to diff with a Diff/Full toggle.

**Architecture:** A pure zero-dep LCS line-diff util (`lineDiff.ts`) feeds a render upgrade inside `RevisionView`. The old side is the already-fetched `viewingRevision.contents`; the new side is the active working buffer passed from `EditorPane` as one new prop. No store/contract/mock/engine changes.

**Tech Stack:** React + TypeScript, Tailwind (v4 via JS config), Vitest + Testing Library, pnpm. All commands run from `web/`.

---

### Task 1: Add success color tokens

**Files:**
- Modify: `web/tailwind.config.ts:19-20`

No test (config-only; verified transitively by the RevisionView render test in Task 3).

- [ ] **Step 1: Add the two tokens after `danger-bg`**

In `web/tailwind.config.ts`, the `colors` block currently ends:

```ts
        danger: "#f87171",
        "danger-bg": "#2a1416",
```

Change it to:

```ts
        danger: "#f87171",
        "danger-bg": "#2a1416",
        success: "#4ade80",
        "success-bg": "#0f2417",
```

- [ ] **Step 2: Verify the config still typechecks**

Run: `cd web && pnpm typecheck`
Expected: PASS (no errors).

- [ ] **Step 3: Commit**

```bash
git add web/tailwind.config.ts
git commit -m "feat(history): add success/success-bg theme tokens for diff adds"
```

---

### Task 2: lineDiff util (LCS line diff)

**Files:**
- Create: `web/src/components/history/lineDiff.ts`
- Test: `web/src/components/history/lineDiff.test.ts`

- [ ] **Step 1: Write the failing tests**

Create `web/src/components/history/lineDiff.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import { lineDiff } from "./lineDiff";

describe("lineDiff", () => {
  it("returns all context rows for identical text", () => {
    const rows = lineDiff("a\nb\nc", "a\nb\nc");
    expect(rows.map((r) => r.type)).toEqual(["ctx", "ctx", "ctx"]);
    expect(rows.map((r) => r.text)).toEqual(["a", "b", "c"]);
    expect(rows[0]).toMatchObject({ oldLine: 1, newLine: 1 });
  });

  it("returns all adds when old is empty", () => {
    const rows = lineDiff("", "x\ny");
    expect(rows).toEqual([
      { type: "add", text: "x", oldLine: null, newLine: 1 },
      { type: "add", text: "y", oldLine: null, newLine: 2 },
    ]);
  });

  it("returns all dels when new is empty", () => {
    const rows = lineDiff("x\ny", "");
    expect(rows).toEqual([
      { type: "del", text: "x", oldLine: 1, newLine: null },
      { type: "del", text: "y", oldLine: 2, newLine: null },
    ]);
  });

  it("returns [] when both are empty", () => {
    expect(lineDiff("", "")).toEqual([]);
  });

  it("detects a mid-document insertion", () => {
    const rows = lineDiff("a\nc", "a\nb\nc");
    expect(rows).toEqual([
      { type: "ctx", text: "a", oldLine: 1, newLine: 1 },
      { type: "add", text: "b", oldLine: null, newLine: 2 },
      { type: "ctx", text: "c", oldLine: 2, newLine: 3 },
    ]);
  });

  it("detects a deletion", () => {
    const rows = lineDiff("a\nb\nc", "a\nc");
    expect(rows).toEqual([
      { type: "ctx", text: "a", oldLine: 1, newLine: 1 },
      { type: "del", text: "b", oldLine: 2, newLine: null },
      { type: "ctx", text: "c", oldLine: 3, newLine: 2 },
    ]);
  });

  it("treats matching trailing newlines as context, not a spurious diff", () => {
    const rows = lineDiff("a\nb\n", "a\nb\n");
    expect(rows.every((r) => r.type === "ctx")).toBe(true);
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd web && pnpm test -- lineDiff`
Expected: FAIL — cannot resolve `./lineDiff` / `lineDiff is not a function`.

- [ ] **Step 3: Write the implementation**

Create `web/src/components/history/lineDiff.ts`:

```ts
export type DiffLineType = "add" | "del" | "ctx";

export interface DiffLine {
  type: DiffLineType;
  /** Line content, without the trailing newline. */
  text: string;
  /** 1-based line number in the old text; null for an "add". */
  oldLine: number | null;
  /** 1-based line number in the new text; null for a "del". */
  newLine: number | null;
}

// Empty text is zero lines (not [""]) so empty/both-empty edges stay clean.
const toLines = (text: string): string[] => (text === "" ? [] : text.split("\n"));

/**
 * Line-level diff of `oldText` vs `newText` via longest-common-subsequence.
 * Returns an ordered row list: context lines plus the adds/dels needed to turn
 * old into new. O(n*m) — fine for note-sized documents.
 */
export function lineDiff(oldText: string, newText: string): DiffLine[] {
  const oldLines = toLines(oldText);
  const newLines = toLines(newText);
  const n = oldLines.length;
  const m = newLines.length;

  // dp[i][j] = LCS length of oldLines[i..] and newLines[j..].
  const dp: number[][] = Array.from({ length: n + 1 }, () =>
    new Array<number>(m + 1).fill(0),
  );
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      dp[i][j] =
        oldLines[i] === newLines[j]
          ? dp[i + 1][j + 1] + 1
          : Math.max(dp[i + 1][j], dp[i][j + 1]);
    }
  }

  const rows: DiffLine[] = [];
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (oldLines[i] === newLines[j]) {
      rows.push({ type: "ctx", text: oldLines[i], oldLine: i + 1, newLine: j + 1 });
      i++;
      j++;
    } else if (dp[i + 1][j] >= dp[i][j + 1]) {
      rows.push({ type: "del", text: oldLines[i], oldLine: i + 1, newLine: null });
      i++;
    } else {
      rows.push({ type: "add", text: newLines[j], oldLine: null, newLine: j + 1 });
      j++;
    }
  }
  while (i < n) {
    rows.push({ type: "del", text: oldLines[i], oldLine: i + 1, newLine: null });
    i++;
  }
  while (j < m) {
    rows.push({ type: "add", text: newLines[j], oldLine: null, newLine: j + 1 });
    j++;
  }
  return rows;
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd web && pnpm test -- lineDiff`
Expected: PASS (7 tests).

- [ ] **Step 5: Commit**

```bash
git add web/src/components/history/lineDiff.ts web/src/components/history/lineDiff.test.ts
git commit -m "feat(history): add pure LCS lineDiff util"
```

---

### Task 3: RevisionView diff render + Diff/Full toggle

**Files:**
- Modify: `web/src/components/history/RevisionView.tsx`
- Test: `web/src/components/history/RevisionView.test.tsx`

- [ ] **Step 1: Replace the test file with diff-aware tests**

Replace `web/src/components/history/RevisionView.test.tsx` with:

```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { RevisionView } from "./RevisionView";

const base = {
  revision: "r1",
  contents: "a\nb",
  current: "a\nc",
  onBack: vi.fn(),
  onRestore: vi.fn(),
};

describe("RevisionView", () => {
  it("keeps the read-only banner with the revision", () => {
    render(<RevisionView {...base} />);
    expect(screen.getByText(/read-only/i)).toBeInTheDocument();
    expect(screen.getByText(/r1/)).toBeInTheDocument();
  });

  it("renders a diff by default with add and del markers", () => {
    render(<RevisionView {...base} />);
    // old "a\nb" -> new "a\nc": b removed, c added.
    expect(screen.getByText("c")).toBeInTheDocument();
    expect(screen.getByText("b")).toBeInTheDocument();
    expect(screen.getByText("+")).toBeInTheDocument();
    expect(screen.getByText("-")).toBeInTheDocument();
  });

  it("switches to full mode showing raw revision contents without markers", () => {
    const { container } = render(<RevisionView {...base} />);
    fireEvent.click(screen.getByRole("button", { name: "Full" }));
    expect(screen.queryByText("+")).not.toBeInTheDocument();
    expect(screen.queryByText("-")).not.toBeInTheDocument();
    // Full mode renders raw contents in a <pre>.
    const pre = container.querySelector("pre");
    expect(pre?.textContent).toBe("a\nb");
  });

  it("switches back to diff mode", () => {
    render(<RevisionView {...base} />);
    fireEvent.click(screen.getByRole("button", { name: "Full" }));
    fireEvent.click(screen.getByRole("button", { name: "Diff" }));
    expect(screen.getByText("+")).toBeInTheDocument();
  });

  it("fires onBack and onRestore from the diff view", () => {
    const onBack = vi.fn();
    const onRestore = vi.fn();
    render(<RevisionView {...base} onBack={onBack} onRestore={onRestore} />);
    fireEvent.click(screen.getByRole("button", { name: /back to current/i }));
    fireEvent.click(screen.getByRole("button", { name: /restore/i }));
    expect(onBack).toHaveBeenCalled();
    expect(onRestore).toHaveBeenCalled();
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd web && pnpm test -- RevisionView`
Expected: FAIL — `current` prop type error and/or no `+`/`-`/`Diff`/`Full` elements.

- [ ] **Step 3: Rewrite the component**

Replace `web/src/components/history/RevisionView.tsx` with:

```tsx
import { useState } from "react";
import { Button } from "../ui/Button";
import { lineDiff, type DiffLine } from "./lineDiff";

function DiffRow({ row }: { row: DiffLine }) {
  const sym = row.type === "add" ? "+" : row.type === "del" ? "-" : " ";
  const tone =
    row.type === "add"
      ? "bg-success-bg text-success"
      : row.type === "del"
        ? "bg-danger-bg text-danger"
        : "text-muted";
  return (
    <div className={`flex ${tone}`}>
      <span className="w-10 shrink-0 select-none px-1 text-right text-faint">
        {row.oldLine ?? ""}
      </span>
      <span className="w-10 shrink-0 select-none px-1 text-right text-faint">
        {row.newLine ?? ""}
      </span>
      <span className="w-4 shrink-0 select-none text-center">{sym}</span>
      <span className="whitespace-pre-wrap break-words">{row.text}</span>
    </div>
  );
}

// Phase 2: diff-vs-current with a Diff/Full toggle (default diff). The old side
// is the fetched revision; the new side is the live working buffer (`current`).
export function RevisionView(props: {
  revision: string;
  contents: string;
  current: string;
  onBack: () => void;
  onRestore: () => void;
}) {
  const [mode, setMode] = useState<"diff" | "full">("diff");
  const rows = mode === "diff" ? lineDiff(props.contents, props.current) : [];

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center gap-2 border-b border-accent/40 bg-accent/10 px-3 py-2 text-xs text-text">
        <span>
          Viewing <span className="font-mono">{props.revision}</span> —
          read-only
        </span>
        <div className="ml-auto flex items-center gap-2">
          <div className="flex overflow-hidden rounded border border-border">
            <button
              type="button"
              aria-pressed={mode === "diff"}
              onClick={() => setMode("diff")}
              className={
                mode === "diff"
                  ? "bg-accent px-2 py-0.5 text-accent-fg"
                  : "px-2 py-0.5 text-muted hover:text-text"
              }
            >
              Diff
            </button>
            <button
              type="button"
              aria-pressed={mode === "full"}
              onClick={() => setMode("full")}
              className={
                mode === "full"
                  ? "bg-accent px-2 py-0.5 text-accent-fg"
                  : "px-2 py-0.5 text-muted hover:text-text"
              }
            >
              Full
            </button>
          </div>
          <Button variant="ghost" onClick={props.onBack}>
            ← Back to current
          </Button>
          <Button variant="primary" onClick={props.onRestore}>
            Restore
          </Button>
        </div>
      </div>
      {mode === "diff" ? (
        <div className="min-h-0 flex-1 overflow-auto py-2 font-mono text-sm">
          {rows.map((row, idx) => (
            <DiffRow key={idx} row={row} />
          ))}
        </div>
      ) : (
        <pre className="min-h-0 flex-1 overflow-auto whitespace-pre-wrap p-3 font-mono text-sm text-muted">
          {props.contents}
        </pre>
      )}
    </div>
  );
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd web && pnpm test -- RevisionView`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
git add web/src/components/history/RevisionView.tsx web/src/components/history/RevisionView.test.tsx
git commit -m "feat(history): render diff-vs-current in RevisionView with Diff/Full toggle"
```

---

### Task 4: Wire the current buffer in EditorPane + full gate

**Files:**
- Modify: `web/src/components/EditorPane.tsx:130-138`

The existing `<RevisionView/>` is missing the new required `current` prop, so the
build will fail until this is wired — no separate test; the existing EditorPane /
suite plus the gate cover it.

- [ ] **Step 1: Pass the working buffer**

In `web/src/components/EditorPane.tsx`, the `<RevisionView/>` block reads:

```tsx
          <RevisionView
            revision={viewingRevision.revision}
            contents={viewingRevision.contents}
            onBack={() => actions.exitRevisionView()}
            onRestore={() =>
              void actions.restoreRevision(viewingRevision.revision)
            }
          />
```

Add the `current` prop (the working buffer is already in scope as `buffer`, line 34):

```tsx
          <RevisionView
            revision={viewingRevision.revision}
            contents={viewingRevision.contents}
            current={buffer}
            onBack={() => actions.exitRevisionView()}
            onRestore={() =>
              void actions.restoreRevision(viewingRevision.revision)
            }
          />
```

- [ ] **Step 2: Run the full local gate**

Run: `cd web && pnpm typecheck && pnpm lint && pnpm run format:check && pnpm test`
Expected: all PASS. If `format:check` fails, run `pnpm run format` then re-run the gate and amend.

- [ ] **Step 3: Commit**

```bash
git add web/src/components/EditorPane.tsx
git commit -m "feat(history): feed working buffer into RevisionView for diff"
```

---

## Self-Review

**Spec coverage:**
- `lineDiff` util + schema (DiffLine with line numbers) → Task 2. ✓
- success/success-bg tokens → Task 1. ✓
- RevisionView `mode` state + `current` prop + Diff/Full toggle, diff default → Task 3. ✓
- Gutter render with +/−/ctx and whitespace preservation → Task 3 (`DiffRow`). ✓
- EditorPane one-line wiring (`current={buffer}`) → Task 4. ✓
- Diff semantics old=revision/new=current → Task 2 arg order + Task 3 call. ✓
- All edge cases (identical/empty/both-empty/insert/delete/trailing-nl) → Task 2 tests. ✓
- Restore still works from diff view → Task 3 test. ✓
- Full gate green → Task 4. ✓

**Placeholder scan:** none — every code/command step is concrete.

**Type consistency:** `DiffLine`/`DiffLineType`/`lineDiff` signatures identical across Task 2 def and Task 3 import; `current: string` prop matches `current={buffer}` (buffer is `string`) in Task 4; token names `success`/`success-bg` match the `bg-success-bg text-success` classes in Task 3.
