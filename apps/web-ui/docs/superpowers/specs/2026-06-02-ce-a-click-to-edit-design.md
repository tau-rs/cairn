# Cairn Web UI — CE‑A: Click‑to‑Edit (Reveal) Design Spec

> **Correction (post-implementation, 2026-06-02):** the root cause analysis in §1
> below was **wrong**. Narrowing `atomicRanges` to widgets (§2 Mechanism A) is a
> valid, kept cleanup, but it did NOT fix the blockquote. The real bug was
> `margin: 0.15em 0` on `.cm-line` (added in UI‑3): CodeMirror lays lines out
> contiguously and ignores margins in its height map, so `posAtCoords` drifted and
> clicks on a blockquote landed on the line below. The fix was changing that margin
> to `padding-top/bottom`. The image click handler (§2 Mechanism B) shipped as
> designed. Lesson: interaction bugs like this are only caught by e2e — the unit
> tests passed throughout because the pure builder was always correct.

**Date:** 2026-06-02
**Status:** approved, ready for implementation planning
**Sub-project:** CE‑A of the click‑to‑edit initiative (CE‑B, the rich table editor,
is a separate later cycle). Follows UI‑3 (document-look live preview, merged).
**Builds on:** the UI‑3 live-preview pipeline (`livePreview.ts` StateField +
`buildLivePreviewDecorations` + widgets) and the graphite design system.

---

## 1. Purpose

Make every complex live-preview element **editable by clicking it**, except
tables (deferred to CE‑B). Today several elements can't be edited in place: the
caret either can't reach them or a click doesn't reveal their raw markdown. CE‑A
fixes this at the root so the existing reveal-on-cursor behavior takes over — no
new rendering, no new modes.

### The two confirmed problems (from live probing)

1. **Blockquotes don't reveal on click** (and the same class of bug affects any
   marker hidden at a line start). The **pure builder is correct** — the unit test
   "reveals the `>` marker when the cursor is in the quote" passes — so the bug is
   in the *interaction* layer: `livePreview()` feeds the **entire** decoration set
   to `EditorView.atomicRanges`, so the atomic `> ` line-start `replace` pushes a
   click-placed caret *out* of the blockquote (onto the preceding blank line),
   leaving the marker hidden. Inline marks (bold) work because their hidden `**`
   sit mid-line, away from where you click.
2. **Atomic widgets can't be clicked into.** Images are `contenteditable=false`
   atomic widgets, so a click can't land a caret in their source.

### Locked decisions (from brainstorming, element-by-element)

| Element | Decision |
|---|---|
| Blockquote | Fix: clicking anywhere in it reveals raw `> ` for editing. |
| Code block | Verify (and fix if needed): clicking inside reveals the ``` fences. |
| Image | Reveal raw `![alt](path)` inline on click (atomic widget → click handler). |
| Wikilink | Click still **opens** the note (unchanged); editing via caret-adjacent reveal of `[[…]]`. |
| Checkbox marker / bullet | Caret adjacent to the `[ ]`/`-` marker reveals it for editing (checkbox click still toggles). |
| Table | **Out of scope** — rich editor in CE‑B. |
| Horizontal rule | **Out of scope** (user excluded). |
| Edit experience | **Reveal raw markdown inline** for all CE‑A elements (the rich table editor is the only "rich" case, and it's CE‑B). |

### Non-goals (deferred)

- The rich table editor (CE‑B).
- Horizontal-rule editing.
- Any new rendering, new editor mode, or change to what markdown renders.

---

## 2. Architecture

The change is contained to the editor's live-preview source — primarily the
`livePreview()` StateField wrapper and the `ImageWidget`. **No** changes to the
markdown rendering, the store, the host, or the contract.

```
components/editor/livePreview.ts     MODIFY — buildLivePreviewDecorations returns
                                     { decorations, atomic } where `atomic` collects ONLY
                                     widget-bearing ranges; the StateField provides
                                     decorations from one and atomicRanges from the other.
                                     Image scan passes the match `from` + opts.onEditImage.
                                     LivePreviewOptions gains onEditImage.
components/editor/widgets/imageWidget.ts  MODIFY — add `from` + `onEdit`; mousedown →
                                     onEdit(from); ignoreEvent() → false.
components/Editor.tsx                MODIFY — supply onEditImage(pos) that dispatches
                                     { selection: EditorSelection.cursor(pos) } via viewRef
                                     (mirrors onToggleCheckbox).
e2e/skeleton.spec.ts                 MODIFY — add click-to-edit interaction assertions.
```

### Mechanism A — narrow `atomicRanges` to widgets only (the root fix)

`buildLivePreviewDecorations` already builds the full decoration list. It also
collects, into a second list, **only** the decorations that carry a `widget`
(wikilink, image, table, checkbox, bullet, HR). It returns
`{ decorations: Decoration.set(all, true), atomic: Decoration.set(widgetsOnly, true) }`.

The `livePreview()` StateField stores that object and provides:
- `EditorView.decorations.from(field, (v) => v.decorations)`
- `EditorView.atomicRanges.of((view) => view.state.field(field)?.atomic ?? Decoration.none)`

Effect: plain marker-hide `replace({})` decorations (`#`, `**`, `*`, `` ` ``, `> `,
``` ``` ```) are **no longer atomic**, so a caret can land in/adjacent to them and
`selectionTouches` flips the element to raw. Widgets stay atomic, preserving clean
arrow-key navigation (the caret still skips over a rendered image/table/wikilink).
This single change fixes blockquotes, code fences, and checkbox/bullet markers.

### Mechanism B — image click handler (atomic widget)

The image stays an atomic widget (so navigation skips it), but gains a click path
into edit:
- `ImageWidget` constructor gains `from: number` and `onEdit: (from: number) => void`.
  `toDOM` adds a `mousedown` listener: `e.preventDefault(); this.onEdit(this.from);`.
  `ignoreEvent()` returns `false` (so the listener fires). `eq()` must also compare
  `from` (so a shifted image doesn't reuse a stale widget and dispatch the caret to
  the wrong position); `onEdit` is a stable callback and need not be compared.
- The image scan in `buildLivePreviewDecorations` constructs
  `new ImageWidget(src, alt, block, from, opts.onEditImage)` (the match's `from`).
- `LivePreviewOptions` gains `onEditImage: (from: number) => void`.
- `Editor.tsx` passes `onEditImage: (pos) => viewRef.current?.dispatch({ selection: EditorSelection.cursor(pos) })`.

Dispatching the caret at the widget's `from` **boundary** is allowed past the
atomic filter (boundaries aren't "inside"), and `selectionTouches(from, to)` is
true at `from`, so the next rebuild drops the image widget and shows raw
`![alt](path)` with the caret in it.

---

## 3. Per-element behavior

| Element | Today | CE‑A | Mechanism |
|---|---|---|---|
| Blockquote | click doesn't reveal `>` | click → reveals `> `, edit, re-render on leave | A (atomicRanges narrowed) |
| Code block | likely works | click inside → reveals ``` fences | A (verify) |
| Image | click does nothing | click → reveals raw `![alt](path)` with caret in it | B (new handler) |
| Wikilink | click opens note | click opens (unchanged); caret-adjacent reveals `[[…]]` | A (verify; widget stays atomic, click=open handler unchanged) |
| Checkbox marker / bullet | checkbox toggles; text editable | caret adjacent to the marker reveals it | A (verify) |
| Table | rendered widget | unchanged (CE‑B) | — |
| Horizontal rule | rendered widget | unchanged (out of scope) | — |

---

## 4. Testing

- **Unit (Vitest, pure builder):**
  - `buildLivePreviewDecorations` now returns `{ decorations, atomic }`. Assert the
    `atomic` set **includes** widget ranges (e.g. an image widget, a table widget,
    a wikilink widget) and **excludes** plain marker-hide replaces (e.g. the `> `
    hide in a blockquote, the heading `#` hide). Assert `decorations` is unchanged
    from before (same full set).
  - Update every call site that reads the return value (the `decos()` test harness
    must read `.decorations` from the returned object). Keep all existing reveal
    tests green (they assert against the full decoration set).
  - The image scan passes `from`/`onEditImage` — assert the emitted image widget
    still appears at the match `from` off-cursor.
- **e2e (Playwright — authoritative interaction coverage):** open `kitchensink.md`
  and assert, against the real browser:
  - clicking the blockquote text reveals `> ` (the line shows the raw marker);
  - clicking inside the code block reveals the ``` fences;
  - clicking the image reveals raw `![logo](img/logo.png)` (the `<img>` is replaced
    by editable text);
  - the wikilink: clicking it still opens the linked note (unchanged);
  - moving the caret out re-renders each element.
- All existing unit tests (117) + e2e (4) stay green; Tauri/desktop unaffected.

---

## 5. Files & dependencies

| File | Change |
|---|---|
| `web/src/components/editor/livePreview.ts` (+ test) | **Modify.** Return `{decorations, atomic}`; widget-only atomic set; image scan passes `from`/`onEditImage`; `LivePreviewOptions.onEditImage`. |
| `web/src/components/editor/widgets/imageWidget.ts` | **Modify.** `from` + `onEdit` + mousedown; `ignoreEvent → false`. |
| `web/src/components/Editor.tsx` | **Modify.** Supply `onEditImage` dispatching a caret via `viewRef`. |
| `web/e2e/skeleton.spec.ts` | **Modify.** Click-to-edit interaction assertions. |

No new dependencies. No store/host/contract changes.

---

## 6. Risks

- **atomicRanges narrowing regresses caret navigation.** Mitigation: widgets stay
  atomic (the property that matters for arrow-key skipping over rendered
  elements); only the zero-visible-width marker-hides become non-atomic, which is
  desirable. e2e arrow/selection behavior over a wikilink/image confirms widgets
  still skip cleanly.
- **Image caret-at-boundary still revealing.** If dispatching at `from` doesn't
  reveal (atomic filter edge case), fall back to `from + 1`? No — `from` is the
  boundary and `selectionTouches` is inclusive at `from`; verified by the reveal
  unit-test semantics. The e2e is the backstop.
- **`{decorations, atomic}` return-shape churn.** Every caller of
  `buildLivePreviewDecorations` (the StateField + the `decos()` test harness) must
  read `.decorations`. The plan updates all call sites in one task; typecheck
  catches misses.
- **Blockquote fix is interaction-level.** The unit suite can't prove the click
  fix (the builder was already correct); the e2e click assertion is the real
  guard. Without it the bug could silently return.
