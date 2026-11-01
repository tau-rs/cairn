# Editor: frontmatter rendering + wikilink autocomplete

Date: 2026-06-12
Status: Approved

Two leftover Phase 3 (editor depth) items in the CodeMirror live-preview editor,
done together because both touch the one editor subsystem and would otherwise
self-conflict:

1. **Frontmatter rendering** — render leading YAML frontmatter as a styled block
   in the rendered document instead of raw text.
2. **Wikilink autocomplete** — a CodeMirror completion source suggesting note
   stems while typing inside `[[ ... ]]`.

## Context

Key files:

- `web/src/components/editor/livePreview.ts` — pure `buildLivePreviewDecorations`
  builds the live-preview `DecorationSet`. Uses two strategies: tree iteration
  (`syntaxTree(state).iterate`) for parser-recognized nodes, and text-scanning
  (`WIKILINK`, `IMAGE` regexes) for tokens the markdown parser ignores.
- `web/src/components/editor/livePreview.css` — live-preview styling classes
  (`cm-lp-*`).
- `web/src/components/editor/wikilinkWidget.ts` and `widgets/` — widget classes.
- `web/src/components/Editor.tsx` — wires extensions; already receives
  `notePaths: string[]` and builds a `stem`-keyed `resolve` map.
- `web/src/client/wikilink.ts` — `stem(path)` helper (filename without dir/`.md`).

Verified facts (against installed deps):

- The default markdown parser does **not** recognize YAML frontmatter. Parsing
  `---\ntitle: x\n---` yields `HorizontalRule` (first `---`) + `SetextHeading2`
  (the content line, with the closing `---` read as a setext underline). So
  frontmatter must be detected by text-scan, and tree nodes inside the block must
  be suppressed to avoid stray heading/HR decorations.
- `@codemirror/autocomplete` is not hoisted to the top level under pnpm, so it
  must be added as an explicit dependency to import from it.

## Feature 1 — Frontmatter rendering (live-preview only)

### Detection

Frontmatter is only valid at the very top of the document. Detection (pure, in
`buildLivePreviewDecorations`):

- If `doc.line(1)` is exactly `---` (strict equality, no trim — matching the YAML
  fence convention), scan downward for the next line whose text is exactly `---`
  (closing fence).
- The block spans line 1 through that closing-fence line: `[blockStart, blockEnd]`
  as document offsets.
- No closing fence found → not frontmatter; emit nothing.
- A `---` anywhere but line 1 is left untouched (it stays a `HorizontalRule`,
  handled by existing code).

### Decorations

Off-cursor (selection does **not** touch `[blockStart, blockEnd]`):

- Every line in the block gets `Decoration.line({ class: "cm-lp-frontmatter" })`.
- The opening fence line additionally gets `cm-lp-frontmatter-first`; the closing
  fence line additionally gets `cm-lp-frontmatter-last` (CSS rounds the box
  top/bottom and draws the top/bottom border there).
- The two `---` fence lines have their text hidden via `Decoration.replace({})`
  over the line's `from..to`, leaving slim empty lines that read as the box's
  top/bottom padding. The key/value content lines keep their raw text, styled.

On-cursor (selection touches the block): emit nothing for the block → raw,
plainly editable text. Same reveal rule as blockquote / fenced code.

### Conflict guard

Compute the frontmatter range before the tree iteration. In the iterate `enter`
callback, `return` early for any node whose range falls within
`[blockStart, blockEnd]`, so the mis-parsed `HorizontalRule` / `SetextHeading2`
nodes inside the block do not also emit decorations.

### CSS

`livePreview.css`: `.cm-lp-frontmatter` draws the box body (subtle background,
left+right border, muted monospace text); `.cm-lp-frontmatter-first` and
`.cm-lp-frontmatter-last` add the top/bottom border and rounded corners.

### Tests (`livePreview.test.ts`)

- Cursor outside the block → content lines carry `cm-lp-frontmatter`; fence lines
  are hidden (replace decoration).
- Cursor inside the block → no frontmatter decorations (raw).
- `---` not on line 1 (e.g. after a paragraph) → no frontmatter decorations; the
  existing HR handling is unaffected.
- Unclosed frontmatter (line 1 `---`, no second `---`) → no frontmatter
  decorations.
- First/last fence lines carry `cm-lp-frontmatter-first` / `-last`.
- Conflict guard: a node inside the block (the setext-mis-parse) emits no heading
  class.

## Feature 2 — Wikilink autocomplete (both modes)

### New file: `web/src/components/editor/wikilinkComplete.ts`

A pure function plus a thin CodeMirror `CompletionSource` wrapper:

```
wikilinkCompletionState(
  textBefore: string,  // current line text up to the cursor
  stems: string[],     // note stems
): { from: number; stems: string[] } | null
```

`from` is returned as an offset **relative to the start of `textBefore`** (i.e.
the line); the wrapper adds the line's document offset and maps the returned
`stems` to `Completion` options. `textAfter` is not needed for the fire decision —
the closing-`]]` logic lives in `wikilinkInsert(stem, textAfter)`, applied only
when a completion is picked.

### Fire rule

Match `/\[\[([^\]|]*)$/` against `textBeforeCursor` — an open `[[` followed by a
partial containing no `]` and no `|`, anchored at the cursor. No match → `null`.
This single rule covers all the don't-fire cases:

- No `[[` before the cursor.
- The link is already closed (`]]` between `[[` and the cursor breaks the match
  via the `]` exclusion).
- The cursor is in the alias part (a `|` between `[[` and the cursor breaks the
  match via the `|` exclusion).

Empty partial (cursor right after `[[`) → match with empty capture → suggest all
stems.

### Options

- Dedupe `stems` (note paths can share a stem across folders).
- Filter by the partial, case-insensitive (substring match), partial empty → all.
- `from` = offset of the first partial character (immediately after `[[`).
- Each `Completion`: `label` = stem; `apply` is a function that inserts the stem
  and, **only if** the document text immediately after the replaced range is not
  already `]]`, appends `]]`; the cursor is placed after the closing `]]`.

### Wiring (`Editor.tsx`)

- Build the source from `props.notePaths` (memoized on `notePaths`).
- Register via `markdownLanguage.data.of({ autocomplete: source })` and add it to
  the shared `common` extensions array → active in **both** live-preview and
  source modes. This hooks into the `autocompletion` already enabled by
  basicSetup (no second autocompletion instance).
- Add `@codemirror/autocomplete` to `web/package.json` dependencies.

### Tests (`wikilinkComplete.test.ts`, pure — no EditorView)

- Fires on `[[ide` → options filtered to matching stems; `from` at the partial
  start (the `i`).
- Empty partial (`[[`) → all stems.
- After `|` (`[[ideas|al`) → `null`.
- No `[[` → `null`.
- Already closed (`[[ideas]]` with cursor after `]]`) → `null`.
- Case-insensitive substring filter.
- Stem dedupe (two paths, same stem → one option).
- `apply`: appends `]]` when the following text is not `]]`; reuses the existing
  `]]` when present (no `]]]]`); cursor ends after the close.

## Out of scope (YAGNI)

- Rendered key/value properties widget for frontmatter (chose the styled raw
  block instead).
- YAML parsing / validation of frontmatter.
- Autocomplete for the alias part, headings (`#`), or block refs.
- Creating a new note from an unresolved `[[name]]`.

## Risk / isolation

Confined to the editor subsystem; no shared mutable state between the two
features. Low contention with other tracks. Full `just` gate, PR `--base main`,
merge via queue after Track A.
