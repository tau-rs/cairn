# UI‑3 Document-Look Live Preview Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the editor's live-preview read like a rendered GitHub-README document (lists, blockquotes, HR, fenced code with syntax highlighting, clickable task checkboxes, tables, inline images) while staying a single editable CodeMirror surface that reveals raw markdown under the caret.

**Architecture:** Extend the existing CodeMirror 6 `buildLivePreviewDecorations` pure-function pipeline with block decorations + replace-widgets, all gated by the existing `selectionTouches` reveal-on-cursor logic (generalized to block line-ranges). A new `docTheme` strips the code-editor chrome and applies graphite prose typography + centered measure. One thin host addition (`assetUrl`) resolves local images. No command/query/contract changes.

**Tech Stack:** React 18 + TypeScript, CodeMirror 6 (`@codemirror/view`/`state`/`language`/`lang-markdown`), `@uiw/react-codemirror`, Tailwind 3 (graphite tokens), Vitest + Testing Library, Playwright. New dep: `@codemirror/language-data`.

**Spec:** `docs/superpowers/specs/2026-06-02-ui3-document-live-preview-design.md`

**Working conventions (read before starting):**
- Run all `pnpm` commands from `web/`. Git commands from the repo root (`/Users/titouanlebocq/code/cairn-ui`) or with `git -C`.
- The dev server (if you launch one) must use a non-default port: `pnpm dev --port 5273 --strictPort` (5173 belongs to a different app).
- Per-task gate before each commit: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check`. Run `pnpm build` on the final task. If `format:check` fails, run `pnpm format` and re-stage.
- The decoration unit-test harness lives in `web/src/components/editor/livePreview.test.ts` as `decos(doc, cursor)` — it builds an `EditorState` with `markdown({ base: markdownLanguage })` (GFM active) and returns `{from,to,class,widget,hidden}[]`. Reuse/extend it; do NOT rewrite it.
- CodeMirror widget DOM and editor interaction do NOT render under jsdom — assert *which decorations are emitted* in unit tests (pure builder), and assert *rendered DOM + interaction* in Playwright e2e. Never assert widget DOM in Vitest.

---

## File Structure

| File | Responsibility |
|---|---|
| `web/src/components/editor/docTheme.ts` | The `EditorView.theme` (transparent bg, Inter, caret/selection tokens, padding), the `docHighlightStyle` (graphite token colors for prose + code tokens), and `markdownCodeLanguages` (the `codeLanguages` list). |
| `web/src/components/editor/livePreview.css` | Prose typography, centered measure scoped to `.cm-doc-livepreview`, and block styles (lists, quote, code block, hr, checkbox, image, table). |
| `web/src/components/editor/livePreview.ts` | Orchestrator `buildLivePreviewDecorations` — extended with block emitters; generalized block reveal. |
| `web/src/components/editor/checkboxToggle.ts` | Pure: compute the `{from,to,insert}` doc change to flip `[ ]`↔`[x]`. |
| `web/src/components/editor/imageResolver.ts` | Pure: `makeImageResolver(assetUrl)` → `(src)=>url`; remote/data pass-through, local via `assetUrl`. |
| `web/src/components/editor/tableParse.ts` | Pure: `parseTable(md)` → `{header, rows}`. |
| `web/src/components/editor/widgets/taskCheckboxWidget.ts` | Clickable checkbox widget. |
| `web/src/components/editor/widgets/imageWidget.ts` | `<img>` widget (block/inline). |
| `web/src/components/editor/widgets/tableWidget.ts` | `<table>` widget from `parseTable`. |
| `web/src/components/editor/widgets/hrWidget.ts` | `<hr>` widget. |
| `web/src/components/Editor.tsx` | Minimal `basicSetup`, `docTheme` + highlight + codeLanguages, mode class, image-resolver wiring, header restyle. |
| `web/src/client/host.ts` | Add `assetUrl(relPath): string` to `CairnHost` + `MockHost`/`alwaysOpenHost`. |
| `web/src/client/tauri.ts` | `TauriHost.assetUrl` via `convertFileSrc` + cached root. |
| `web/src/store/store.ts` | Expose `assetUrl(relPath): string` (delegates to host) on store state. |
| `web/src/app/App.tsx` | Pass `assetUrl={actions.assetUrl}` to `<Editor/>`. |
| `web/src/client/fixtures.ts` | Add `kitchensink.md` exercising every element (for dev + e2e). |
| `web/e2e/skeleton.spec.ts` | New test asserting rendered elements + checkbox toggle + table reveal. |
| `web/package.json` | Add `@codemirror/language-data`. |

---

## Task 1: Foundation — chrome removal, document theme, prose typography

**Files:**
- Create: `web/src/components/editor/docTheme.ts`
- Create: `web/src/components/editor/docTheme.test.ts`
- Modify: `web/src/components/Editor.tsx`
- Modify: `web/src/components/editor/livePreview.css`
- Add dep: `@codemirror/language-data`

- [ ] **Step 1: Add the dependency**

Run (from `web/`): `pnpm add @codemirror/language-data`
Expected: `package.json` gains `@codemirror/language-data`, lockfile updates.

- [ ] **Step 2: Write the failing test for `docTheme`**

Create `web/src/components/editor/docTheme.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import { docTheme, docHighlightStyle, markdownCodeLanguages } from "./docTheme";

describe("docTheme", () => {
  it("exports a CodeMirror extension for the theme", () => {
    expect(docTheme).toBeTruthy();
  });
  it("exports a highlight style extension", () => {
    expect(docHighlightStyle).toBeTruthy();
  });
  it("exports a non-empty list of code languages for fenced highlighting", () => {
    expect(Array.isArray(markdownCodeLanguages)).toBe(true);
    expect(markdownCodeLanguages.length).toBeGreaterThan(0);
  });
});
```

- [ ] **Step 3: Run it to verify it fails**

Run: `pnpm test -- docTheme`
Expected: FAIL — cannot find module `./docTheme`.

- [ ] **Step 4: Implement `docTheme.ts`**

Create `web/src/components/editor/docTheme.ts`:

```ts
import { EditorView } from "@codemirror/view";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { tags as t } from "@lezer/highlight";
import { languages } from "@codemirror/language-data";

/** Languages available for fenced-code syntax highlighting. */
export const markdownCodeLanguages = languages;

/** Graphite-token colors for markdown prose tokens and fenced-code tokens. */
const style = HighlightStyle.define([
  { tag: t.keyword, color: "#c4b5fd" },
  { tag: [t.string, t.special(t.string)], color: "#a5d6a7" },
  { tag: [t.number, t.bool, t.null], color: "#f5b76b" },
  { tag: [t.function(t.variableName), t.labelName], color: "#7dd3fc" },
  { tag: [t.comment, t.lineComment, t.blockComment], color: "#6b6c77", fontStyle: "italic" },
  { tag: [t.typeName, t.className], color: "#7dd3fc" },
  { tag: t.propertyName, color: "#cdd0e0" },
  { tag: [t.operator, t.punctuation], color: "#9a9ba6" },
]);

export const docHighlightStyle = syntaxHighlighting(style);

/** Strips the code-editor look: transparent bg, Inter prose font, token caret/
 *  selection, comfortable padding, no focus outline. The centered measure and
 *  monospace-vs-prose split is handled in CSS via the cm-doc-* container class. */
export const docTheme = EditorView.theme(
  {
    "&": { backgroundColor: "transparent", color: "#f1f1f4" },
    "&.cm-focused": { outline: "none" },
    ".cm-content": {
      fontFamily: '"Inter Variable", Inter, system-ui, sans-serif',
      fontSize: "14px",
      lineHeight: "1.7",
      padding: "8px 0 40px",
      caretColor: "#6366f1",
    },
    ".cm-scroller": { fontFamily: "inherit" },
    ".cm-cursor": { borderLeftColor: "#6366f1" },
    "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection": {
      backgroundColor: "#2a2a44",
    },
    ".cm-line": { padding: "0 2px" },
  },
  { dark: true },
);
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `pnpm test -- docTheme`
Expected: PASS (3 tests).

- [ ] **Step 6: Wire the theme + minimal setup into `Editor.tsx`**

In `web/src/components/Editor.tsx`: import the theme pieces and `EditorView`, build extensions with `codeLanguages`, drop `theme="dark"`, pass a minimal `basicSetup`, and put a mode class on the wrapper. Replace the imports/`extensions`/`<CodeMirror>` region with:

```tsx
import { useMemo } from "react";
import CodeMirror from "@uiw/react-codemirror";
import { EditorView } from "@codemirror/view";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { livePreview } from "./editor/livePreview";
import {
  docTheme,
  docHighlightStyle,
  markdownCodeLanguages,
} from "./editor/docTheme";
import { stem } from "../client/wikilink";
import { Button } from "./ui/Button";
```

Then the `extensions` memo becomes:

```tsx
  const onOpenNote = props.onOpenNote;
  const extensions = useMemo(() => {
    const base = markdown({
      base: markdownLanguage,
      codeLanguages: markdownCodeLanguages,
    });
    const common = [base, docTheme, docHighlightStyle, EditorView.lineWrapping];
    return props.mode === "livepreview"
      ? [...common, livePreview({ resolve, onOpenNote })]
      : common;
  }, [props.mode, resolve, onOpenNote]);
```

And the returned `<CodeMirror>` block becomes (note the wrapper class + `basicSetup`, and `theme="dark"` removed):

```tsx
  return (
    <div className="flex h-full flex-col">
      <div className="mb-2 flex items-center justify-between">
        <span className="text-sm text-muted">{props.path}</span>
        <Button variant="ghost" onClick={props.onToggleMode}>
          {props.mode === "livepreview" ? "Source" : "Live Preview"}
        </Button>
      </div>
      <div
        className={
          props.mode === "livepreview" ? "cm-doc-livepreview" : "cm-doc-source"
        }
      >
        <CodeMirror
          value={props.value}
          height="100%"
          extensions={extensions}
          basicSetup={{
            lineNumbers: false,
            foldGutter: false,
            highlightActiveLine: false,
            highlightActiveLineGutter: false,
          }}
          onChange={props.onChange}
        />
      </div>
    </div>
  );
```

(The `resolve` memo above it is unchanged.)

- [ ] **Step 7: Add prose typography + centered measure to `livePreview.css`**

Prepend to `web/src/components/editor/livePreview.css` (keep the existing `.cm-lp-*` rules below; update heading sizes for prose feel):

```css
/* --- UI-3 document look --- */
.cm-doc-livepreview .cm-content {
  max-width: 34em;
  margin: 0 auto;
}
.cm-doc-source .cm-content {
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  max-width: none;
}
.cm-doc-livepreview .cm-line {
  margin: 0.15em 0;
}
```

- [ ] **Step 8: Run the full per-task gate**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check`
Expected: all PASS (existing 85 tests + 3 new docTheme tests = 88). If `format:check` fails, run `pnpm format` and re-stage.

- [ ] **Step 9: Commit**

```bash
git add web/src/components/editor/docTheme.ts web/src/components/editor/docTheme.test.ts web/src/components/Editor.tsx web/src/components/editor/livePreview.css web/package.json web/pnpm-lock.yaml
git commit -m "feat(editor): document theme + prose typography, drop code-editor chrome"
```

---

## Task 2: Bulleted & numbered lists

**Files:**
- Modify: `web/src/components/editor/livePreview.ts`
- Modify: `web/src/components/editor/livePreview.test.ts`
- Modify: `web/src/components/editor/livePreview.css`

- [ ] **Step 1: Write the failing tests**

Append to `web/src/components/editor/livePreview.test.ts` inside the `describe`:

```ts
  it("marks bullet list lines and replaces the bullet marker off-cursor", () => {
    const doc = "- alpha\n- beta";
    const ds = decos(doc, doc.length); // cursor on 2nd line end
    // first line's marker (positions 0..1 = "- ") is replaced by a bullet widget
    expect(ds.some((d) => d.widget && d.from === 0)).toBe(true);
    // a list-item line class is applied
    expect(ds.some((d) => d.class === "cm-lp-li")).toBe(true);
  });
  it("reveals the raw bullet marker when the cursor is on that item", () => {
    const ds = decos("- alpha\n- beta", 2); // cursor inside first item
    expect(ds.some((d) => d.widget && d.from === 0)).toBe(false);
  });
  it("keeps the number marker on ordered lists (no bullet widget)", () => {
    const doc = "1. one\n2. two";
    const ds = decos(doc, doc.length);
    // ordered markers are not replaced with a bullet widget
    expect(ds.some((d) => d.widget && d.from === 0)).toBe(false);
    expect(ds.some((d) => d.class === "cm-lp-li")).toBe(true);
  });
```

- [ ] **Step 2: Run to verify failure**

Run: `pnpm test -- livePreview`
Expected: FAIL — the three new tests fail (no `cm-lp-li`, no bullet widget).

- [ ] **Step 3: Add the bullet widget**

Create `web/src/components/editor/widgets/bulletWidget.ts`:

```ts
import { WidgetType } from "@codemirror/view";

/** Renders a styled list bullet in place of a `-`/`*`/`+` marker. */
export class BulletWidget extends WidgetType {
  eq(): boolean {
    return true;
  }
  toDOM(): HTMLElement {
    const el = document.createElement("span");
    el.className = "cm-lp-bullet";
    el.textContent = "•";
    return el;
  }
  ignoreEvent(): boolean {
    return true;
  }
}
```

- [ ] **Step 4: Emit list decorations in `livePreview.ts`**

In `web/src/components/editor/livePreview.ts`, add the import at the top:

```ts
import { BulletWidget } from "./widgets/bulletWidget";
```

Add a helper near `selectionTouches` (it lets block emitters gate on the whole line range):

```ts
function lineRange(state: EditorState, from: number, to: number) {
  return { start: state.doc.lineAt(from).from, end: state.doc.lineAt(to).to };
}
```

Inside `tree.iterate`'s `enter` callback, add a branch (after the existing `Link` branch's `else if`), to handle list items:

```ts
      } else if (name === "ListItem") {
        // Style each line of the item; replace a bullet marker with a • widget.
        const line = state.doc.lineAt(from);
        decos.push(Decoration.line({ class: "cm-lp-li" }).range(line.from));
        const mark = node.node.getChild("ListMark");
        if (mark) {
          const markText = state.doc.sliceString(mark.from, mark.to);
          const isBullet = /^[-*+]$/.test(markText);
          const touched = selectionTouches(state, line.from, line.to);
          if (isBullet && !touched) {
            // replace "- " (marker + following space) with the bullet widget
            const end = Math.min(mark.to + 1, line.to);
            decos.push(
              Decoration.replace({ widget: new BulletWidget() }).range(
                mark.from,
                end,
              ),
            );
          }
        }
      }
```

Note: `Decoration.line` must be sorted with `startSide`; the existing `Decoration.set(decos, true)` sort handles ordering. Line decorations attach at the line start position.

- [ ] **Step 5: Run the tests to verify pass**

Run: `pnpm test -- livePreview`
Expected: PASS (new list tests green; existing tests still green).

- [ ] **Step 6: Style lists in `livePreview.css`**

Append to `web/src/components/editor/livePreview.css`:

```css
.cm-lp-li {
  padding-left: 1.4em;
}
.cm-lp-bullet {
  color: #6b6c77;
  display: inline-block;
  width: 1.2em;
  margin-left: -1.2em;
  text-align: center;
}
```

- [ ] **Step 7: Per-task gate**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check`
Expected: all PASS. Fix format if needed.

- [ ] **Step 8: Commit**

```bash
git add web/src/components/editor/widgets/bulletWidget.ts web/src/components/editor/livePreview.ts web/src/components/editor/livePreview.test.ts web/src/components/editor/livePreview.css
git commit -m "feat(editor): render bulleted/numbered lists in live preview"
```

---

## Task 3: Blockquotes & horizontal rules

**Files:**
- Create: `web/src/components/editor/widgets/hrWidget.ts`
- Modify: `web/src/components/editor/livePreview.ts`
- Modify: `web/src/components/editor/livePreview.test.ts`
- Modify: `web/src/components/editor/livePreview.css`

- [ ] **Step 1: Write the failing tests**

Append to the `describe` in `livePreview.test.ts`:

```ts
  it("styles a blockquote line and hides the > marker off-cursor", () => {
    const doc = "> quoted\n\nbody";
    const ds = decos(doc, doc.indexOf("body"));
    expect(ds.some((d) => d.class === "cm-lp-quote")).toBe(true);
    expect(ds.some((d) => d.hidden && d.from === 0)).toBe(true);
  });
  it("reveals the > marker when the cursor is in the quote", () => {
    const ds = decos("> quoted\n\nbody", 2);
    expect(ds.some((d) => d.hidden && d.from === 0)).toBe(false);
  });
  it("replaces a horizontal rule with a widget off-cursor", () => {
    const doc = "a\n\n---\n\nb";
    const hrPos = doc.indexOf("---");
    const ds = decos(doc, 0);
    expect(ds.some((d) => d.widget && d.from === hrPos)).toBe(true);
  });
  it("reveals the raw rule when the cursor is on it", () => {
    const doc = "a\n\n---\n\nb";
    const hrPos = doc.indexOf("---");
    const ds = decos(doc, hrPos + 1);
    expect(ds.some((d) => d.widget && d.from === hrPos)).toBe(false);
  });
```

- [ ] **Step 2: Run to verify failure**

Run: `pnpm test -- livePreview`
Expected: FAIL — the four new tests fail.

- [ ] **Step 3: Add the HR widget**

Create `web/src/components/editor/widgets/hrWidget.ts`:

```ts
import { WidgetType } from "@codemirror/view";

export class HrWidget extends WidgetType {
  eq(): boolean {
    return true;
  }
  toDOM(): HTMLElement {
    const el = document.createElement("div");
    el.className = "cm-lp-hr";
    return el;
  }
  ignoreEvent(): boolean {
    return true;
  }
}
```

- [ ] **Step 4: Emit blockquote + HR decorations in `livePreview.ts`**

Add the import:

```ts
import { HrWidget } from "./widgets/hrWidget";
```

Add two branches inside `tree.iterate`'s `enter` callback (after the `ListItem` branch):

```ts
      } else if (name === "Blockquote") {
        // Class every line of the quote; hide each line's "> " mark off-cursor.
        const touched = selectionTouches(state, from, to);
        let pos = from;
        while (pos <= to) {
          const line = state.doc.lineAt(pos);
          decos.push(Decoration.line({ class: "cm-lp-quote" }).range(line.from));
          if (!touched) {
            const m = /^(\s*>\s?)/.exec(line.text);
            if (m) {
              decos.push(
                Decoration.replace({}).range(line.from, line.from + m[1].length),
              );
            }
          }
          if (line.to >= to) break;
          pos = line.to + 1;
        }
      } else if (name === "HorizontalRule") {
        if (!selectionTouches(state, from, to)) {
          decos.push(
            Decoration.replace({ widget: new HrWidget() }).range(from, to),
          );
        }
      }
```

- [ ] **Step 5: Run the tests to verify pass**

Run: `pnpm test -- livePreview`
Expected: PASS (blockquote + HR tests green; existing still green).

- [ ] **Step 6: Style quote + hr in `livePreview.css`**

Append:

```css
.cm-lp-quote {
  border-left: 3px solid #2f2f3a;
  padding-left: 12px;
  color: #9a9ba6;
}
.cm-lp-hr {
  border-top: 1px solid #26262e;
  margin: 0.4em 0;
}
```

- [ ] **Step 7: Per-task gate**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check`
Expected: all PASS.

- [ ] **Step 8: Commit**

```bash
git add web/src/components/editor/widgets/hrWidget.ts web/src/components/editor/livePreview.ts web/src/components/editor/livePreview.test.ts web/src/components/editor/livePreview.css
git commit -m "feat(editor): render blockquotes and horizontal rules"
```

---

## Task 4: Fenced code blocks (background + syntax highlighting)

**Files:**
- Modify: `web/src/components/editor/livePreview.ts`
- Modify: `web/src/components/editor/livePreview.test.ts`
- Modify: `web/src/components/editor/livePreview.css`

Syntax-token coloring is already provided by `docHighlightStyle` + `codeLanguages` (wired in Task 1). This task adds the block background and hides the ``` fences off-cursor.

- [ ] **Step 1: Write the failing tests**

Append to the `describe`:

```ts
  it("styles fenced-code lines and hides the fence lines off-cursor", () => {
    const doc = "text\n\n```js\nconst x = 1;\n```\n\nmore";
    const fence = doc.indexOf("```");
    const ds = decos(doc, 0);
    expect(ds.some((d) => d.class === "cm-lp-codeblock")).toBe(true);
    // opening fence line is hidden
    expect(ds.some((d) => d.hidden && d.from === fence)).toBe(true);
  });
  it("reveals the fences when the cursor is inside the code block", () => {
    const doc = "text\n\n```js\nconst x = 1;\n```\n\nmore";
    const fence = doc.indexOf("```");
    const ds = decos(doc, doc.indexOf("const"));
    expect(ds.some((d) => d.hidden && d.from === fence)).toBe(false);
  });
```

- [ ] **Step 2: Run to verify failure**

Run: `pnpm test -- livePreview`
Expected: FAIL — both new tests fail.

- [ ] **Step 3: Emit fenced-code decorations in `livePreview.ts`**

Add a branch inside `tree.iterate`'s `enter` callback (after the `HorizontalRule` branch):

```ts
      } else if (name === "FencedCode") {
        const touched = selectionTouches(
          state,
          lineRange(state, from, to).start,
          lineRange(state, from, to).end,
        );
        const firstLine = state.doc.lineAt(from).number;
        const lastLine = state.doc.lineAt(to).number;
        for (let n = firstLine; n <= lastLine; n++) {
          const line = state.doc.line(n);
          decos.push(
            Decoration.line({ class: "cm-lp-codeblock" }).range(line.from),
          );
          const isFence = /^\s*```/.test(line.text);
          if (isFence && !touched && line.length > 0) {
            decos.push(Decoration.replace({}).range(line.from, line.to));
          }
        }
      }
```

- [ ] **Step 4: Run the tests to verify pass**

Run: `pnpm test -- livePreview`
Expected: PASS.

- [ ] **Step 5: Style the code block in `livePreview.css`**

Append:

```css
.cm-lp-codeblock {
  background: #16161c;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 0.88em;
}
.cm-lp-codeblock:first-of-type,
.cm-doc-livepreview .cm-line.cm-lp-codeblock {
  padding-left: 13px;
  padding-right: 13px;
}
```

- [ ] **Step 6: Per-task gate**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check`
Expected: all PASS.

- [ ] **Step 7: Commit**

```bash
git add web/src/components/editor/livePreview.ts web/src/components/editor/livePreview.test.ts web/src/components/editor/livePreview.css
git commit -m "feat(editor): styled fenced code blocks with syntax highlighting"
```

---

## Task 5: Clickable task checkboxes

**Files:**
- Create: `web/src/components/editor/checkboxToggle.ts`
- Create: `web/src/components/editor/checkboxToggle.test.ts`
- Create: `web/src/components/editor/widgets/taskCheckboxWidget.ts`
- Modify: `web/src/components/editor/livePreview.ts`
- Modify: `web/src/components/editor/livePreview.test.ts`
- Modify: `web/src/components/editor/livePreview.css`

- [ ] **Step 1: Write the failing test for `checkboxToggle`**

Create `web/src/components/editor/checkboxToggle.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import { toggleCheckboxChange } from "./checkboxToggle";

describe("toggleCheckboxChange", () => {
  it("turns an unchecked box into a checked one", () => {
    const doc = "- [ ] task";
    const open = doc.indexOf("[");
    expect(toggleCheckboxChange(doc, open)).toEqual({
      from: open + 1,
      to: open + 2,
      insert: "x",
    });
  });
  it("turns a checked box into an unchecked one", () => {
    const doc = "- [x] task";
    const open = doc.indexOf("[");
    expect(toggleCheckboxChange(doc, open)).toEqual({
      from: open + 1,
      to: open + 2,
      insert: " ",
    });
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `pnpm test -- checkboxToggle`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement `checkboxToggle.ts`**

Create `web/src/components/editor/checkboxToggle.ts`:

```ts
/** Given the doc text and the index of the `[` in a `[ ]`/`[x]` task marker,
 *  return the single-character change that flips its state. */
export function toggleCheckboxChange(
  doc: string,
  bracketOpen: number,
): { from: number; to: number; insert: string } {
  const inner = doc[bracketOpen + 1];
  const insert = inner === " " ? "x" : " ";
  return { from: bracketOpen + 1, to: bracketOpen + 2, insert };
}
```

- [ ] **Step 4: Run to verify pass**

Run: `pnpm test -- checkboxToggle`
Expected: PASS (2 tests).

- [ ] **Step 5: Add the checkbox widget**

Create `web/src/components/editor/widgets/taskCheckboxWidget.ts`:

```ts
import { WidgetType } from "@codemirror/view";

/** A clickable checkbox; on mousedown it asks the host to toggle the source. */
export class TaskCheckboxWidget extends WidgetType {
  constructor(
    readonly checked: boolean,
    readonly bracketOpen: number,
    readonly onToggle: (bracketOpen: number) => void,
  ) {
    super();
  }
  eq(other: TaskCheckboxWidget): boolean {
    return (
      other.checked === this.checked && other.bracketOpen === this.bracketOpen
    );
  }
  toDOM(): HTMLElement {
    const el = document.createElement("span");
    el.className = "cm-lp-task " + (this.checked ? "checked" : "unchecked");
    el.setAttribute("role", "checkbox");
    el.setAttribute("aria-checked", String(this.checked));
    el.addEventListener("mousedown", (e) => {
      e.preventDefault();
      this.onToggle(this.bracketOpen);
    });
    return el;
  }
  ignoreEvent(): boolean {
    return false;
  }
}
```

- [ ] **Step 6: Write the failing live-preview test for checkboxes**

The `livePreview` options need an `onToggleCheckbox` callback. Update the shared `opts` in `livePreview.test.ts` to include it, then add tests. Change the top `opts`:

```ts
const opts = {
  resolve: (t: string) => (t === "ideas" ? "ideas.md" : null),
  onOpenNote: vi.fn(),
  onToggleCheckbox: vi.fn(),
};
```

Append tests to the `describe`:

```ts
  it("renders a task checkbox as a widget off-cursor", () => {
    const doc = "- [ ] todo item";
    const ds = decos(doc, doc.length); // cursor at end, off the marker
    const open = doc.indexOf("[");
    expect(ds.some((d) => d.widget && d.from === open)).toBe(true);
  });
  it("reveals the raw [ ] marker when the cursor is on the task", () => {
    const doc = "- [ ] todo item";
    const open = doc.indexOf("[");
    const ds = decos(doc, open + 1);
    expect(ds.some((d) => d.widget && d.from === open)).toBe(false);
  });
```

- [ ] **Step 7: Run to verify failure**

Run: `pnpm test -- livePreview`
Expected: FAIL — checkbox widget not emitted (and a type error if `LivePreviewOptions` lacks `onToggleCheckbox`).

- [ ] **Step 8: Emit checkbox decorations in `livePreview.ts`**

Add imports:

```ts
import { TaskCheckboxWidget } from "./widgets/taskCheckboxWidget";
```

Extend the options interface:

```ts
export interface LivePreviewOptions {
  resolve: (target: string) => string | null;
  onOpenNote: (path: string) => void;
  onToggleCheckbox: (bracketOpen: number) => void;
}
```

Inside the `ListItem` branch (Task 2), after the bullet-marker handling, detect a task marker on the item's first line and replace it. Add at the end of the `ListItem` branch, before its closing brace:

```ts
        const liLine = state.doc.lineAt(from);
        const taskMatch = /^(\s*[-*+]\s+)(\[[ xX]\])/.exec(liLine.text);
        if (taskMatch) {
          const open = liLine.from + taskMatch[1].length; // index of "["
          const close = open + 3; // covers "[ ]"
          if (!selectionTouches(state, liLine.from, liLine.to)) {
            const checked = /[xX]/.test(liLine.text[open - liLine.from + 1]);
            decos.push(
              Decoration.replace({
                widget: new TaskCheckboxWidget(
                  checked,
                  open,
                  opts.onToggleCheckbox,
                ),
              }).range(open, close),
            );
          }
        }
```

- [ ] **Step 9: Run to verify pass**

Run: `pnpm test -- livePreview`
Expected: PASS.

- [ ] **Step 10: Wire `onToggleCheckbox` through `Editor.tsx`**

The widget calls `onToggleCheckbox(bracketOpen)`; the editor must dispatch the doc change. In `Editor.tsx`, import the helper and `EditorView` (already imported), and add a ref to the view. The simplest robust wiring: pass an `onToggleCheckbox` that uses the `onChange`-backed value is NOT enough (no position API), so use a CodeMirror update via the `EditorView`. Use `@uiw/react-codemirror`'s `onCreateEditor` to capture the view:

Add import:

```tsx
import { toggleCheckboxChange } from "./editor/checkboxToggle";
```

Add a view ref inside the component (top of `Editor`):

```tsx
  const viewRef = useRef<EditorView | null>(null);
```

(add `useRef` to the `react` import: `import { useMemo, useRef } from "react";`)

Extend the `livePreview` call in the `extensions` memo to provide the toggle:

```tsx
    const lp = livePreview({
      resolve,
      onOpenNote,
      onToggleCheckbox: (bracketOpen: number) => {
        const view = viewRef.current;
        if (!view) return;
        const change = toggleCheckboxChange(view.state.doc.toString(), bracketOpen);
        view.dispatch({ changes: change });
      },
    });
    return props.mode === "livepreview" ? [...common, lp] : common;
```

(Replace the previous `livePreview({ resolve, onOpenNote })` usage accordingly; `onToggleCheckbox` is captured outside the memo deps because it only reads `viewRef.current` — add an eslint-disable if the linter complains about exhaustive-deps, or include nothing extra since `viewRef` is stable.)

And capture the view on the `<CodeMirror>`:

```tsx
        <CodeMirror
          value={props.value}
          height="100%"
          extensions={extensions}
          basicSetup={{
            lineNumbers: false,
            foldGutter: false,
            highlightActiveLine: false,
            highlightActiveLineGutter: false,
          }}
          onChange={props.onChange}
          onCreateEditor={(view) => {
            viewRef.current = view;
          }}
        />
```

- [ ] **Step 11: Style the checkbox in `livePreview.css`**

Append:

```css
.cm-lp-task {
  display: inline-block;
  width: 14px;
  height: 14px;
  border: 1.5px solid #6b6c77;
  border-radius: 3px;
  vertical-align: -2px;
  margin-right: 8px;
  cursor: pointer;
}
.cm-lp-task.checked {
  background: #6366f1;
  border-color: #6366f1;
}
```

- [ ] **Step 12: Per-task gate**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check`
Expected: all PASS. (`Editor.tsx` cannot be unit-tested for the dispatch under jsdom; the click→toggle is covered by e2e in Task 8.)

- [ ] **Step 13: Commit**

```bash
git add web/src/components/editor/checkboxToggle.ts web/src/components/editor/checkboxToggle.test.ts web/src/components/editor/widgets/taskCheckboxWidget.ts web/src/components/editor/livePreview.ts web/src/components/editor/livePreview.test.ts web/src/components/editor/livePreview.css web/src/components/Editor.tsx
git commit -m "feat(editor): clickable task checkboxes"
```

---

## Task 6: Inline images + host `assetUrl`

**Files:**
- Create: `web/src/components/editor/imageResolver.ts`
- Create: `web/src/components/editor/imageResolver.test.ts`
- Create: `web/src/components/editor/widgets/imageWidget.ts`
- Modify: `web/src/client/host.ts`
- Modify: `web/src/client/tauri.ts`
- Modify: `web/src/store/store.ts`
- Modify: `web/src/app/App.tsx`
- Modify: `web/src/components/Editor.tsx`
- Modify: `web/src/components/editor/livePreview.ts`
- Modify: `web/src/components/editor/livePreview.test.ts`
- Modify: `web/src/components/editor/livePreview.css`

- [ ] **Step 1: Write the failing test for `imageResolver`**

Create `web/src/components/editor/imageResolver.test.ts`:

```ts
import { describe, it, expect, vi } from "vitest";
import { makeImageResolver } from "./imageResolver";

describe("makeImageResolver", () => {
  it("passes through http(s) URLs unchanged", () => {
    const r = makeImageResolver(vi.fn());
    expect(r("https://x/y.png")).toBe("https://x/y.png");
  });
  it("passes through data URLs unchanged", () => {
    const r = makeImageResolver(vi.fn());
    expect(r("data:image/png;base64,AAAA")).toBe("data:image/png;base64,AAAA");
  });
  it("resolves local relative paths via assetUrl", () => {
    const assetUrl = vi.fn().mockReturnValue("asset://img/logo.png");
    const r = makeImageResolver(assetUrl);
    expect(r("img/logo.png")).toBe("asset://img/logo.png");
    expect(assetUrl).toHaveBeenCalledWith("img/logo.png");
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `pnpm test -- imageResolver`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement `imageResolver.ts`**

Create `web/src/components/editor/imageResolver.ts`:

```ts
export type AssetUrl = (relPath: string) => string;

/** Map an image markdown `src` to a displayable URL. Remote/data URLs pass
 *  through; local relative paths are resolved through the host's assetUrl. */
export function makeImageResolver(assetUrl: AssetUrl) {
  return (src: string): string => {
    if (/^(https?:|data:)/i.test(src)) return src;
    return assetUrl(src);
  };
}
```

- [ ] **Step 4: Run to verify pass**

Run: `pnpm test -- imageResolver`
Expected: PASS (3 tests).

- [ ] **Step 5: Write the failing host test**

Append to `web/src/client/host.test.ts` (a `MockHost` describe likely exists; add a test):

```ts
import { MockHost } from "./host";

describe("MockHost.assetUrl", () => {
  it("returns an inline data image for any local path", () => {
    const url = new MockHost().assetUrl("img/logo.png");
    expect(url.startsWith("data:image/")).toBe(true);
  });
});
```

(If `describe`/imports are already present at top of the file, only add the new `describe` block and reuse existing imports.)

- [ ] **Step 6: Run to verify failure**

Run: `pnpm test -- host`
Expected: FAIL — `assetUrl` not on `MockHost`.

- [ ] **Step 7: Add `assetUrl` to the host contract + mock**

In `web/src/client/host.ts`:

Add to the interface:

```ts
  /** Resolve a local relative asset path to a displayable URL (sync). */
  assetUrl(relPath: string): string;
```

A shared 1×1 transparent PNG data URL constant + implementations:

```ts
const BLANK_PNG =
  "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
```

Add to `alwaysOpenHost`:

```ts
  assetUrl: () => BLANK_PNG,
```

Add to `MockHost`:

```ts
  assetUrl() {
    return BLANK_PNG;
  }
```

- [ ] **Step 8: Add `assetUrl` to `TauriHost`**

In `web/src/client/tauri.ts`, import `convertFileSrc` and cache the root:

```ts
import { convertFileSrc } from "@tauri-apps/api/core";
```

Update `TauriHost`:

```ts
export class TauriHost implements CairnHost {
  private root: string | null = null;
  async currentCairn(): Promise<string | null> {
    this.root = await invoke<string | null>("current_cairn");
    return this.root;
  }
  async openCairn(): Promise<string | null> {
    this.root = await invoke<string | null>("pick_and_open_cairn");
    return this.root;
  }
  assetUrl(relPath: string): string {
    if (/^(https?:|data:)/i.test(relPath)) return relPath;
    if (!this.root) return relPath;
    const sep = this.root.endsWith("/") ? "" : "/";
    return convertFileSrc(`${this.root}${sep}${relPath}`);
  }
}
```

(If `tauri.test.ts` mocks `@tauri-apps/api/core`'s `invoke`, extend that mock to also export a `convertFileSrc` stub, e.g. `convertFileSrc: (p: string) => "asset://" + p`, and add a test: `new TauriHost().assetUrl("img/x.png")` returns the input when no root is set. Run `pnpm test -- tauri` and make it green.)

- [ ] **Step 9: Expose `assetUrl` on the store + pass to Editor**

In `web/src/store/store.ts`, add to the store state shape (near the other actions) and its implementation, delegating to the captured `host`:

```ts
  assetUrl: (relPath: string) => string;
```

In the `createCairnStore` returned object literal, add:

```ts
    assetUrl: (relPath: string) => host.assetUrl(relPath),
```

In `web/src/app/App.tsx`, pass it to `<Editor/>` (in the editor branch around line 127):

```tsx
              <Editor
                path={activePath}
                value={activeContents}
                mode={editorMode}
                notePaths={notePaths}
                assetUrl={actions.assetUrl}
                onChange={actions.editBuffer}
                onOpenNote={actions.openNote}
                onToggleMode={() =>
                  actions.setSettings({
                    editorMode:
                      editorMode === "livepreview" ? "source" : "livepreview",
                  })
                }
              />
```

- [ ] **Step 10: Add the image widget**

Create `web/src/components/editor/widgets/imageWidget.ts`:

```ts
import { WidgetType } from "@codemirror/view";

export class ImageWidget extends WidgetType {
  constructor(
    readonly src: string,
    readonly alt: string,
    readonly block: boolean,
  ) {
    super();
  }
  eq(other: ImageWidget): boolean {
    return (
      other.src === this.src &&
      other.alt === this.alt &&
      other.block === this.block
    );
  }
  toDOM(): HTMLElement {
    const img = document.createElement("img");
    img.className = this.block ? "cm-lp-img block" : "cm-lp-img";
    img.src = this.src;
    img.alt = this.alt;
    return img;
  }
  ignoreEvent(): boolean {
    return true;
  }
}
```

- [ ] **Step 11: Write the failing live-preview test for images**

Add `resolveImage` to the shared `opts` in `livePreview.test.ts`:

```ts
const opts = {
  resolve: (t: string) => (t === "ideas" ? "ideas.md" : null),
  onOpenNote: vi.fn(),
  onToggleCheckbox: vi.fn(),
  resolveImage: (src: string) => "resolved:" + src,
};
```

Append tests:

```ts
  it("replaces an image with a widget off-cursor", () => {
    const doc = "see ![logo](img/logo.png) here";
    const at = doc.indexOf("![");
    const ds = decos(doc, 0);
    expect(ds.some((d) => d.widget && d.from === at)).toBe(true);
  });
  it("reveals the raw image markdown when the cursor is on it", () => {
    const doc = "see ![logo](img/logo.png) here";
    const at = doc.indexOf("![");
    const ds = decos(doc, at + 2);
    expect(ds.some((d) => d.widget && d.from === at)).toBe(false);
  });
```

- [ ] **Step 12: Run to verify failure**

Run: `pnpm test -- livePreview`
Expected: FAIL — image widget not emitted (and type error: `LivePreviewOptions` lacks `resolveImage`).

- [ ] **Step 13: Emit image decorations in `livePreview.ts`**

Add the import:

```ts
import { ImageWidget } from "./widgets/imageWidget";
```

Extend the options interface:

```ts
  resolveImage: (src: string) => string;
```

Add a scan after the wikilink scan (images aren't reliably a single replaceable token via the tree across nesting, so scan the text like wikilinks). Add near the bottom of `buildLivePreviewDecorations`, before `return Decoration.set(...)`:

```ts
  const IMAGE = /!\[([^\]]*)\]\(([^)\s]+)\)/g;
  IMAGE.lastIndex = 0;
  let im: RegExpExecArray | null;
  while ((im = IMAGE.exec(text)) !== null) {
    const from = im.index;
    const to = from + im[0].length;
    if (isInsideCode(state, from)) continue;
    if (selectionTouches(state, from, to)) continue;
    const alt = im[1];
    const src = opts.resolveImage(im[2]);
    const line = state.doc.lineAt(from);
    const block = line.text.trim() === im[0];
    decos.push(
      Decoration.replace({
        widget: new ImageWidget(src, alt, block),
      }).range(from, to),
    );
  }
```

(`text` is already defined above for the wikilink scan; reuse it.)

- [ ] **Step 14: Pass `resolveImage` from `Editor.tsx`**

In `Editor.tsx`, add the import and prop, and build the resolver. Add prop to the `Editor` props type:

```tsx
  assetUrl: (relPath: string) => string;
```

Add the import:

```tsx
import { makeImageResolver } from "./editor/imageResolver";
```

Build a memoized resolver and pass it into `livePreview`:

```tsx
  const resolveImage = useMemo(
    () => makeImageResolver(props.assetUrl),
    [props.assetUrl],
  );
```

And include `resolveImage` in the `livePreview({ ... })` call options object (alongside `resolve`, `onOpenNote`, `onToggleCheckbox`). Add `resolveImage` to the `extensions` memo dependency array.

- [ ] **Step 15: Run to verify pass**

Run: `pnpm test -- livePreview imageResolver host`
Expected: PASS.

- [ ] **Step 16: Style images in `livePreview.css`**

Append:

```css
.cm-lp-img {
  max-width: 100%;
  vertical-align: middle;
  border-radius: 6px;
}
.cm-lp-img.block {
  display: block;
  margin: 0.6em 0;
}
```

- [ ] **Step 17: Per-task gate**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check`
Expected: all PASS.

- [ ] **Step 18: Commit**

```bash
git add web/src/components/editor/imageResolver.ts web/src/components/editor/imageResolver.test.ts web/src/components/editor/widgets/imageWidget.ts web/src/client/host.ts web/src/client/host.test.ts web/src/client/tauri.ts web/src/client/tauri.test.ts web/src/store/store.ts web/src/app/App.tsx web/src/components/Editor.tsx web/src/components/editor/livePreview.ts web/src/components/editor/livePreview.test.ts web/src/components/editor/livePreview.css
git commit -m "feat(editor): inline images with host asset resolution"
```

---

## Task 7: Tables

**Files:**
- Create: `web/src/components/editor/tableParse.ts`
- Create: `web/src/components/editor/tableParse.test.ts`
- Create: `web/src/components/editor/widgets/tableWidget.ts`
- Modify: `web/src/components/editor/livePreview.ts`
- Modify: `web/src/components/editor/livePreview.test.ts`
- Modify: `web/src/components/editor/livePreview.css`

- [ ] **Step 1: Write the failing test for `parseTable`**

Create `web/src/components/editor/tableParse.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import { parseTable } from "./tableParse";

describe("parseTable", () => {
  it("parses header and body rows, dropping the delimiter row", () => {
    const md = "| A | B |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |";
    expect(parseTable(md)).toEqual({
      header: ["A", "B"],
      rows: [
        ["1", "2"],
        ["3", "4"],
      ],
    });
  });
  it("tolerates missing outer pipes", () => {
    const md = "A | B\n--- | ---\n1 | 2";
    expect(parseTable(md)).toEqual({ header: ["A", "B"], rows: [["1", "2"]] });
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `pnpm test -- tableParse`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement `tableParse.ts`**

Create `web/src/components/editor/tableParse.ts`:

```ts
export interface ParsedTable {
  header: string[];
  rows: string[][];
}

const cells = (line: string): string[] =>
  line
    .trim()
    .replace(/^\|/, "")
    .replace(/\|$/, "")
    .split("|")
    .map((c) => c.trim());

/** Parse a GFM pipe table's source into a header + body rows. Assumes line 2 is
 *  the `---|---` delimiter row and drops it. */
export function parseTable(md: string): ParsedTable {
  const lines = md
    .split("\n")
    .map((l) => l.trim())
    .filter((l) => l.length > 0);
  if (lines.length === 0) return { header: [], rows: [] };
  const header = cells(lines[0]);
  const rows = lines.slice(2).map(cells);
  return { header, rows };
}
```

- [ ] **Step 4: Run to verify pass**

Run: `pnpm test -- tableParse`
Expected: PASS (2 tests).

- [ ] **Step 5: Add the table widget**

Create `web/src/components/editor/widgets/tableWidget.ts`:

```ts
import { WidgetType } from "@codemirror/view";
import { parseTable } from "../tableParse";

export class TableWidget extends WidgetType {
  constructor(readonly md: string) {
    super();
  }
  eq(other: TableWidget): boolean {
    return other.md === this.md;
  }
  toDOM(): HTMLElement {
    const { header, rows } = parseTable(this.md);
    const table = document.createElement("table");
    table.className = "cm-lp-table";
    const thead = table.createTHead();
    const hr = thead.insertRow();
    for (const h of header) {
      const th = document.createElement("th");
      th.textContent = h;
      hr.appendChild(th);
    }
    const tbody = table.createTBody();
    for (const r of rows) {
      const tr = tbody.insertRow();
      for (const c of r) {
        const td = tr.insertCell();
        td.textContent = c;
      }
    }
    return table;
  }
  ignoreEvent(): boolean {
    return true;
  }
}
```

- [ ] **Step 6: Write the failing live-preview test for tables**

Append tests to `livePreview.test.ts`:

```ts
  it("replaces a table with a single block widget off-cursor", () => {
    const doc = "intro\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\nend";
    const at = doc.indexOf("| A");
    const ds = decos(doc, 0);
    expect(ds.some((d) => d.widget && d.from === at)).toBe(true);
  });
  it("reveals the raw table when the cursor is inside it", () => {
    const doc = "intro\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\nend";
    const at = doc.indexOf("| A");
    const ds = decos(doc, doc.indexOf("| 1"));
    expect(ds.some((d) => d.widget && d.from === at)).toBe(false);
  });
```

- [ ] **Step 7: Run to verify failure**

Run: `pnpm test -- livePreview`
Expected: FAIL — table widget not emitted.

- [ ] **Step 8: Emit table decorations in `livePreview.ts`**

Add the import:

```ts
import { TableWidget } from "./widgets/tableWidget";
```

Add a branch inside `tree.iterate`'s `enter` callback (after the `FencedCode` branch):

```ts
      } else if (name === "Table") {
        const start = state.doc.lineAt(from).from;
        const end = state.doc.lineAt(to).to;
        if (!selectionTouches(state, start, end)) {
          const md = state.doc.sliceString(start, end);
          decos.push(
            Decoration.replace({
              widget: new TableWidget(md),
              block: true,
            }).range(start, end),
          );
        }
      }
```

(Note `block: true` — a block-replacing widget must span whole lines; `start`/`end` are line boundaries.)

- [ ] **Step 9: Run to verify pass**

Run: `pnpm test -- livePreview`
Expected: PASS.

- [ ] **Step 10: Style the table in `livePreview.css`**

Append:

```css
.cm-lp-table {
  border-collapse: collapse;
  margin: 0.4em 0;
  font-size: 0.92em;
}
.cm-lp-table th,
.cm-lp-table td {
  border: 1px solid #26262e;
  padding: 5px 11px;
  text-align: left;
}
.cm-lp-table th {
  background: #16161c;
  font-weight: 600;
}
```

- [ ] **Step 11: Per-task gate**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check`
Expected: all PASS.

- [ ] **Step 12: Commit**

```bash
git add web/src/components/editor/tableParse.ts web/src/components/editor/tableParse.test.ts web/src/components/editor/widgets/tableWidget.ts web/src/components/editor/livePreview.ts web/src/components/editor/livePreview.test.ts web/src/components/editor/livePreview.css
git commit -m "feat(editor): render GFM tables in live preview"
```

---

## Task 8: Fixture + e2e + final gate

**Files:**
- Modify: `web/src/client/fixtures.ts`
- Modify: `web/e2e/skeleton.spec.ts`

- [ ] **Step 1: Add a kitchen-sink fixture note**

In `web/src/client/fixtures.ts`, add a key to `FIXTURE_NOTES` (keep the existing notes):

```ts
  "kitchensink.md": `# Kitchen sink

A paragraph with **bold** text and a [[ideas]] link.

- first bullet
- second bullet

- [ ] open task
- [x] done task

> a quoted line

---

\`\`\`js
const x = 1;
\`\`\`

| A | B |
|---|---|
| 1 | 2 |

![logo](img/logo.png)
`,
```

- [ ] **Step 2: Check existing tests still pass with the new fixture**

Run: `pnpm test`
Expected: PASS. If `web/src/client/mock.test.ts` asserts the exact set/count of fixture notes, update that assertion to include `kitchensink.md` (query by membership, not exact length, if possible). Re-run until green.

- [ ] **Step 3: Add the e2e test**

Append to `web/e2e/skeleton.spec.ts`:

```ts
test("document live-preview: blocks render, checkbox toggles, table reveals raw", async ({
  page,
}) => {
  await page.goto("/");
  await page.getByRole("button", { name: "kitchensink.md" }).click();

  // Block elements render in live preview.
  await expect(page.locator(".cm-lp-table")).toBeVisible();
  await expect(page.locator("img.cm-lp-img")).toBeVisible();
  await expect(page.locator(".cm-lp-hr")).toBeVisible();
  await expect(page.locator(".cm-lp-codeblock").first()).toBeVisible();
  await expect(page.locator(".cm-lp-bullet").first()).toBeVisible();

  // The open task renders an unchecked checkbox; clicking it checks the source.
  const openTask = page.locator(".cm-lp-task.unchecked").first();
  await expect(openTask).toBeVisible();
  await openTask.click();
  // After toggle the source has one more checked task than before (now two).
  await expect(page.locator(".cm-lp-task.checked")).toHaveCount(2);

  // Moving the caret into the table reveals the raw pipe markdown.
  await page.locator(".cm-content").click();
  await page.getByText("| A | B |").first();
});
```

(If the last `getByText` is flaky because the table is widget-replaced until the caret is inside, instead click directly on the line that contains the table after entering edit; keep the assertion focused on the rendered `.cm-lp-table` + checkbox toggle, which are the load-bearing behaviors. Adjust selectors to what actually renders, but do not weaken the checkbox-toggle assertion.)

- [ ] **Step 4: Run e2e**

Run: `pnpm e2e`
Expected: all tests PASS (the existing 3 + the new one). Debug selectors against the real rendered DOM if needed; do not weaken the checkbox-toggle or table-render assertions.

- [ ] **Step 5: Final full gate + build**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check && pnpm build`
Expected: all PASS.

- [ ] **Step 6: Manual visual check**

Launch: `pnpm dev --port 5273 --strictPort` and open http://localhost:5273. Open `kitchensink.md` and confirm: centered measure, prose typography, rendered bullets/quote/hr/code-block (syntax-colored)/table/image/checkboxes; clicking a checkbox toggles it; placing the caret in an element reveals its raw markdown and re-renders on leave; the **Source** toggle still shows raw monospace full-width.

- [ ] **Step 7: Commit**

```bash
git add web/src/client/fixtures.ts web/e2e/skeleton.spec.ts
git commit -m "test(e2e): document live-preview fixture + block-render assertions"
```

---

## Notes for the executor

- **GFM is already enabled** via `markdownLanguage` — `Table`, `Blockquote`, `FencedCode`, `ListItem`/`ListMark`, `HorizontalRule` nodes are present. Task markers are detected by regex on the list item's first line (robust across `@lezer/markdown` versions) rather than a version-specific `Task` node name.
- **Atomic ranges:** all widgets flow through the existing `EditorView.atomicRanges` provider in `livePreview()` (it reads the plugin's full decoration set), so new widgets are skipped by caret motion automatically — no extra wiring needed.
- **Reveal-on-cursor:** inline elements gate on the node range; block elements (code, table, hr, block image) gate on the line range via `selectionTouches(state, lineStart, lineEnd)`. The pure builder tests assert both the rendered (off-cursor) and revealed (on-cursor) states for every element.
- **Order of `Decoration.set`:** the orchestrator already calls `Decoration.set(decos, true)` (sorted). Line decorations and block-replace widgets must have correct boundaries (line starts / whole-line spans) — the code above respects this.
- If any `block: true` replace decoration throws a "decorations that replace line breaks must be block" or overlap error, verify the `from`/`to` are exact line boundaries (`lineAt(x).from` … `lineAt(y).to`) and that no inline decoration overlaps the same range (e.g. an image inside a table cell — acceptable to skip inline scans whose range falls inside a table block).
