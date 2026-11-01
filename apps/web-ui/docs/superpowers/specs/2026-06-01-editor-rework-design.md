# Cairn Web UI — Editor Rework (Rendered-Default) Design Spec

**Date:** 2026-06-01
**Status:** approved, ready for implementation planning
**Sub-project:** Phase 3 of [`docs/roadmap.md`](../../roadmap.md) (Editor depth)
**Builds on:** the walking-skeleton UI + Tauri desktop (Phases 1–2). Runs on both
the mock (browser/tests) and the real Tauri backend unchanged.

---

## 1. Purpose

Make the note editor open in a **beautiful, readable rendered markdown view by
default** (GitHub-README style), with **markdown source editing as a secondary
mode**. Today the editor only ever shows *source* (a CodeMirror "rich" mode and
a plain "raw" textarea); there is no rendered view. This rework adds the rendered
view, makes it the default, consolidates source editing into CodeMirror, and
makes the rendered view navigable via clickable `[[wikilinks]]`.

### Non-goals (deferred)

- **Frontmatter handling** in the rendered view (hiding/▸ a properties block). Out
  of scope this cycle; a note beginning with a `---` YAML block renders that block
  roughly as-is. No current note/fixture has frontmatter, so it isn't visible today.
- WYSIWYG / live-preview editing (editing directly on the rendered document).
  Considered and rejected for this cycle — it needs a rich-text engine
  (ProseMirror/Tiptap/Milkdown) with markdown round-trip fidelity risk. The
  rendered⇄source toggle is the chosen model.
- Side-by-side split view; click-into-rendered-text-to-edit (explicit toggle only).
- `[[wikilink]]` autocomplete in the source editor (a later editor-depth item).

---

## 2. Locked decisions (from brainstorming)

| Decision | Choice |
|---|---|
| Editing model | **Rendered (default) ⇄ Source** toggle. Rendered is a read view; edit in Source, toggle back. Markdown source stays the single source of truth. |
| Source editor | **CodeMirror only.** Drop the plain `<textarea>` mode. Two modes total: `rendered`, `source`. |
| Rendered scope | **GFM** (remark-gfm), **Tailwind Typography** styling, **clickable `[[wikilinks]]`** (resolved-by-stem; unresolved styled distinctly), **fenced-code syntax highlighting**. |
| Frontmatter | Deferred (rendered roughly as-is for now). |

---

## 3. Architecture

```
App.tsx
  passes activeContents (live buffer) + editorMode + notePaths + openNote → Editor
Editor.tsx  (toggle + autosave wiring)
  editorMode === "rendered"  → <MarkdownView contents notePaths onOpenNote />   (read view)
  editorMode === "source"    → <CodeMirror value onChange={editBuffer} />        (edit)
  toggle button / ⌘E flips settings.editorMode
MarkdownView.tsx  (pure, focused)
  react-markdown( contents )
    + remark-gfm
    + remark-wiki-link (parse [[target]])
    + rehype-highlight (code blocks)
    custom `a` renderer: wikilink → onOpenNote(resolve(target)); resolved vs unresolved styling
  wrapper: <div class="prose prose-invert max-w-none …">
```

- **Both modes read the same `activeContents` buffer.** Switching to Rendered
  shows the live (possibly unsaved) buffer; switching to Source edits it. Autosave
  fires only from Source edits (unchanged debounced `write_note`).
- Rendered view is **read-only** (a display); editing happens in Source.
- `MarkdownView` depends only on its props (`contents`, `notePaths`, `onOpenNote`)
  — no store coupling, independently testable.

---

## 4. Rendering & wikilinks

- **Library stack:** `react-markdown` + `remark-gfm` + `remark-wiki-link` +
  `rehype-highlight`. Styling via `@tailwindcss/typography` (`prose prose-invert
  max-w-none`), plus a highlight.js theme CSS (github-dark) imported once.
- **Wikilinks:** `remark-wiki-link` parses `[[target]]` / `[[target|alias]]`.
  Resolution is **by stem** (filename without dir/`.md`), reusing
  `web/src/client/wikilink.ts`'s `stem()` — consistent with the engine and the
  backlinks panel. Configure the plugin with the set of existing note stems
  (derived from `notePaths`) so it can mark links as existing vs missing.
- **Click behavior:** a custom `a` component in `react-markdown` detects wikilink
  anchors (by the plugin's class/`data-` marker), and on click calls
  `onOpenNote(resolvedPath)` and prevents default navigation. Non-wikilink links
  (normal `http(s)` links) keep default behavior (open as usual).
- **Resolved vs unresolved:** existing-target wikilinks render as normal links;
  unresolved targets render in a distinct muted style and either do nothing on
  click or are non-interactive (no note to open).
- **Code blocks:** `rehype-highlight` adds token classes; the github-dark theme CSS
  styles them. Inline code is styled by the typography preset.

---

## 5. State / settings changes

- In `web/src/store/store.ts`: the `Settings.editorMode` type changes from
  `"rich" | "raw"` to `"rendered" | "source"`, and `DEFAULT_SETTINGS.editorMode`
  becomes `"rendered"`. The existing toggle action (via `setSettings`) flips
  between the two.
- `web/src/app/App.tsx`: pass `notePaths` and the `openNote` action into `Editor`
  so `MarkdownView` can resolve + open wikilinks. The mode-toggle handler flips
  `editorMode` between `"rendered"` and `"source"`.
- No contract/transport/store-logic changes beyond the rename + new props — this
  is a presentation-layer rework.

---

## 6. Components & files

| File | Change |
|---|---|
| `web/src/components/MarkdownView.tsx` | **New.** Rendered markdown view (react-markdown + plugins + wikilink `a` renderer + prose wrapper). Props: `contents`, `notePaths`, `onOpenNote`. |
| `web/src/components/MarkdownView.test.tsx` | **New.** Unit tests (GFM render, wikilink click → onOpenNote, unresolved styling). |
| `web/src/components/Editor.tsx` | **Modify.** Toggle Rendered⇄Source; render `MarkdownView` or CodeMirror; drop the textarea; keep autosave/buffer wiring; toggle labels ("Edit"/"Done" or "Source"/"Rendered"). New props: `notePaths`, `onOpenNote`. |
| `web/src/components/Editor.test.tsx` | **Modify.** Assert Rendered shows the rendered view, Source shows CodeMirror, toggle flips. |
| `web/src/store/store.ts` | **Modify.** `editorMode` values → `"rendered" \| "source"`, default `"rendered"`. |
| `web/src/store/store.test.ts` | **Modify if needed** (DEFAULT_SETTINGS editorMode assertion). |
| `web/src/app/App.tsx` | **Modify.** Pass `notePaths`/`openNote` to `Editor`; toggle handler uses new mode values. |
| `web/tailwind.config.ts` | **Modify.** Add `@tailwindcss/typography` plugin. |
| `web/src/index.css` (or a dedicated import) | **Modify.** Import the highlight.js github-dark theme CSS. |
| `web/package.json` | **Modify.** Add `react-markdown`, `remark-gfm`, `remark-wiki-link`, `rehype-highlight`, `@tailwindcss/typography`, `highlight.js`. |
| `web/e2e/skeleton.spec.ts` | **Modify.** Replace textarea typing with: toggle to Source, type into CodeMirror (`.cm-content`); optionally assert the rendered view shows a heading. |

---

## 7. Testing

- **Unit (Vitest + Testing Library):**
  - `MarkdownView`: renders a heading/bold/list/table (GFM); renders inline + fenced
    code; a `[[ideas]]` with `ideas.md` in `notePaths` renders a clickable element
    that calls `onOpenNote("ideas.md")`; a `[[missing]]` renders in the unresolved
    style and does not call `onOpenNote`.
  - `Editor`: `rendered` mode renders `MarkdownView` (no CodeMirror/textarea);
    `source` mode renders CodeMirror; the toggle button flips mode.
  - Store: `DEFAULT_SETTINGS.editorMode === "rendered"`.
- **Component caveat:** `react-markdown` works under jsdom; `rehype-highlight`
  runs synchronously — fine in tests. CodeMirror stays exercised via e2e (as today).
- **e2e (Playwright):** the full loop still passes — adjust the edit step to toggle
  into Source and type into CodeMirror, and (optionally) assert the rendered view
  renders the typed markdown as HTML after toggling back. Keep all other assertions.
- All must stay green on the **mock** backend (`isTauri()` false), and the desktop
  app continues to work unchanged (this is presentation-only).

---

## 8. Risks

- **`remark-wiki-link` API/version:** confirm the installed version's options
  (`pageResolver`, `hrefTemplate`, `permalinks`, class names for existing/missing)
  and how it marks resolved vs missing, then wire the custom `a` renderer to that.
  If its behavior is awkward, fall back to a small custom remark plugin (the
  `[[…]]` grammar is simple and our `extractLinks`/`stem` already encode the rules).
- **Click interception:** ensure the custom `a` renderer reliably distinguishes
  wikilink anchors from ordinary links and calls `onOpenNote` (preventDefault)
  only for wikilinks.
- **e2e + CodeMirror typing:** typing into CodeMirror's contenteditable in
  Playwright needs click-then-`keyboard.type` (not `fill`); budget for that.
- **Bundle size:** highlight.js + react-markdown add weight; acceptable (already a
  chunk-size advisory exists). Use `rehype-highlight` (sync) over Shiki (async) to
  keep it simple.
- **Sanitization:** `react-markdown` does not execute raw HTML by default (no
  `rehype-raw`), so untrusted note content can't inject scripts — keep it that way
  (do not add `rehype-raw`).

---

## 9. Build order (for the plan)

1. Add deps + `@tailwindcss/typography` to `tailwind.config.ts`; import highlight.js theme.
2. `MarkdownView` (TDD): GFM render + prose wrapper.
3. `MarkdownView` wikilinks (TDD): clickable resolved links → `onOpenNote`; unresolved styling.
4. Store: rename `editorMode` → `"rendered" | "source"`, default `"rendered"` (+ test).
5. `Editor` (TDD): toggle Rendered⇄Source, drop textarea, render `MarkdownView`/CodeMirror, new props.
6. `App`: pass `notePaths`/`openNote`; wire toggle.
7. Update the Playwright e2e (toggle-to-source + CodeMirror typing; rendered assertion).
8. Full gate: `pnpm test`/`typecheck`/`lint`/`format:check`/`build` + `pnpm e2e`; confirm `cargo` side unaffected.
```
