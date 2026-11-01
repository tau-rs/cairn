# Editor: Frontmatter Rendering + Wikilink Autocomplete Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Render leading YAML frontmatter as a styled block in the live-preview editor, and add a `[[wikilink]]` autocomplete that suggests note stems in both editor modes.

**Architecture:** Frontmatter is detected by text-scan in the pure `buildLivePreviewDecorations` (the markdown parser mis-parses it), line-classed with fences hidden off-cursor, and a range-guard suppresses the parser's stray nodes inside the block. Wikilink autocomplete is a pure fire/filter function wrapped in a CodeMirror `CompletionSource`, registered through markdown language data so it works in both modes.

**Tech Stack:** TypeScript, React 19, CodeMirror 6 (`@codemirror/view`, `@codemirror/state`, `@codemirror/lang-markdown`, `@codemirror/autocomplete`), Vitest, pnpm, `just`.

**Spec:** `docs/superpowers/specs/2026-06-12-editor-frontmatter-wikilink-autocomplete-design.md`

---

## File Structure

- **Create** `web/src/components/editor/wikilinkComplete.ts` — pure fire/filter logic (`wikilinkCompletionState`, `wikilinkInsert`) + CodeMirror `CompletionSource` (`wikilinkCompletionSource`).
- **Create** `web/src/components/editor/wikilinkComplete.test.ts` — pure unit tests for the two pure functions.
- **Modify** `web/src/components/editor/livePreview.ts` — add `frontmatterRange` helper, the in-iterate suppression guard, the frontmatter line decorations, and the wikilink/image scan guards.
- **Modify** `web/src/components/editor/livePreview.test.ts` — frontmatter test cases.
- **Modify** `web/src/components/editor/livePreview.css` — `.cm-lp-frontmatter` box styling.
- **Modify** `web/src/components/Editor.tsx` — wire the completion source into the shared `common` extensions.
- **Modify** `web/package.json` — add `@codemirror/autocomplete` dependency.

Run all commands from the repo root unless a `cd` is shown. All `pnpm` commands run inside `web/`.

---

## Task 1: Add the `@codemirror/autocomplete` dependency

**Files:**
- Modify: `web/package.json`

- [ ] **Step 1: Add the dependency**

The package is already present transitively (a dep of `@codemirror/view`), but pnpm does not hoist it, so a direct import requires it as a direct dependency. Pin to the major already in the tree:

```bash
cd web && pnpm add "@codemirror/autocomplete@^6"
```

- [ ] **Step 2: Verify it resolves**

Run: `cd web && node --input-type=module -e "import('@codemirror/autocomplete').then(m => console.log(typeof m.autocompletion))"`
Expected: prints `function`

- [ ] **Step 3: Commit**

```bash
git add web/package.json web/pnpm-lock.yaml
git commit -m "build(web): add @codemirror/autocomplete dependency"
```

---

## Task 2: Wikilink autocomplete — pure logic (TDD)

**Files:**
- Create: `web/src/components/editor/wikilinkComplete.ts`
- Test: `web/src/components/editor/wikilinkComplete.test.ts`

- [ ] **Step 1: Write the failing tests**

Create `web/src/components/editor/wikilinkComplete.test.ts`:

```typescript
import { describe, it, expect } from "vitest";
import { wikilinkCompletionState, wikilinkInsert } from "./wikilinkComplete";

const stems = ["ideas", "inbox", "journal"];

describe("wikilinkCompletionState — fire rule", () => {
  it("fires on an open [[ with a partial, from at the partial start", () => {
    const r = wikilinkCompletionState("see [[ide", stems);
    expect(r).not.toBeNull();
    expect(r!.from).toBe("see [[".length);
    expect(r!.stems).toEqual(["ideas"]);
  });

  it("suggests all stems for an empty partial (just typed [[)", () => {
    const r = wikilinkCompletionState("see [[", stems);
    expect(r).not.toBeNull();
    expect(r!.from).toBe("see [[".length);
    expect(r!.stems).toEqual(["ideas", "inbox", "journal"]);
  });

  it("does not fire when there is no [[ before the cursor", () => {
    expect(wikilinkCompletionState("just text", stems)).toBeNull();
  });

  it("does not fire in the alias part (after a |)", () => {
    expect(wikilinkCompletionState("see [[ideas|al", stems)).toBeNull();
  });

  it("does not fire once the link is closed (]] before the cursor)", () => {
    expect(wikilinkCompletionState("see [[ideas]] ", stems)).toBeNull();
  });

  it("filters case-insensitively by substring", () => {
    expect(wikilinkCompletionState("[[IN", stems)!.stems).toEqual(["inbox"]);
    expect(wikilinkCompletionState("[[na", stems)!.stems).toEqual(["journal"]);
  });

  it("dedupes stems that repeat across folders", () => {
    const r = wikilinkCompletionState("[[", ["ideas", "ideas", "inbox"]);
    expect(r!.stems).toEqual(["ideas", "inbox"]);
  });
});

describe("wikilinkInsert — closing bracket handling", () => {
  it("appends ]] when the following text is not already ]]", () => {
    expect(wikilinkInsert("ideas", "")).toBe("ideas]]");
    expect(wikilinkInsert("ideas", " rest")).toBe("ideas]]");
  });

  it("reuses an existing ]] (no doubling)", () => {
    expect(wikilinkInsert("ideas", "]]")).toBe("ideas");
    expect(wikilinkInsert("ideas", "]] rest")).toBe("ideas");
  });
});
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd web && pnpm test wikilinkComplete`
Expected: FAIL — `wikilinkComplete.ts` does not exist / no such exports.

- [ ] **Step 3: Write the minimal implementation**

Create `web/src/components/editor/wikilinkComplete.ts` (only the pure functions for now — the `CompletionSource` wrapper is added in Task 3):

```typescript
/** Fire rule: an open `[[` with a partial containing no `]` or `|`, anchored at
 *  the cursor. The single regex rejects the no-`[[`, already-closed, and
 *  alias-part cases. Returns the filtered+deduped stems and the offset (within
 *  `textBefore`) where the partial starts, or null when it should not fire. */
export function wikilinkCompletionState(
  textBefore: string,
  stems: string[],
): { from: number; stems: string[] } | null {
  const m = /\[\[([^\]|]*)$/.exec(textBefore);
  if (!m) return null;
  const partial = m[1];
  const from = m.index + 2; // position right after the `[[`
  const seen = new Set<string>();
  const deduped = stems.filter((s) => (seen.has(s) ? false : seen.add(s)));
  const needle = partial.toLowerCase();
  const filtered = needle
    ? deduped.filter((s) => s.toLowerCase().includes(needle))
    : deduped;
  return { from, stems: filtered };
}

/** The text to insert when a completion is applied: the stem, plus a closing
 *  `]]` unless the text right after the cursor already starts with `]]`. */
export function wikilinkInsert(stem: string, textAfter: string): string {
  return textAfter.startsWith("]]") ? stem : stem + "]]";
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd web && pnpm test wikilinkComplete`
Expected: PASS — all cases green.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/editor/wikilinkComplete.ts web/src/components/editor/wikilinkComplete.test.ts
git commit -m "feat(editor): wikilink autocomplete fire/filter logic"
```

---

## Task 3: Wikilink autocomplete — CompletionSource + wire into the editor

**Files:**
- Modify: `web/src/components/editor/wikilinkComplete.ts`
- Modify: `web/src/components/Editor.tsx`

- [ ] **Step 1: Add the CompletionSource wrapper**

Append to `web/src/components/editor/wikilinkComplete.ts` (add the import at the top of the file):

```typescript
import type {
  CompletionContext,
  CompletionResult,
  CompletionSource,
} from "@codemirror/autocomplete";
```

```typescript
/** CodeMirror completion source that suggests note stems inside `[[ ... ]]`.
 *  `getStems` is called per request so it always sees the current note list. */
export function wikilinkCompletionSource(
  getStems: () => string[],
): CompletionSource {
  return (context: CompletionContext): CompletionResult | null => {
    const line = context.state.doc.lineAt(context.pos);
    const textBefore = line.text.slice(0, context.pos - line.from);
    const state = wikilinkCompletionState(textBefore, getStems());
    if (!state) return null;
    return {
      from: line.from + state.from,
      validFor: /^[^\]|]*$/,
      options: state.stems.map((stem) => ({
        label: stem,
        type: "text",
        apply: (view, _completion, applyFrom, applyTo) => {
          const after = view.state.sliceDoc(applyTo, applyTo + 2);
          view.dispatch({
            changes: { from: applyFrom, to: applyTo, insert: wikilinkInsert(stem, after) },
            selection: { anchor: applyFrom + stem.length + 2 },
          });
        },
      })),
    };
  };
}
```

- [ ] **Step 2: Verify it type-checks**

Run: `cd web && pnpm typecheck`
Expected: PASS — no type errors.

- [ ] **Step 3: Wire the source into the editor**

In `web/src/components/Editor.tsx`, add the import next to the other editor imports (near line 6):

```typescript
import { wikilinkCompletionSource } from "./editor/wikilinkComplete";
```

Then in the `extensions` `useMemo` (currently lines ~50-90), add the language-data
extension to the shared `common` array so it is active in BOTH modes. Replace:

```typescript
    const common = [base, docTheme, docHighlightStyle, EditorView.lineWrapping];
```

with:

```typescript
    const wikilinkAutocomplete = markdownLanguage.data.of({
      autocomplete: wikilinkCompletionSource(() => props.notePaths),
    });
    const common = [
      base,
      docTheme,
      docHighlightStyle,
      EditorView.lineWrapping,
      wikilinkAutocomplete,
    ];
```

`markdownLanguage` is already imported in this file. The `extensions` memo already
re-runs when `resolve` changes (which is whenever `props.notePaths` changes), so
the closure always reads the current note list. The completion plugin itself is
provided by basicSetup (autocompletion is enabled by default and not disabled in
the `basicSetup` prop).

- [ ] **Step 4: Verify type-check and existing tests still pass**

Run: `cd web && pnpm typecheck && pnpm test`
Expected: PASS — type-check clean, all existing tests still green.

- [ ] **Step 5: Manual verification in the app**

Run: `cd web && pnpm dev`, open a note with at least two other notes present.
- Type `[[` → a dropdown of note names appears.
- Type a few letters → list filters case-insensitively.
- Pick one → inserts `[[name]]` with the cursor after `]]`; no `]]]]` when brackets already existed.
- Toggle to Source mode (top-right button) → same behavior.
- After a `|` (`[[name|`) → no dropdown.

- [ ] **Step 6: Commit**

```bash
git add web/src/components/editor/wikilinkComplete.ts web/src/components/Editor.tsx
git commit -m "feat(editor): suggest note stems while typing [[wikilinks]]"
```

---

## Task 4: Frontmatter rendering — detection + decorations (TDD)

**Files:**
- Modify: `web/src/components/editor/livePreview.ts`
- Test: `web/src/components/editor/livePreview.test.ts`

- [ ] **Step 1: Write the failing tests**

Add to `web/src/components/editor/livePreview.test.ts`, inside the existing
`describe("buildLivePreviewDecorations", ...)` block (the `decos` helper returns
`{ from, to, class, hidden, widget }`; `class` may hold several space-separated
classes, so assert with `includes`):

```typescript
  it("styles frontmatter content lines and hides the --- fences off-cursor", () => {
    const doc = "---\ntitle: x\ntags: a\n---\n\nbody";
    const ds = decos(doc, doc.indexOf("body"));
    // content lines carry the frontmatter class
    expect(
      ds.some((d) => d.class?.includes("cm-lp-frontmatter")),
    ).toBe(true);
    // opening fence (offset 0) and closing fence are hidden
    const closeFence = doc.indexOf("---", 3);
    expect(ds.some((d) => d.hidden && d.from === 0)).toBe(true);
    expect(ds.some((d) => d.hidden && d.from === closeFence)).toBe(true);
    // first/last marker classes are present
    expect(ds.some((d) => d.class?.includes("cm-lp-frontmatter-first"))).toBe(true);
    expect(ds.some((d) => d.class?.includes("cm-lp-frontmatter-last"))).toBe(true);
  });

  it("does not emit a horizontal-rule widget for the opening fence", () => {
    const doc = "---\ntitle: x\n---\n\nbody";
    const ds = decos(doc, doc.indexOf("body"));
    // without the suppression guard the first --- parses as an HR widget at 0
    expect(ds.some((d) => d.widget && d.from === 0)).toBe(false);
  });

  it("reveals raw frontmatter when the cursor is inside the block", () => {
    const doc = "---\ntitle: x\n---\n\nbody";
    const ds = decos(doc, doc.indexOf("title"));
    expect(ds.some((d) => d.class?.includes("cm-lp-frontmatter"))).toBe(false);
    expect(ds.some((d) => d.hidden && d.from === 0)).toBe(false);
  });

  it("treats --- that is not on line 1 as a horizontal rule, not frontmatter", () => {
    const doc = "intro\n\n---\n\nbody";
    const ds = decos(doc, 0);
    expect(ds.some((d) => d.class?.includes("cm-lp-frontmatter"))).toBe(false);
    const hrPos = doc.indexOf("---");
    expect(ds.some((d) => d.widget && d.from === hrPos)).toBe(true);
  });

  it("ignores unclosed frontmatter (no second ---)", () => {
    const doc = "---\ntitle: x\n\nbody";
    const ds = decos(doc, doc.indexOf("body"));
    expect(ds.some((d) => d.class?.includes("cm-lp-frontmatter"))).toBe(false);
  });
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd web && pnpm test livePreview`
Expected: FAIL — frontmatter classes not emitted; the HR-widget assertion fails (HR currently rendered at 0).

- [ ] **Step 3: Add the `frontmatterRange` helper**

In `web/src/components/editor/livePreview.ts`, add this helper just above
`buildLivePreviewDecorations` (after the `isInsideTable` helper, ~line 101):

```typescript
/** Detect a leading YAML frontmatter block: line 1 is exactly `---`, with a
 *  later line that is exactly `---` closing it. Returns the block's document
 *  span and fence line numbers, or null when there is no closing fence (or the
 *  doc does not open with `---`). The markdown parser mis-parses this region, so
 *  it is detected by text-scan and its stray nodes are suppressed below. */
function frontmatterRange(
  state: EditorState,
): { start: number; end: number; openLine: number; closeLine: number } | null {
  const first = state.doc.line(1);
  if (first.text !== "---") return null;
  for (let n = 2; n <= state.doc.lines; n++) {
    const line = state.doc.line(n);
    if (line.text === "---") {
      return { start: first.from, end: line.to, openLine: 1, closeLine: n };
    }
  }
  return null;
}
```

- [ ] **Step 4: Add the suppression guard and the frontmatter decorations**

In `buildLivePreviewDecorations`, compute the range once at the top, right after
`const tree = syntaxTree(state);` (~line 109):

```typescript
  const fm = frontmatterRange(state);
```

Add the suppression guard as the FIRST statement inside the `enter` callback,
before `const { name, from, to } = node;` is used — insert immediately after that
destructuring (~line 113):

```typescript
      const { name, from, to } = node;
      if (fm && from < fm.end) return; // suppress parser nodes inside frontmatter
```

Guard the wikilink scan: in the `while ((m = WIKILINK.exec(text)) !== null)` loop,
add after the existing `if (isInsidePos(state, from, "Table")) continue;` (~line 289):

```typescript
    if (fm && from < fm.end) continue;
```

Guard the image scan: in the `while ((im = IMAGE.exec(text)) !== null)` loop, add
after its `if (isInsidePos(state, from, "Table")) continue;` (~line 316):

```typescript
    if (fm && from < fm.end) continue;
```

Finally, emit the frontmatter decorations. Add this block immediately after the
image scan loop and before `const decorations = Decoration.set(decos, true);`
(~line 335):

```typescript
  // Frontmatter: a styled box off-cursor (line classes + hidden `---` fences),
  // raw when the cursor is inside. The parser mis-reads this region, so it is
  // handled here by text-scan and suppressed in the tree iterate above.
  if (fm && !selectionTouches(state, fm.start, fm.end)) {
    for (let n = fm.openLine; n <= fm.closeLine; n++) {
      const line = state.doc.line(n);
      let cls = "cm-lp-frontmatter";
      if (n === fm.openLine) cls += " cm-lp-frontmatter-first";
      if (n === fm.closeLine) cls += " cm-lp-frontmatter-last";
      decos.push(Decoration.line({ class: cls }).range(line.from));
      if ((n === fm.openLine || n === fm.closeLine) && line.length > 0) {
        decos.push(Decoration.replace({}).range(line.from, line.to));
      }
    }
  }
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cd web && pnpm test livePreview`
Expected: PASS — frontmatter cases green, all existing live-preview tests still pass.

- [ ] **Step 6: Commit**

```bash
git add web/src/components/editor/livePreview.ts web/src/components/editor/livePreview.test.ts
git commit -m "feat(editor): render leading frontmatter as a styled block"
```

---

## Task 5: Frontmatter styling

**Files:**
- Modify: `web/src/components/editor/livePreview.css`

- [ ] **Step 1: Add the box styles**

Append to `web/src/components/editor/livePreview.css` (palette matches the
existing `cm-lp-codeblock` / `cm-lp-quote` graphite tones):

```css
.cm-lp-frontmatter {
  background: #16161c;
  border-left: 1px solid #26262e;
  border-right: 1px solid #26262e;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 0.85em;
  color: #9a9ba6;
}
.cm-doc-livepreview .cm-line.cm-lp-frontmatter {
  padding-left: 13px;
  padding-right: 13px;
}
.cm-lp-frontmatter-first {
  border-top: 1px solid #26262e;
  border-top-left-radius: 6px;
  border-top-right-radius: 6px;
}
.cm-lp-frontmatter-last {
  border-bottom: 1px solid #26262e;
  border-bottom-left-radius: 6px;
  border-bottom-right-radius: 6px;
}
```

- [ ] **Step 2: Manual verification in the app**

Run: `cd web && pnpm dev`, open a note that begins with a `---` frontmatter block.
- Off-cursor: the block shows as a bordered, muted, monospace box; the `---`
  fence lines are not visible (they read as the box's top/bottom padding).
- Click into the block: it reveals the raw `---` / `key: value` text for editing.
- A `---` later in the body still renders as a horizontal rule.

- [ ] **Step 3: Commit**

```bash
git add web/src/components/editor/livePreview.css
git commit -m "style(editor): frontmatter block styling"
```

---

## Task 6: Full gate + branch finish

**Files:** none (verification only)

- [ ] **Step 1: Format, then run the full web gate**

```bash
cd web && pnpm format && pnpm format:check && pnpm lint && pnpm typecheck && pnpm test && pnpm build
```
Expected: every step exits 0. (`format:check` is easy to miss and eslint will not
catch it — run it explicitly.)

- [ ] **Step 2: Run the full `just` gate (both stacks)**

```bash
just lint && just test && just build
```
Expected: all green (web + rust).

- [ ] **Step 3: Commit any formatting changes**

```bash
git add -A && git commit -m "chore(editor): formatting" || echo "nothing to format"
```

- [ ] **Step 4: Push and open the PR**

```bash
git push -u origin editor-frontmatter-wikilink-autocomplete
gh pr create --base main --title "feat(editor): frontmatter rendering + wikilink autocomplete" --body "$(cat <<'EOF'
Two leftover Phase 3 (editor depth) items, done together to avoid self-conflict in the live-preview subsystem.

## Frontmatter rendering
- Leading `---` YAML frontmatter renders as a styled, bordered box off-cursor; `---` fences hidden.
- Reveals raw text when the cursor is inside, like blockquote/codeblock.
- Text-scan detection + a tree-iterate guard suppress the parser's mis-read nodes (the opening `---` otherwise renders as an HR widget).

## Wikilink autocomplete
- Typing `[[` suggests note stems (case-insensitive substring, deduped) in both live-preview and source modes.
- Picking one auto-closes `]]` (reuses an existing `]]`, no doubling); no suggestions in the alias part after `|`.

Spec: `docs/superpowers/specs/2026-06-12-editor-frontmatter-wikilink-autocomplete-design.md`
Plan: `docs/superpowers/plans/2026-06-12-editor-frontmatter-wikilink-autocomplete.md`

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 5: Merge via the queue after Track A**

Per the repo's merge-queue workflow, use "Merge when ready" once Track A has
landed and CI is green. Do not manually update the branch.
```
