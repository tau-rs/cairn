# Cairn Web UI — UI‑3: Document-Look Live Preview Design Spec

**Date:** 2026-06-02
**Status:** approved, ready for implementation planning
**Sub-project:** UI‑3 of the UI/UX overhaul (UI‑1 + UI‑2 done).
**Builds on:** UI‑1 design system (graphite tokens, Inter, primitives) and the
existing CodeMirror 6 live-preview pipeline (`livePreview.ts` + `wikilinkWidget.ts`).

---

## 1. Purpose

Make the editor's **live-preview** read like a rendered document — GitHub-README
fidelity — while staying a single editable surface (Obsidian "Live Preview"
model): markers hide, blocks render, and the element under the caret reveals its
raw markdown so you can edit in place. This addresses the audit complaint that
"live preview looks like a code editor."

### Locked decisions (from brainstorming)

| Decision | Choice |
|---|---|
| Editing model | **One editable CodeMirror surface** (hybrid live-preview). NOT a separate rendered read view. The existing per-note **Source** toggle stays. |
| Render scope | **Full GitHub parity:** existing inline rendering PLUS lists, blockquotes, fenced code, horizontal rules, task checkboxes, tables, and images. |
| Layout | **Centered readable measure** (~68ch, `max-width: 34em; margin: 0 auto`) in live-preview. Source mode stays monospace, left-aligned, full-width. |
| Code blocks | **Syntax-highlighted** per language (via `codeLanguages`). |
| Images | **Local relative images render inline** (host-resolved) plus remote/`data:` URLs. |
| Task checkboxes | **Clickable** — clicking toggles `- [ ]` ↔ `- [x]` in the source. |
| Reveal-on-cursor | **Kept** — selection touching a rendered element reveals raw markdown; re-renders on leave. |

### Non-goals (deferred)

- A separate non-editable Reading view, or a 3-mode (Reading/Live/Source) cycle.
- Footnotes, definition lists, math/LaTeX, mermaid/diagram fences, embedded
  callouts/admonitions, front-matter rendering.
- Image resizing/captions, lightbox, drag-drop/paste image insertion.
- UI‑4 (Obsidian graph) — separate cycle.

---

## 2. Architecture

The change is contained to the editor. **No command/query/contract changes.** One
small, well-bounded host addition (`assetUrl`) for local image resolution, in
keeping with the existing transport abstraction.

```
components/Editor.tsx                MODIFY — replace theme="dark" + default basicSetup
                                     with the minimal setup + docTheme; restyle the header;
                                     wire the imageResolver (from host) into livePreview.
components/editor/livePreview.ts     MODIFY — extend buildLivePreviewDecorations to emit
                                     block decorations/widgets for lists, blockquotes, HR,
                                     fenced code, task checkboxes, tables, images; generalize
                                     reveal-on-cursor to multi-line block ranges.
components/editor/livePreview.css    MODIFY — token-based prose typography, centered measure,
                                     list/quote/code-block/table/hr/checkbox/image styles.
components/editor/docTheme.ts        NEW — EditorView.theme (transparent bg, Inter, caret/
                                     selection tokens, padding) + the minimal basicSetup
                                     object + the token HighlightStyle (inline + code tokens).
components/editor/imageResolver.ts   NEW — makeImageResolver(host): (target)=>url|null.
                                     remote/data pass-through; local relative via host.assetUrl.
components/editor/checkboxToggle.ts  NEW — pure helper: given doc + marker position, compute
                                     the {from,to,insert} change that flips [ ]↔[x].
components/editor/widgets/
  taskCheckboxWidget.ts              NEW — clickable checkbox; mousedown → onToggle(pos).
  imageWidget.ts                     NEW — <img src> (block if alone on line, else inline).
  tableWidget.ts                     NEW — parsed GFM table → HTML <table>.
  hrWidget.ts                        NEW — <hr>.
components/editor/wikilinkWidget.ts  UNCHANGED — pattern reference for the new widgets.
```

Host interface addition (vendored host contract):

```
CairnHost.assetUrl(relPath: string): string
  TauriHost  → convertFileSrc(join(cairnRoot, relPath))   // asset protocol, sync, no IPC
  MockHost   → a small inline `data:image/...;base64,…` (so browser/e2e render a real <img>)
```

**Separation of concerns:** *which* decorations to emit for a given `EditorState`
stays in exported pure functions (testable without a live DOM). *Widget DOM* and
*CodeMirror rendering* are covered by e2e (CM/widgets don't fully render under
jsdom — the established pattern).

---

## 3. Rendering model

All rendering is driven off the Lezer syntax tree (GFM is already active — the
existing `Strikethrough` handling proves it), in a single
`buildLivePreviewDecorations(state, opts)` pass.

Two decoration kinds:
- **Always-on** (never conceal characters; stable while editing): prose
  typography, list indent, blockquote left-bar, code-block background, heading/
  inline mark classes.
- **Cursor-gated** (hide marker / replace with widget when the selection does NOT
  touch the element; reveal raw when it does): heading `#`, inline marks, list
  marks, quote marks, fence lines, HR, checkbox, table, image, wikilink/link
  syntax.

### Per-element plan

| Element | Lezer node(s) | Decoration |
|---|---|---|
| Headings 1–6 | `ATXHeading1–6`, `HeaderMark` | mark class `cm-lp-hN`; hide `#`+space (gated). *(existing)* |
| Bold/italic/strike | `StrongEmphasis`/`Emphasis`/`Strikethrough` + `EmphasisMark`/`StrikethroughMark` | mark class; hide marks (gated). *(existing)* |
| Inline code | `InlineCode`/`CodeMark` | mark class `cm-lp-code`; hide backticks (gated). *(existing)* |
| Links | `Link`/`LinkMark` | style; hide `[`…`](url)` keeping text (gated). *(existing)* |
| Wikilinks | `[[…]]` text scan | replace with `WikilinkWidget` (gated). *(existing)* |
| Bulleted/numbered lists | `BulletList`/`OrderedList`/`ListItem`/`ListMark` | line-deco `cm-lp-li` (indent/spacing); replace `-`/`*` `ListMark` with a `•` widget; ordered lists keep `1.` (gated for the mark). |
| Blockquote | `Blockquote`/`QuoteMark` | line-deco `cm-lp-quote` (left bar, muted); hide `> ` `QuoteMark` (gated). |
| Horizontal rule | `HorizontalRule` | replace the rule line with `HrWidget` → `<hr>` (gated; raw `---` reveals on cursor). |
| Fenced code | `FencedCode`/`CodeMark`/`CodeInfo`/`CodeText` | line-deco `cm-lp-codeblock` (background, monospace, border) over the body; syntax highlight via markdown `codeLanguages` + the token `HighlightStyle`; hide the opening/closing fence + lang-info lines (gated over the block line-range). |
| Task checkbox | GFM `Task`/`TaskMarker` (inside `ListItem`) | replace `[ ]`/`[x]` with `TaskCheckboxWidget` (checked/unchecked); mousedown → `checkboxToggle` doc change. List bullet for the task item is hidden. (gated). |
| Table | GFM `Table`/`TableRow`/`TableCell`/`TableDelimiter` | block widget `TableWidget` spanning the `Table` range → parsed HTML `<table>` (token-styled). (gated over the block line-range: cursor inside → raw pipe text reveals.) |
| Image | `Image` (`![alt](url)`) | replace with `ImageWidget`; `src` via `imageResolver`; block `<img>` if the image is alone on its line, else inline. Unresolved local → small filename chip. (gated.) |

Syntax highlighting requires a `HighlightStyle` in the extension set (we dropped
`basicSetup`'s default). It lives in `docTheme.ts`, mapping Lezer highlight tags
to graphite-token colors, and colors both inline prose and fenced-code tokens.

### Reveal-on-cursor generalization

`selectionTouches(state, from, to)` already gates the existing inline elements.
UI‑3 generalizes it:
- **Inline / single-line** (list mark, quote mark, checkbox, image, inline marks):
  touch = selection intersects the node range.
- **Block-level elements** (fenced code, table, HR, block image — single- or
  multi-line): touch = selection intersects the element's **line range**
  (`state.doc.lineAt(from)` … `lineAt(to)`); when touched, the widget/hide is
  skipped and the raw markdown text shows for editing; re-renders on leave. (HR
  and a one-line block image have a single-line range; the same rule applies.)

All new widgets register in `EditorView.atomicRanges` (as `WikilinkWidget`
already does) so caret motion skips cleanly over rendered widgets.

---

## 4. Editor chrome & theme (the "document look")

- **Minimal setup:** replace `@uiw/react-codemirror`'s default `basicSetup` with
  an explicit minimal config — **no** `lineNumbers`, `foldGutter`,
  `highlightActiveLine`, `highlightActiveLineGutter`, or any gutter. Keep editing
  essentials (history, default + indent keymaps, `indentOnInput`, multi-selection,
  `drawSelection`). This object lives in `docTheme.ts`.
- **`docTheme` (`EditorView.theme`):** transparent/`bg` background (no dark code
  panel); `.cm-content` uses Inter (`font-sans`), ~14px, `line-height: 1.7`; caret
  + active selection tinted with `accent`; comfortable vertical padding; no focus
  outline; no active-line tint.
- **Centered measure:** `.cm-content` (or its wrapper) `max-width: 34em; margin:
  0 auto` → ~68ch centered column (mockup A). **Source mode** uses the same theme
  but keeps content monospace, left-aligned, full-width (raw text reads better
  untruncated). Mechanism: `Editor.tsx` puts a mode class on the CodeMirror
  container (`cm-doc-livepreview` vs `cm-doc-source`), and the CSS scopes the
  centered measure + Inter prose font to `cm-doc-livepreview` only; source keeps
  monospace + full width.
- **Header:** the existing path label + Source/Live-Preview toggle restyle to
  tokens (muted path text, ghost `Button`), sitting quietly above the document.

---

## 5. Testing

- **Unit (Vitest):**
  - `buildLivePreviewDecorations` against `EditorState` fixtures: for each new
    element, assert the expected decorations/widgets are emitted, and that a
    selection touching the element flips it to raw (no replace/hide). Multi-line
    block line-range intersection is asserted for code/table/HR.
  - `makeImageResolver`: remote pass-through, `data:` pass-through, local relative
    → `host.assetUrl`, unresolved → null/chip.
  - `checkboxToggle`: given a doc + marker position, returns the correct
    `{from,to,insert}` flipping `[ ]`↔`[x]`.
- **e2e (Playwright):** extend `skeleton.spec.ts` with a fixture note exercising
  every element. Assert a rendered `<table>`, `<img>`, `<hr>`, a styled code
  block, a `•` bullet, and a checkbox render; click a checkbox and assert the
  source toggled (`- [ ]` → `- [x]`); move the caret into the table and assert raw
  pipe characters reveal. Keep all existing assertions green.
- All green on the mock; Tauri/desktop unaffected.

---

## 6. Files & dependencies

| File | Change |
|---|---|
| `web/src/components/editor/docTheme.ts` (+ test) | **New.** Minimal setup + theme + HighlightStyle. |
| `web/src/components/editor/imageResolver.ts` (+ test) | **New.** |
| `web/src/components/editor/checkboxToggle.ts` (+ test) | **New.** |
| `web/src/components/editor/widgets/taskCheckboxWidget.ts` | **New.** |
| `web/src/components/editor/widgets/imageWidget.ts` | **New.** |
| `web/src/components/editor/widgets/tableWidget.ts` | **New.** |
| `web/src/components/editor/widgets/hrWidget.ts` | **New.** |
| `web/src/components/editor/livePreview.ts` (+ test) | **Modify.** Block decorations + widgets + block reveal. |
| `web/src/components/editor/livePreview.css` | **Modify.** Prose typography, measure, block styles. |
| `web/src/components/Editor.tsx` | **Modify.** Minimal setup + docTheme + header restyle + resolver wiring. |
| host contract + `TauriHost`/`MockHost` | **Modify.** Add `assetUrl(relPath)`. |
| `web/e2e/skeleton.spec.ts` (+ fixture) | **Modify.** Full-element fixture note + assertions. |
| `web/package.json` | **Modify.** Add `@codemirror/language-data`. |

---

## 7. Plan staging

One spec; the plan executes in independently-shippable, reviewable slices:

1. **Foundation** — minimal setup, `docTheme.ts` (theme + HighlightStyle), prose
   CSS + centered measure. (Existing headings/marks/links/wikilinks get the new
   look; nothing else changes yet.)
2. **Simple blocks** — lists (bullets/indent), blockquote, HR.
3. **Fenced code** — block styling + `codeLanguages` syntax highlighting.
4. **Task checkboxes** — `checkboxToggle` + clickable widget.
5. **Images** — `imageResolver` + host `assetUrl` + `ImageWidget`.
6. **Tables** — `TableWidget` + block reveal-on-edit.
7. **Polish + e2e** — full fixture note, e2e assertions, manual visual check.

---

## 8. Risks

- **Block widgets + reveal-on-edit** (tables, block images, HR) are the fiddliest:
  the line-range intersection must be exact so the caret is never trapped behind a
  widget and editing always reveals raw text. Covered by the staged tasks +
  explicit e2e (caret-into-table reveals pipes).
- **`convertFileSrc` is Tauri-only:** the Mock `assetUrl` returns a `data:` image
  so browser/e2e keep working without a desktop runtime.
- **Atomic ranges:** every new widget must be registered in
  `EditorView.atomicRanges` (as wikilinks are) or caret navigation breaks across
  rendered widgets.
- **Syntax highlighting without basicSetup:** dropping `basicSetup` removes its
  default highlight style, so the token `HighlightStyle` must be added explicitly
  or all code (inline + fenced) renders uncolored.
- **Scope:** UI‑3 is large for one cycle but is a single subsystem (the editor's
  live-preview); the staged plan keeps each slice small and green. Resist leaking
  deferred items (§1 non-goals).
