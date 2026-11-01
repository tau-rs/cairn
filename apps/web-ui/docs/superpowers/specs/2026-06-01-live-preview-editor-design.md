# Cairn Web UI — Obsidian-style Live Preview Editor Design Spec

**Date:** 2026-06-01
**Status:** approved, ready for implementation planning
**Sub-project:** editor enhancement on Phase 3 (editor depth) in
[`docs/roadmap.md`](../../roadmap.md)
**Builds on:** the editor rework (Phase 3). Supersedes the read-only rendered
view. Runs on mock (browser/tests) and real Tauri backend unchanged.

---

## 1. Purpose

Make the note **editable while it looks rendered** — Obsidian "Live Preview".
Today the default is a read-only rendered view (`MarkdownView`, react-markdown)
with a separate "Edit source" toggle. This replaces the rendered view with a
**CodeMirror 6 live-preview mode**: a single editable surface that styles
markdown inline, hides syntax markers, and *reveals* the raw markdown around the
cursor (selection-aware). The buffer stays literal markdown — byte-perfect for
git, no re-serialization.

### Non-goals (deferred)

- Block-level rich rendering in live preview: fenced **code blocks** (syntax
  highlighting widget), **tables** (HTML widget), **images/embeds**, and
  inline-styled **lists / blockquotes / horizontal rules**. These remain
  editable plain text in live preview for now; rich widgets are a follow-up.
- A separate read-only "Reading" view (we chose the 2-mode model). It can be
  re-added later as a third mode if wanted.
- ProseMirror/Milkdown WYSIWYG (rejected — re-serializes markdown, harming git
  diffs).
- `[[wikilink]]` autocomplete while typing (separate later item).

---

## 2. Locked decisions (from brainstorming + research)

| Decision | Choice |
|---|---|
| Approach | **CodeMirror 6 + a decoration `ViewPlugin`** (the actual Obsidian mechanism): style inline, hide markers, selection-aware reveal. Buffer = literal markdown. |
| Mode model | **2 modes: `livepreview` (default) + `source` (raw).** The read-only `MarkdownView` (react-markdown) is **removed**. |
| v1 element scope | headings, bold, italic, strikethrough, inline code, standard links, `[[wikilinks]]`. Code blocks / tables / images / list & blockquote styling **deferred**. |
| Wikilinks | clickable widget resolved by **stem** → `onOpenNote(path)`; unresolved muted; raw `[[…]]` revealed when the cursor enters. |

**Why CodeMirror, not ProseMirror:** the file stays byte-for-byte as typed (no
re-serialization), it's how Obsidian does it, and we already use CodeMirror 6.
([Obsidian Editor docs](https://docs.obsidian.md/Plugins/Editor/Editor);
reference: kenforthewin/atomic-editor.)

---

## 3. Architecture

```
Editor.tsx
  mode "livepreview" → <CodeMirror extensions=[markdown(), livePreview({ resolve, onOpenNote })]>
  mode "source"      → <CodeMirror extensions=[markdown()]>            (raw, no decorations)
  (same activeContents buffer + onChange autosave for both modes)

editor/livePreview.ts
  livePreview(opts) -> CM6 Extension (a ViewPlugin)
    ViewPlugin recomputes decorations on doc/selection change via:
  buildLivePreviewDecorations(state, opts) -> DecorationSet     ← PURE, unit-tested
    - walk syntaxTree (from @codemirror/lang-markdown):
        heading/strong/emphasis/strikethrough/inlineCode → Decoration.mark(class)
        their syntax-marker child ranges → Decoration.replace (HIDE) unless the
        selection intersects the element range (then leave raw → "reveal")
        link: hide the `(url)`+brackets off-cursor, style the text
    - regex pass for [[target]] / [[target|alias]] (parser ignores them):
        Decoration.replace with a wikilink WidgetType (resolved/unresolved),
        unless selection intersects → reveal raw
  editor/wikilinkWidget.ts  WidgetType rendering a clickable span → onOpenNote

store.ts        editorMode: "livepreview" | "source"; default "livepreview"
App.tsx         toggle label "Source" / "Live Preview"; passes notePaths + openNote into Editor
```

- `buildLivePreviewDecorations` is **pure** (`EditorState` → `DecorationSet`),
  so it is unit-testable headlessly (CM6 state works in jsdom; views do not).
- The `ViewPlugin` is the thin glue that calls the builder on `update`.
- Both modes share the live `activeContents` buffer + the existing debounced
  autosave (`onChange` → `editBuffer`). Live preview is fully editable.

---

## 4. The live-preview extension

`livePreview(opts: { resolve: (target: string) => string | null; onOpenNote: (path: string) => void }): Extension`

Returns a `ViewPlugin` holding a `DecorationSet`, rebuilt on `docChanged` or
`selectionSet` by calling `buildLivePreviewDecorations(view.state, opts)`.

`buildLivePreviewDecorations(state, opts)`:
- Iterate `syntaxTree(state)` over the document. For each supported node:
  - **ATXHeading1..6**: mark the heading content with `cm-lp-h1..h6`; the
    leading `#`+space marker child → hide (`Decoration.replace({})`) unless the
    selection overlaps the heading line.
  - **StrongEmphasis / Emphasis / Strikethrough**: mark inner text
    (`cm-lp-strong` / `-em` / `-strike`); hide the `**`/`*`/`~~` marker children
    unless selection overlaps the node.
  - **InlineCode**: mark (`cm-lp-code`); hide the backtick markers unless
    selection overlaps.
  - **Link** (standard `[text](url)`): mark the link text (`cm-lp-link`); hide the
    `[` `]` `(url)` parts unless selection overlaps. **v1 styles them but does NOT
    make them click-to-open** (only `[[wikilinks]]` are clickable in v1);
    opening external links is a deferred follow-up.
- **Wikilinks** (separate regex scan, since the markdown parser doesn't know
  `[[…]]`): for each `[[target|alias]]` not inside code, if the selection does
  **not** overlap it, replace it with a `WikilinkWidget(label, target,
  resolved)`; else leave raw. `resolve(target)` (stem-based) decides
  resolved/unresolved (class + whether the widget opens a note).
- "Selection-aware": a marker/element is revealed (not hidden/replaced) when
  `state.selection.main` (or any range) intersects its `[from,to)`.

`Decoration` ordering and `RangeSetBuilder` must add ranges in ascending order;
the builder sorts/structures accordingly. Replaced wikilink widgets use
`Decoration.replace({ widget, … })`; the plugin provides `atomicRanges` so the
cursor steps over a rendered widget cleanly.

`WikilinkWidget` (in `editor/wikilinkWidget.ts`) renders a `<span>`/`<a>` with a
class (`cm-lp-wikilink resolved|unresolved`); a CM6 DOM event handler (or the
widget's own listener) calls `opts.onOpenNote(target)` for resolved links and
`preventDefault`s.

CSS (e.g. `editor/livePreview.css`, imported once): heading sizes, bold/italic,
inline-code chip, link/wikilink colors, resolved vs unresolved styling — dark
theme consistent with the app.

---

## 5. State & wiring changes

- `web/src/store/store.ts`: `Settings.editorMode` becomes `"livepreview" |
  "source"`; `DEFAULT_SETTINGS.editorMode = "livepreview"`.
- `web/src/components/Editor.tsx`: render a CodeMirror in both modes (drop the
  `MarkdownView` branch). In `livepreview` mode add the `livePreview` extension
  (built from `notePaths` → a stem `resolve` + `onOpenNote`); in `source` mode
  use plain `markdown()`. Toggle button flips the two modes ("Source" ↔ "Live
  Preview"). Keep the existing autosave `onChange` wiring; `Editor` keeps its
  `notePaths`/`onOpenNote` props (already present).
- `web/src/app/App.tsx`: unchanged wiring except the toggle now flips
  `livepreview`/`source` (it already passes `notePaths` + `openNote`).
- **Remove** `web/src/components/MarkdownView.tsx`, `MarkdownView.test.tsx`,
  `remarkWikiLink.ts` (dead once the rendered view is gone), and drop the
  now-unused deps `react-markdown`, `remark-gfm`, `rehype-highlight` from
  `package.json` (and the highlight.js theme import in `main.tsx` if it was only
  for the rendered view — keep `highlight.js` only if still referenced).

---

## 6. Files & dependencies

| File | Change |
|---|---|
| `web/src/components/editor/livePreview.ts` | **New.** `livePreview(opts)` ViewPlugin + pure `buildLivePreviewDecorations(state, opts)`. |
| `web/src/components/editor/livePreview.test.ts` | **New.** Headless `EditorState` decoration tests. |
| `web/src/components/editor/wikilinkWidget.ts` | **New.** `WidgetType` for clickable wikilinks. |
| `web/src/components/editor/livePreview.css` | **New.** Live-preview element styles. |
| `web/src/components/Editor.tsx` | **Modify.** livepreview/source CodeMirror; drop MarkdownView. |
| `web/src/components/Editor.test.tsx` | **Modify.** Assert both modes render CodeMirror; toggle flips; (no jsdom assertion on live-preview decorations — e2e covers that). |
| `web/src/store/store.ts` | **Modify.** `editorMode` `"livepreview" \| "source"`, default `"livepreview"`. |
| `web/src/store/store.test.ts` | **Modify.** default-editorMode assertion. |
| `web/src/app/App.tsx` | **Modify.** toggle label/values. |
| `web/src/main.tsx` | **Modify.** import `livePreview.css`; remove the highlight.js theme import if now unused. |
| `web/e2e/skeleton.spec.ts` | **Modify.** edit-in-live-preview flow + a wikilink-click test. |
| `web/package.json` | **Modify.** add `@codemirror/view`, `@codemirror/state`, `@codemirror/language`; remove `react-markdown`, `remark-gfm`, `rehype-highlight`. |
| **Remove** | `MarkdownView.tsx`/`.test.tsx`, `remarkWikiLink.ts`. |

(`@codemirror/*` are present transitively via `@uiw/react-codemirror` +
`@codemirror/lang-markdown`; add them explicitly since we import from them
directly.)

---

## 7. Testing

- **Unit (Vitest, headless CodeMirror state):** `buildLivePreviewDecorations`
  via `EditorState.create({ doc, selection, extensions: [markdown()] })`:
  - `# Title` → the heading content range gets `cm-lp-h1`; the `# ` marker is
    hidden when the selection is elsewhere, and **not** hidden when the selection
    is on that line.
  - `a **b** c` → `b` gets `cm-lp-strong`; `**` markers hidden off-selection,
    revealed when selection overlaps.
  - `[[ideas]]` with `resolve` returning `ideas.md` → a replace decoration with a
    resolved wikilink widget when off-selection; raw when the selection overlaps;
    unresolved target → unresolved widget.
  - (Assert decoration **presence/ranges/specs**, not pixels.)
- **Component (Editor):** `livepreview` and `source` modes both mount a
  CodeMirror (`.cm-editor` present); the toggle calls `onToggleMode`. (Do not
  assert decoration rendering in jsdom — CM6 views don't lay out there.)
- **e2e (Playwright, real browser):** open a note (live preview default); a
  heading shows styled with the `#` hidden; click into the heading → the `#`
  reveals; type edits autosave; click a rendered `[[wikilink]]` → opens that
  note. Toggle to Source → raw markdown shows. Keep prior assertions (adjust the
  editor-interaction parts of the existing test to the new modes).
- All green on the **mock**; Tauri/desktop unaffected (presentation-only).

---

## 8. Risks

- **Decoration correctness/complexity:** selection-aware hide/reveal across many
  node types is the hard part. Mitigation: bounded v1 element set; the pure
  builder is unit-tested per element; e2e validates interaction. Reference the
  Obsidian/CM6 community patterns (syntaxTree walk + `Decoration.replace` for
  markers).
- **Wikilink detection:** the markdown parser ignores `[[…]]`, so a regex scan is
  used; must skip matches inside inline/fenced code (check the syntaxTree node at
  the match position, or scan only outside code ranges).
- **CodeMirror in jsdom:** only `EditorState` is testable headlessly; `EditorView`
  rendering/measurement is not — keep view-dependent behavior in e2e (same
  discipline as React Flow).
- **Atomic widgets + cursor:** rendered wikilink widgets must be atomic so cursor
  navigation/selection behaves; reveal-on-selection avoids trapping the cursor.
- **Removing react-markdown:** confirm nothing else imports `MarkdownView` /
  `remarkWikiLink` before deleting (only `Editor.tsx` uses `MarkdownView`).

---

## 9. Build order (for the plan)

1. Deps: add `@codemirror/view`/`state`/`language`; (removal of react-markdown
   stack happens with the Editor change). Build check.
2. `wikilinkWidget` + `buildLivePreviewDecorations` (TDD, headless EditorState):
   headings, bold/italic/strike, inline code, links, wikilinks; selection-aware.
3. `livePreview(opts)` ViewPlugin wrapping the builder; live-preview CSS.
4. Store: `editorMode` → `"livepreview" | "source"`, default `"livepreview"`.
5. `Editor`: both modes CodeMirror, add the extension in live preview, drop
   `MarkdownView`; update Editor tests. Remove `MarkdownView`/`remarkWikiLink`
   (+ tests) and the unused deps.
6. `App`: toggle label/values; `main.tsx` CSS imports.
7. e2e: live-preview styling + cursor reveal + wikilink click + source toggle.
8. Full gate: `pnpm test`/`typecheck`/`lint`/`format:check`/`build` + `pnpm e2e`.
```
