# CE‑A Click‑to‑Edit (Reveal) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make blockquotes, code blocks, images, wikilinks, and checkbox/bullet markers editable by clicking them in the live preview — by narrowing `atomicRanges` to widgets only (the root fix) and giving the image widget a click→caret handler. (Tables = CE‑B, separate cycle.)

**Architecture:** `buildLivePreviewDecorations` returns `{ decorations, atomic }` where `atomic` is the widget-only subset; the `livePreview()` StateField feeds `atomicRanges` from `atomic` so marker-hide `replace` decorations stop trapping the caret. The image widget gains a mousedown handler that dispatches a caret at its boundary, triggering the existing reveal-on-cursor.

**Tech Stack:** React 18 + TypeScript, CodeMirror 6 (`@codemirror/view`/`state`), `@uiw/react-codemirror`, Vitest, Playwright.

**Spec:** `docs/superpowers/specs/2026-06-02-ce-a-click-to-edit-design.md`

**Working conventions (read before starting):**
- Run all `pnpm` commands from `web/`. Git from repo root or `git -C /Users/titouanlebocq/code/cairn-ui`.
- Dev server (if needed): `pnpm dev --port 5273 --strictPort` (5173 belongs to another app).
- Per-task gate before commit: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check`. `pnpm build` on the final task. Run `pnpm format` + re-stage if format fails.
- Current state: 117 unit tests, 4 e2e tests, all green. `buildLivePreviewDecorations(state, opts)` currently returns a `DecorationSet` and ends with `return Decoration.set(decos, /* sort */ true);`. The `decos()` test harness in `livePreview.test.ts` calls it and reads the set directly. The `livePreview()` StateField wrapper provides `EditorView.decorations.from(f)` + `EditorView.atomicRanges.of((view) => view.state.field(f) ?? Decoration.none)`.
- CodeMirror widget DOM + click interaction do NOT render under jsdom — assert *which decorations/atomic ranges are emitted* in unit tests; assert *click→reveal* in Playwright e2e.

---

## File Structure

| File | Responsibility |
|---|---|
| `web/src/components/editor/livePreview.ts` | Builder returns `{decorations, atomic}` (atomic = widget ranges only); StateField provides decorations + atomicRanges from the two; `LivePreviewOptions.onEditImage`; image scan passes `from`/`onEditImage`. |
| `web/src/components/editor/widgets/imageWidget.ts` | `ImageWidget` gains `from` + `onEdit`; mousedown → `onEdit(from)`; `ignoreEvent → false`; `eq` compares `from`. |
| `web/src/components/Editor.tsx` | Supplies `onEditImage(pos)` dispatching `{selection: EditorSelection.cursor(pos)}` via `viewRef`. |
| `web/src/components/editor/livePreview.test.ts` | Harness reads `.decorations`; new atomic-set tests; `opts` gains `onEditImage`. |
| `web/e2e/skeleton.spec.ts` | Click-to-edit interaction assertions. |

---

## Task 1: Narrow atomicRanges to widgets only (root fix)

**Files:**
- Modify: `web/src/components/editor/livePreview.ts`
- Modify: `web/src/components/editor/livePreview.test.ts`

- [ ] **Step 1: Update the test harness to read `.decorations`, and add an atomic-set helper + failing tests**

In `web/src/components/editor/livePreview.test.ts`, change the `decos()` helper's call to read `.decorations`:

```ts
  const set: DecorationSet = buildLivePreviewDecorations(state, opts).decorations;
```

Add a helper after `decos()` (reuses the same `opts`/extensions setup):

```ts
function atomicRanges(doc: string, cursor: number): { from: number; to: number }[] {
  const state = EditorState.create({
    doc,
    selection: EditorSelection.cursor(cursor),
    extensions: [markdown({ base: markdownLanguage })],
  });
  const set = buildLivePreviewDecorations(state, opts).atomic;
  const out: { from: number; to: number }[] = [];
  set.between(0, doc.length, (from, to) => {
    out.push({ from, to });
  });
  return out;
}
```

Add a new `describe` block with the failing tests:

```ts
describe("atomic ranges (widgets only)", () => {
  it("marks a widget (image) range as atomic", () => {
    const doc = "see ![a](x.png) end";
    const at = doc.indexOf("![");
    expect(atomicRanges(doc, 0).some((r) => r.from === at)).toBe(true);
  });
  it("marks a table widget range as atomic", () => {
    const doc = "intro\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\nend";
    const at = doc.indexOf("| A");
    expect(atomicRanges(doc, 0).some((r) => r.from === at)).toBe(true);
  });
  it("does NOT mark a blockquote's hidden > marker as atomic", () => {
    const doc = "> quoted\n\nbody";
    // cursor off the quote so the > is hidden (a plain replace) — but not atomic
    const ranges = atomicRanges(doc, doc.indexOf("body"));
    expect(ranges.some((r) => r.from === 0)).toBe(false);
  });
  it("does NOT mark a heading's hidden # marker as atomic", () => {
    const doc = "# Title\n\nbody";
    const ranges = atomicRanges(doc, doc.indexOf("body"));
    expect(ranges.some((r) => r.from === 0)).toBe(false);
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `pnpm test -- livePreview`
Expected: FAIL — `buildLivePreviewDecorations(...).atomic`/`.decorations` is undefined (builder still returns a bare `DecorationSet`), and the `decos()` change breaks compile.

- [ ] **Step 3: Change the builder to return `{ decorations, atomic }`**

In `web/src/components/editor/livePreview.ts`, replace the final return of `buildLivePreviewDecorations`:

```ts
  return Decoration.set(decos, /* sort */ true);
```

with:

```ts
  const decorations = Decoration.set(decos, /* sort */ true);
  // Only widget-bearing decorations are atomic (so the caret skips rendered
  // widgets). Plain marker-hide replaces (#, **, > , ```) must NOT be atomic, or
  // a click near a hidden line-start marker gets pushed out of the element.
  const atomicDecos = decos.filter(
    (r) => (r.value.spec as { widget?: unknown }).widget != null,
  );
  const atomic = Decoration.set(atomicDecos, /* sort */ true);
  return { decorations, atomic };
```

Update the function's return type annotation. Find the signature:

```ts
export function buildLivePreviewDecorations(
  state: EditorState,
  opts: LivePreviewOptions,
): DecorationSet {
```

change the return type to:

```ts
export function buildLivePreviewDecorations(
  state: EditorState,
  opts: LivePreviewOptions,
): { decorations: DecorationSet; atomic: DecorationSet } {
```

- [ ] **Step 4: Update the `livePreview()` StateField to use the two sets**

In the same file, replace the StateField definition body in `livePreview()`:

```ts
  const field = StateField.define<DecorationSet>({
    create(state) {
      return buildLivePreviewDecorations(state, opts);
    },
    update(value, tr) {
      if (tr.docChanged || tr.selection) {
        return buildLivePreviewDecorations(tr.state, opts);
      }
      return value;
    },
    provide: (f) => [
      EditorView.decorations.from(f),
      EditorView.atomicRanges.of(
        (view) => view.state.field(f) ?? Decoration.none,
      ),
    ],
  });
```

with:

```ts
  const field = StateField.define<{
    decorations: DecorationSet;
    atomic: DecorationSet;
  }>({
    create(state) {
      return buildLivePreviewDecorations(state, opts);
    },
    update(value, tr) {
      if (tr.docChanged || tr.selection) {
        return buildLivePreviewDecorations(tr.state, opts);
      }
      return value;
    },
    provide: (f) => [
      EditorView.decorations.from(f, (v) => v.decorations),
      EditorView.atomicRanges.of(
        (view) => view.state.field(f)?.atomic ?? Decoration.none,
      ),
    ],
  });
```

- [ ] **Step 5: Run the tests to verify pass**

Run: `pnpm test -- livePreview`
Expected: PASS — the 4 new atomic-set tests pass and all existing tests pass (the `decos()` harness now reads `.decorations`; the full decoration set is unchanged).

- [ ] **Step 6: Per-task gate**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check`
Expected: all PASS (121 tests: 117 + 4 new). Run `pnpm format` + re-stage if needed.

- [ ] **Step 7: Commit**

```bash
git add web/src/components/editor/livePreview.ts web/src/components/editor/livePreview.test.ts
git commit -m "fix(editor): make only widgets atomic so clicks reach hidden markers"
```

---

## Task 2: Image click-to-edit handler

**Files:**
- Modify: `web/src/components/editor/widgets/imageWidget.ts`
- Modify: `web/src/components/editor/livePreview.ts`
- Modify: `web/src/components/Editor.tsx`
- Modify: `web/src/components/editor/livePreview.test.ts`

- [ ] **Step 1: Update the `ImageWidget`**

Replace `web/src/components/editor/widgets/imageWidget.ts` with:

```ts
import { WidgetType } from "@codemirror/view";

export class ImageWidget extends WidgetType {
  constructor(
    readonly src: string,
    readonly alt: string,
    readonly block: boolean,
    readonly from: number,
    readonly onEdit: (from: number) => void,
  ) {
    super();
  }
  eq(other: ImageWidget): boolean {
    return (
      other.src === this.src &&
      other.alt === this.alt &&
      other.block === this.block &&
      other.from === this.from
    );
  }
  toDOM(): HTMLElement {
    const img = document.createElement("img");
    img.className = this.block ? "cm-lp-img block" : "cm-lp-img";
    img.src = this.src;
    img.alt = this.alt;
    img.addEventListener("mousedown", (e) => {
      e.preventDefault();
      this.onEdit(this.from);
    });
    return img;
  }
  ignoreEvent(): boolean {
    return false;
  }
}
```

- [ ] **Step 2: Add `onEditImage` to options and pass it in the image scan**

In `web/src/components/editor/livePreview.ts`, extend `LivePreviewOptions` (it currently has `resolve`, `onOpenNote`, `onToggleCheckbox`, `resolveImage`):

```ts
  onEditImage: (from: number) => void;
```

In the image scan, change the widget construction:

```ts
    decos.push(
      Decoration.replace({
        widget: new ImageWidget(src, alt, block, from, opts.onEditImage),
      }).range(from, to),
    );
```

- [ ] **Step 3: Add `onEditImage` to the test `opts` and add a failing test**

In `web/src/components/editor/livePreview.test.ts`, add to the shared `opts`:

```ts
  onEditImage: vi.fn(),
```

Add a test (image widget still emitted at the match `from` with the new ctor args):

```ts
  it("still renders an image widget at the match position off-cursor", () => {
    const doc = "see ![logo](img/logo.png) end";
    const at = doc.indexOf("![");
    const ds = decos(doc, 0);
    expect(ds.some((d) => d.widget && d.from === at)).toBe(true);
  });
```

- [ ] **Step 4: Run to verify failure**

Run: `pnpm test -- livePreview`
Expected: FAIL — typecheck error (`onEditImage` missing from `opts`/options) and/or the `ImageWidget` ctor arity. Confirm the failure is the expected compile/shape error.

- [ ] **Step 5: Wire `onEditImage` in `Editor.tsx`**

In `web/src/components/Editor.tsx`, import `EditorSelection`:

```tsx
import { EditorSelection } from "@codemirror/state";
```

In the `extensions` memo where `livePreview({...})` is called (it already passes `resolve`, `onOpenNote`, `onToggleCheckbox`, `resolveImage`), add:

```tsx
      onEditImage: (pos: number) => {
        const view = viewRef.current;
        if (!view) return;
        view.dispatch({ selection: EditorSelection.cursor(pos) });
      },
```

(`viewRef` already exists from the checkbox wiring. `onEditImage` reads only the stable `viewRef`, so it does not need to be in the memo deps — match the existing `onToggleCheckbox` handling; do not add lint-disable unless the linter actually complains.)

- [ ] **Step 6: Run the tests to verify pass**

Run: `pnpm test -- livePreview`
Expected: PASS.

- [ ] **Step 7: Per-task gate**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check`
Expected: all PASS (122 tests). Fix format if needed.

- [ ] **Step 8: Commit**

```bash
git add web/src/components/editor/widgets/imageWidget.ts web/src/components/editor/livePreview.ts web/src/components/editor/livePreview.test.ts web/src/components/Editor.tsx
git commit -m "feat(editor): click an image to reveal its raw markdown"
```

---

## Task 3: e2e click-to-edit + final gate

**Files:**
- Modify: `web/e2e/skeleton.spec.ts`

- [ ] **Step 1: Add the click-to-edit e2e test**

Append to `web/e2e/skeleton.spec.ts` (the `kitchensink.md` fixture already exists from UI‑3 and contains a blockquote `> a quoted line`, a `js` code block with `const x = 1;`, and an image `![logo](img/logo.png)`):

```ts
test("click-to-edit: blockquote, code block, and image reveal raw on click", async ({
  page,
}) => {
  await page.goto("/");
  await page.getByRole("button", { name: "kitchensink.md" }).click();
  const content = page.locator(".cm-content");

  // Blockquote: clicking its text reveals the raw "> " marker.
  await page.getByText("a quoted line").click();
  await expect(content).toContainText("> a quoted line");

  // Code block: clicking inside reveals the ``` fences.
  await page.getByText("const x = 1;").click();
  await expect(content).toContainText("```");

  // Image: clicking the rendered <img> reveals its raw markdown and removes the img.
  await expect(page.locator("img.cm-lp-img")).toBeVisible();
  await page.locator("img.cm-lp-img").click();
  await expect(content).toContainText("![logo](img/logo.png)");
  await expect(page.locator("img.cm-lp-img")).toHaveCount(0);
});
```

- [ ] **Step 2: Run e2e**

Run: `pnpm e2e`
Expected: all 5 e2e tests pass (4 existing + this one). The existing wikilink test (click opens the note) must still pass — wikilinks stay atomic widgets with their open handler, unaffected by the atomicRanges narrowing.
If a click selector is flaky (e.g. `getByText` matches multiple nodes), scope it (`.cm-content`-relative or `.first()`), but do NOT weaken the reveal assertions. If a *reveal* assertion genuinely fails (not a selector issue), that's a real regression — STOP and report it.

- [ ] **Step 3: Final full gate + build**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check && pnpm build`
Expected: all PASS.

- [ ] **Step 4: Manual/visual sanity (no browser available to the agent)**

Start the dev server briefly and confirm it serves without runtime errors:
`pnpm dev --port 5273 --strictPort` (background), `curl -s -o /dev/null -w "%{http_code}" http://localhost:5273` (expect 200), check the dev log for errors, then stop it. Report that the app loads. (The human does the actual visual click-test.)

- [ ] **Step 5: Commit**

```bash
git add web/e2e/skeleton.spec.ts
git commit -m "test(e2e): click-to-edit reveals raw for blockquote, code, image"
```

---

## Notes for the executor

- **Why the atomic narrowing is the fix:** the pure builder already reveals on cursor (existing tests prove it); the bug was that the whole decoration set was atomic, so the line-start `> ` replace bounced a clicked caret out of the blockquote. Making only widgets atomic lets the caret land in/adjacent to hidden markers. Widgets (image/table/wikilink/checkbox/bullet/HR) stay atomic so arrow-key navigation still skips them cleanly.
- **Boundary caret for images:** dispatching `EditorSelection.cursor(from)` places the caret at the image's start boundary (allowed past the atomic filter); `selectionTouches(from, to)` is inclusive at `from`, so the rebuild drops the widget and shows raw `![alt](path)`.
- **Do not touch the table or HR branches** — tables are CE‑B; HR is out of scope. The atomicRanges change benefits them for free (they stay atomic) but no behavior change is intended for them here.
- **`{decorations, atomic}` return shape:** the only callers are the `livePreview()` StateField and the `decos()`/`atomicRanges()` test helpers — all updated in Task 1. Typecheck catches any missed caller. The `.spec.widget` filter matches the existing harness pattern (`value.spec as { class?; widget? }`), so it's a known-good way to identify widget decorations.
- **Checkbox markers & bullets (spec scope, no code change):** these are widget decorations and intentionally stay atomic. Their markers reveal via the existing caret-adjacent mechanism — placing the caret at the marker boundary (arrow into it from the adjacent editable text) flips `selectionTouches` and reveals the raw `[ ]`/`- `; clicking the checkbox still toggles it. This works for free once Task 1 lands (the text around them was always editable) and needs no new code; it is keyboard-driven so it is not separately e2e-tested (avoids brittle synthetic-arrow tests). If manual testing shows a marker won't reveal, treat it as a follow-up.
