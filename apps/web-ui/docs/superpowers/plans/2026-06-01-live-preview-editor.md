# Cairn Live Preview Editor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the read-only rendered note view with an Obsidian-style **Live Preview** — an editable CodeMirror surface that styles markdown inline, hides syntax markers, and reveals raw markdown around the cursor.

**Architecture:** A CodeMirror 6 `ViewPlugin` (`livePreview`) driven by a PURE `buildLivePreviewDecorations(state, opts)` that walks the markdown `syntaxTree` (+ a regex pass for `[[wikilinks]]`) and emits `Decoration.mark` (styling) / `Decoration.replace` (hide markers) / widget (clickable wikilinks), made selection-aware so markers reveal when the cursor enters. The buffer stays literal markdown (byte-perfect for git). `Editor` runs CodeMirror in `livepreview` (default, +extension) and `source` (raw) modes; the read-only `MarkdownView` + react-markdown stack are removed.

**Tech Stack:** CodeMirror 6 (`@codemirror/view`, `/state`, `/language`, `/lang-markdown` with `markdownLanguage`/GFM), `@uiw/react-codemirror` (existing). Vitest (headless `EditorState`) for the pure builder; Playwright e2e for the interactive view.

**Reference:** Spec `docs/superpowers/specs/2026-06-01-live-preview-editor-design.md`. **Verified against installed packages:** `@codemirror/view@6.43`, `state@6.6`, `language@6.12`, `@lezer/markdown@1.6`; Lezer node names `ATXHeading1`–`ATXHeading6`, `HeaderMark`, `StrongEmphasis`, `Emphasis`, `EmphasisMark`, `Strikethrough`, `StrikethroughMark`, `InlineCode`, `CodeMark`, `Link`, `LinkMark`, `URL`. GFM (for `Strikethrough`) requires the markdown extension configured with `markdownLanguage` as base. All work under `web/`; run commands from `web/`.

---

## File Structure

```
web/src/components/editor/wikilinkWidget.ts        NEW  CM6 WidgetType for clickable wikilinks
web/src/components/editor/livePreview.ts           NEW  buildLivePreviewDecorations (pure) + livePreview() ViewPlugin
web/src/components/editor/livePreview.test.ts      NEW  headless EditorState decoration tests
web/src/components/editor/livePreview.css          NEW  element styles
web/src/components/Editor.tsx                       MOD  livepreview/source CodeMirror; drop MarkdownView
web/src/components/Editor.test.tsx                  MOD  both modes mount CodeMirror; toggle flips
web/src/store/store.ts                              MOD  editorMode "livepreview"|"source", default livepreview
web/src/store/store.test.ts                         MOD  default editorMode assertion
web/src/app/App.tsx                                 MOD  toggle label/values
web/src/main.tsx                                    MOD  import livePreview.css; drop highlight.js theme import
web/e2e/skeleton.spec.ts                            MOD  live-preview edit + wikilink-click + source toggle
web/package.json                                    MOD  +@codemirror/view,state,language; -react-markdown,remark-gfm,rehype-highlight
REMOVE: web/src/components/MarkdownView.tsx (+test), web/src/components/remarkWikiLink.ts
```

---

## Task 1: Dependencies

**Files:** Modify `web/package.json`.

- [ ] **Step 1: Add the CodeMirror core packages (explicit, since we import from them)**

From `web/`:
```bash
pnpm add @codemirror/view @codemirror/state @codemirror/language
```
(They already resolve transitively; adding them explicitly declares our direct imports.)

- [ ] **Step 2: Verify build + typecheck**

Run (from `web/`): `pnpm typecheck && pnpm build`
Expected: PASS. (react-markdown removal happens in Task 5 with the Editor change.)

- [ ] **Step 3: Commit**

```bash
git add web/package.json web/pnpm-lock.yaml
git commit -m "build: add @codemirror/view,state,language for the live-preview extension"
```

---

## Task 2: wikilinkWidget + buildLivePreviewDecorations (pure)

**Files:** Create `web/src/components/editor/wikilinkWidget.ts`, `web/src/components/editor/livePreview.ts`, `web/src/components/editor/livePreview.test.ts`.

- [ ] **Step 1: Write the failing test**

`web/src/components/editor/livePreview.test.ts`:
```ts
import { describe, it, expect, vi } from "vitest";
import { EditorState, EditorSelection } from "@codemirror/state";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import type { DecorationSet } from "@codemirror/view";
import { buildLivePreviewDecorations } from "./livePreview";

const opts = {
  resolve: (t: string) => (t === "ideas" ? "ideas.md" : null),
  onOpenNote: vi.fn(),
};

interface Deco { from: number; to: number; class?: string; hidden: boolean; widget: boolean }

function decos(doc: string, cursor: number): Deco[] {
  const state = EditorState.create({
    doc,
    selection: EditorSelection.cursor(cursor),
    extensions: [markdown({ base: markdownLanguage })],
  });
  const set: DecorationSet = buildLivePreviewDecorations(state, opts);
  const out: Deco[] = [];
  set.between(0, doc.length, (from, to, value) => {
    const spec = value.spec as { class?: string; widget?: unknown };
    out.push({
      from,
      to,
      class: spec.class,
      widget: spec.widget != null,
      hidden: spec.class == null && spec.widget == null,
    });
  });
  return out;
}

describe("buildLivePreviewDecorations", () => {
  it("styles a heading and hides the # marker when the cursor is elsewhere", () => {
    const doc = "# Title\n\nbody";
    const bodyPos = doc.indexOf("body");
    const ds = decos(doc, bodyPos);
    expect(ds.some((d) => d.class === "cm-lp-h1")).toBe(true);
    // the "# " marker (positions 0..2) is hidden
    expect(ds.some((d) => d.hidden && d.from === 0)).toBe(true);
  });

  it("reveals the # marker when the cursor is on the heading line", () => {
    const doc = "# Title\n\nbody";
    const ds = decos(doc, 3); // cursor inside "Title"
    expect(ds.some((d) => d.hidden && d.from === 0)).toBe(false);
  });

  it("hides ** markers around bold off-cursor", () => {
    const doc = "a **b** c";
    const ds = decos(doc, 0); // cursor at start, not in the bold
    expect(ds.some((d) => d.class === "cm-lp-strong")).toBe(true);
    expect(ds.filter((d) => d.hidden).length).toBeGreaterThanOrEqual(2); // both **
  });

  it("renders a resolved [[wikilink]] as a widget off-cursor", () => {
    const doc = "see [[ideas]] end";
    const ds = decos(doc, 0);
    expect(ds.some((d) => d.widget)).toBe(true);
  });

  it("reveals raw [[wikilink]] when the cursor is inside it", () => {
    const doc = "see [[ideas]] end";
    const ds = decos(doc, 7); // cursor inside [[ideas]]
    expect(ds.some((d) => d.widget)).toBe(false);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test -- livePreview`
Expected: FAIL — cannot find module `./livePreview`.

- [ ] **Step 3: Write the wikilink widget**

`web/src/components/editor/wikilinkWidget.ts`:
```ts
import { WidgetType } from "@codemirror/view";

/** A rendered, clickable wikilink in live preview. Resolved → opens the note. */
export class WikilinkWidget extends WidgetType {
  constructor(
    readonly label: string,
    readonly target: string | null,
    readonly onOpenNote: (path: string) => void,
  ) {
    super();
  }

  eq(other: WikilinkWidget): boolean {
    return other.label === this.label && other.target === this.target;
  }

  toDOM(): HTMLElement {
    const el = document.createElement("span");
    el.className = "cm-lp-wikilink " + (this.target ? "resolved" : "unresolved");
    el.textContent = this.label;
    if (this.target) {
      el.style.cursor = "pointer";
      // mousedown + preventDefault so CodeMirror doesn't move the cursor into the widget.
      el.addEventListener("mousedown", (e) => {
        e.preventDefault();
        this.onOpenNote(this.target!);
      });
    }
    return el;
  }

  ignoreEvent(): boolean {
    return false; // let our mousedown handler run
  }
}
```

- [ ] **Step 4: Write the decoration builder + ViewPlugin**

`web/src/components/editor/livePreview.ts`:
```ts
import { syntaxTree } from "@codemirror/language";
import {
  Decoration,
  EditorView,
  ViewPlugin,
  type DecorationSet,
  type ViewUpdate,
} from "@codemirror/view";
import { type EditorState, type Range } from "@codemirror/state";
import { WikilinkWidget } from "./wikilinkWidget";

export interface LivePreviewOptions {
  resolve: (target: string) => string | null;
  onOpenNote: (path: string) => void;
}

const HEADING_CLASS: Record<string, string> = {
  ATXHeading1: "cm-lp-h1",
  ATXHeading2: "cm-lp-h2",
  ATXHeading3: "cm-lp-h3",
  ATXHeading4: "cm-lp-h4",
  ATXHeading5: "cm-lp-h5",
  ATXHeading6: "cm-lp-h6",
};
const INLINE_CLASS: Record<string, string> = {
  StrongEmphasis: "cm-lp-strong",
  Emphasis: "cm-lp-em",
  Strikethrough: "cm-lp-strike",
  InlineCode: "cm-lp-code",
};
const MARK_CHILD: Record<string, string> = {
  StrongEmphasis: "EmphasisMark",
  Emphasis: "EmphasisMark",
  Strikethrough: "StrikethroughMark",
  InlineCode: "CodeMark",
};

const WIKILINK = /\[\[([^\]]+?)\]\]/g;

function selectionTouches(state: EditorState, from: number, to: number): boolean {
  return state.selection.ranges.some((r) => r.from <= to && r.to >= from);
}

function isInsideCode(state: EditorState, pos: number): boolean {
  let node = syntaxTree(state).resolveInner(pos, 1);
  for (let n: typeof node | null = node; n; n = n.parent) {
    if (/Code/.test(n.name)) return true;
  }
  return false;
}

/** PURE: build the live-preview decoration set for a given editor state. */
export function buildLivePreviewDecorations(
  state: EditorState,
  opts: LivePreviewOptions,
): DecorationSet {
  const decos: Range<Decoration>[] = [];
  const tree = syntaxTree(state);

  tree.iterate({
    enter: (node) => {
      const { name, from, to } = node;
      if (HEADING_CLASS[name]) {
        decos.push(Decoration.mark({ class: HEADING_CLASS[name] }).range(from, to));
        if (!selectionTouches(state, from, to)) {
          const mark = node.node.getChild("HeaderMark");
          if (mark) {
            // hide the #'s and the following space
            const end = Math.min(mark.to + 1, to);
            decos.push(Decoration.replace({}).range(mark.from, end));
          }
        }
      } else if (INLINE_CLASS[name]) {
        decos.push(Decoration.mark({ class: INLINE_CLASS[name] }).range(from, to));
        if (!selectionTouches(state, from, to)) {
          for (const m of node.node.getChildren(MARK_CHILD[name])) {
            decos.push(Decoration.replace({}).range(m.from, m.to));
          }
        }
      } else if (name === "Link") {
        decos.push(Decoration.mark({ class: "cm-lp-link" }).range(from, to));
        if (!selectionTouches(state, from, to)) {
          // hide everything except the link text between the first [ and ]
          const openBracket = node.node.getChild("LinkMark"); // first [
          const url = node.node.getChild("URL");
          if (openBracket) decos.push(Decoration.replace({}).range(openBracket.from, openBracket.to));
          // hide from the closing ] through the end of the link (] + (url))
          const textEnd = url ? url.from : to;
          // closing bracket starts right after link text; hide ]...end
          const closeStart = findCloseBracket(state, openBracket ? openBracket.to : from, to);
          if (closeStart != null) decos.push(Decoration.replace({}).range(closeStart, to));
          void textEnd;
        }
      }
    },
  });

  // Wikilinks: the markdown parser ignores [[...]], so scan the text.
  const text = state.doc.toString();
  WIKILINK.lastIndex = 0;
  let m: RegExpExecArray | null;
  while ((m = WIKILINK.exec(text)) !== null) {
    const from = m.index;
    const to = from + m[0].length;
    if (isInsideCode(state, from)) continue;
    const inner = m[1];
    const target = inner.split("|")[0].trim();
    if (!target) continue;
    if (selectionTouches(state, from, to)) continue; // reveal raw
    const alias = inner.includes("|") ? inner.slice(inner.indexOf("|") + 1).trim() : target;
    const path = opts.resolve(target);
    decos.push(
      Decoration.replace({
        widget: new WikilinkWidget(alias, path, opts.onOpenNote),
      }).range(from, to),
    );
  }

  return Decoration.set(decos, /* sort */ true);
}

/** Find the closing `]` position of a link, between `searchFrom` and `to`. */
function findCloseBracket(state: EditorState, searchFrom: number, to: number): number | null {
  const slice = state.doc.sliceString(searchFrom, to);
  const idx = slice.indexOf("]");
  return idx === -1 ? null : searchFrom + idx;
}

/** The live-preview CodeMirror extension. */
export function livePreview(opts: LivePreviewOptions) {
  return ViewPlugin.fromClass(
    class {
      decorations: DecorationSet;
      constructor(view: EditorView) {
        this.decorations = buildLivePreviewDecorations(view.state, opts);
      }
      update(u: ViewUpdate) {
        if (u.docChanged || u.selectionSet || u.viewportChanged) {
          this.decorations = buildLivePreviewDecorations(u.state, opts);
        }
      }
    },
    {
      decorations: (v) => v.decorations,
      provide: (plugin) =>
        EditorView.atomicRanges.of((view) => view.plugin(plugin)?.decorations ?? Decoration.none),
    },
  );
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `pnpm test -- livePreview`
Expected: PASS (5 tests). If a node name or `getChild`/`getChildren` detail differs from the real parser, console.log `syntaxTree(state)` node names for the test docs and adjust — the verified names are `ATXHeading1`–`6`/`HeaderMark`, `StrongEmphasis`/`Emphasis`/`EmphasisMark`, `Strikethrough`/`StrikethroughMark`, `InlineCode`/`CodeMark`, `Link`/`LinkMark`/`URL`. The `Link` handling is best-effort; if the link test isn't asserted (it isn't in Step 1), getting links perfect is not blocking — keep it from throwing.

- [ ] **Step 6: typecheck + lint**

Run: `pnpm typecheck && pnpm lint`
Expected: clean. (`Range` is imported as a type from `@codemirror/state`.)

- [ ] **Step 7: Commit**

```bash
git add web/src/components/editor/wikilinkWidget.ts web/src/components/editor/livePreview.ts web/src/components/editor/livePreview.test.ts
git commit -m "feat: live-preview decoration builder + wikilink widget (pure, CM6)"
```

---

## Task 3: Live-preview styles

**Files:** Create `web/src/components/editor/livePreview.css`.

- [ ] **Step 1: Write the CSS**

`web/src/components/editor/livePreview.css`:
```css
.cm-lp-h1 { font-size: 1.6em; font-weight: 700; }
.cm-lp-h2 { font-size: 1.4em; font-weight: 700; }
.cm-lp-h3 { font-size: 1.2em; font-weight: 600; }
.cm-lp-h4 { font-size: 1.1em; font-weight: 600; }
.cm-lp-h5 { font-size: 1em; font-weight: 600; }
.cm-lp-h6 { font-size: 0.9em; font-weight: 600; color: #9aa0b4; }
.cm-lp-strong { font-weight: 700; }
.cm-lp-em { font-style: italic; }
.cm-lp-strike { text-decoration: line-through; }
.cm-lp-code {
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  background: #2a2a36;
  border-radius: 3px;
  padding: 0 3px;
}
.cm-lp-link { color: #7aa2ff; }
.cm-lp-wikilink.resolved { color: #7aa2ff; cursor: pointer; }
.cm-lp-wikilink.resolved:hover { text-decoration: underline; }
.cm-lp-wikilink.unresolved { color: #8b90a0; text-decoration: underline dotted; }
```

- [ ] **Step 2: Commit**

```bash
git add web/src/components/editor/livePreview.css
git commit -m "feat: live-preview element styles"
```

---

## Task 4: Store editorMode → livepreview|source

**Files:** Modify `web/src/store/store.ts`, `web/src/store/store.test.ts`.

- [ ] **Step 1: Write the failing test**

Add to `web/src/store/store.test.ts` (inside `describe("cairn store", ...)`), and UPDATE the existing `editorMode` default test if present (the Phase-3 test asserted `"rendered"`):
```ts
  it("defaults the editor to live preview", () => {
    expect(DEFAULT_SETTINGS.editorMode).toBe("livepreview");
  });
```
If a prior test asserts `DEFAULT_SETTINGS.editorMode === "rendered"`, change it to `"livepreview"` (or delete it in favor of the above).

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test -- store`
Expected: FAIL — editorMode is `"rendered"`.

- [ ] **Step 3: Change the type and default**

In `web/src/store/store.ts`:
- `Settings.editorMode`: change `"rendered" | "source"` → `"livepreview" | "source"`.
- `DEFAULT_SETTINGS.editorMode`: change `"rendered"` → `"livepreview"`.

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm test -- store`
Expected: PASS (App.tsx/Editor.tsx may still show type errors from the old `"rendered"` literal until Tasks 5–6 — full typecheck not required yet; just `pnpm test -- store`).

- [ ] **Step 5: Commit**

```bash
git add web/src/store/store.ts web/src/store/store.test.ts
git commit -m "feat: editorMode is livepreview|source, defaulting to livepreview"
```

---

## Task 5: Editor — live preview + source modes; remove MarkdownView

**Files:** Modify `web/src/components/Editor.tsx`, `web/src/components/Editor.test.tsx`; remove `web/src/components/MarkdownView.tsx`, `web/src/components/MarkdownView.test.tsx`, `web/src/components/remarkWikiLink.ts`; modify `web/package.json`.

- [ ] **Step 1: Write the failing test (replace the file)**

Replace `web/src/components/Editor.test.tsx` with:
```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Editor } from "./Editor";

const base = {
  path: "a.md" as string | null,
  value: "# Hi\n\nlink [[ideas]]",
  notePaths: ["ideas.md"],
  mode: "livepreview" as "livepreview" | "source",
  onChange: vi.fn(),
  onOpenNote: vi.fn(),
  onToggleMode: vi.fn(),
};

describe("Editor", () => {
  it("shows a placeholder when no note is open", () => {
    render(<Editor {...base} path={null} />);
    expect(screen.getByText(/no note open/i)).toBeInTheDocument();
  });

  it("live preview mode mounts a CodeMirror editor", () => {
    const { container } = render(<Editor {...base} mode="livepreview" />);
    expect(container.querySelector(".cm-editor")).not.toBeNull();
  });

  it("source mode mounts a CodeMirror editor", () => {
    const { container } = render(<Editor {...base} mode="source" />);
    expect(container.querySelector(".cm-editor")).not.toBeNull();
  });

  it("the toggle button flips the mode", async () => {
    const onToggleMode = vi.fn();
    render(<Editor {...base} mode="livepreview" onToggleMode={onToggleMode} />);
    await userEvent.click(screen.getByRole("button", { name: /source/i }));
    expect(onToggleMode).toHaveBeenCalled();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test -- Editor`
Expected: FAIL — current Editor uses `"rendered"`/`MarkdownView` and prop type mismatch.

- [ ] **Step 3: Rewrite the Editor (replace the file)**

Replace `web/src/components/Editor.tsx` with:
```tsx
import { useMemo } from "react";
import CodeMirror from "@uiw/react-codemirror";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { livePreview } from "./editor/livePreview";
import { stem } from "../client/wikilink";

export function Editor(props: {
  path: string | null;
  value: string;
  mode: "livepreview" | "source";
  notePaths: string[];
  onChange: (value: string) => void;
  onOpenNote: (path: string) => void;
  onToggleMode: () => void;
}) {
  const resolve = useMemo(() => {
    const byStem = new Map<string, string>();
    for (const p of props.notePaths) byStem.set(stem(p), p);
    return (target: string) => byStem.get(stem(target)) ?? null;
  }, [props.notePaths]);

  const extensions = useMemo(() => {
    const base = markdown({ base: markdownLanguage });
    return props.mode === "livepreview"
      ? [base, livePreview({ resolve, onOpenNote: props.onOpenNote })]
      : [base];
    // onOpenNote is stable (a store action); resolve changes with notePaths.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.mode, resolve]);

  if (!props.path) {
    return (
      <div className="text-sm text-neutral-500">No note open. Pick one from the list.</div>
    );
  }
  return (
    <div className="flex h-full flex-col">
      <div className="mb-2 flex items-center justify-between">
        <span className="text-sm text-neutral-300">{props.path}</span>
        <button
          className="rounded border border-neutral-700 px-2 py-0.5 text-xs text-neutral-300 hover:bg-neutral-800"
          onClick={props.onToggleMode}
        >
          {props.mode === "livepreview" ? "Source" : "Live Preview"}
        </button>
      </div>
      <CodeMirror
        value={props.value}
        height="100%"
        theme="dark"
        extensions={extensions}
        onChange={props.onChange}
      />
    </div>
  );
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm test -- Editor`
Expected: PASS (4 tests). `@uiw/react-codemirror` mounts `.cm-editor` in jsdom; the live-preview decorations themselves are not asserted here (e2e covers them).

- [ ] **Step 5: Remove the now-dead rendered view + deps**

```bash
git rm web/src/components/MarkdownView.tsx web/src/components/MarkdownView.test.tsx web/src/components/remarkWikiLink.ts
cd web && pnpm remove react-markdown remark-gfm rehype-highlight && cd ..
```
Confirm nothing else imports them: `grep -rn "MarkdownView\|remarkWikiLink\|react-markdown" web/src` → only `web/src/main.tsx` may import the highlight.js theme CSS (handled in Task 6). If `grep` finds other references, fix them.

- [ ] **Step 6: Run the unit suite + typecheck + lint**

Run (from `web/`): `pnpm test && pnpm typecheck`
Expected: PASS, EXCEPT `App.tsx` may still error on the toggle's `"rendered"` literal — fixed in Task 6. If `pnpm typecheck` only errors in App.tsx, that's expected; `pnpm test` should pass.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat: Editor live-preview + source modes (CodeMirror); remove rendered view + react-markdown"
```

---

## Task 6: App wiring + CSS import + full gate

**Files:** Modify `web/src/app/App.tsx`, `web/src/main.tsx`.

- [ ] **Step 1: Update the toggle in App**

In `web/src/app/App.tsx`, the Editor's `onToggleMode` currently flips `"rendered"`/`"source"`. Change that handler to flip `"livepreview"`/`"source"`:
```tsx
                onToggleMode={() =>
                  actions.setSettings({
                    editorMode:
                      editorMode === "livepreview" ? "source" : "livepreview",
                  })
                }
```
(Leave the rest of the `<Editor … />` props and the graph/editor center-swap unchanged.)

- [ ] **Step 2: Fix the CSS imports in main.tsx**

In `web/src/main.tsx`:
- Add: `import "./components/editor/livePreview.css";`
- Remove the now-unused highlight.js theme import: delete `import "highlight.js/styles/github-dark.css";` (it was only for the removed rendered view's code blocks).

- [ ] **Step 3: Full gate**

Run (from `web/`): `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check && pnpm build`
Expected: all PASS. If `format:check` fails, run `pnpm format` and include the changes. Confirm no dangling references to the removed files/deps.

- [ ] **Step 4: Commit**

```bash
git add web/src/app/App.tsx web/src/main.tsx
git commit -m "feat: wire live-preview toggle; import live-preview CSS; drop highlight theme"
```

---

## Task 7: e2e — edit in live preview, wikilink click, source toggle

**Files:** Modify `web/e2e/skeleton.spec.ts`.

- [ ] **Step 1: Update the editing flow + add live-preview assertions**

The first test currently creates `fresh.md`, clicks **"Edit source"**, types into `.cm-content`, clicks **"Done"**, and asserts a rendered `link "ideas"`. With live preview, the note opens **editable** by default in live preview — there's no separate read-only render and the toggle labels are now **"Source"** / **"Live Preview"**. Update that section. Replace the block that currently reads (from the editor-rework cycle):
```ts
  // New note opens in the rendered view; toggle to source (CodeMirror) to type.
  await page.getByRole("button", { name: /edit source/i }).click();
  const cm = page.locator(".cm-content");
  await cm.click();
  await cm.fill("a new note pointing at [[ideas]]");
  // Back to the rendered view; the wikilink renders as a clickable link.
  await page.getByRole("button", { name: /^done$/i }).click();
  await expect(page.getByRole("link", { name: "ideas" })).toBeVisible();
```
with:
```ts
  // New note opens editable in live preview — type directly into CodeMirror.
  const cm = page.locator(".cm-content");
  await cm.click();
  await cm.fill("a new note pointing at [[ideas]]");
  // In live preview the wikilink renders as a clickable widget (not raw [[…]]).
  await expect(page.locator(".cm-lp-wikilink", { hasText: "ideas" })).toBeVisible();
```
(If `.fill` doesn't register with CodeMirror, use `await cm.click(); await page.keyboard.type("a new note pointing at [[ideas]]");`. Keep the downstream search/backlink/commit assertions unchanged — autosave still fires from CodeMirror edits.)

- [ ] **Step 2: Add a live-preview-specific test**

Append a new test to `web/e2e/skeleton.spec.ts`:
```ts
test("live preview: heading renders, cursor reveals syntax, wikilink opens note", async ({
  page,
}) => {
  await page.goto("/");
  // index.md = "# Index\n\nStart at [[ideas]] or the [[todo]] list."
  await page.getByRole("button", { name: "index.md" }).click();

  // Heading is styled (the "# " marker is hidden in live preview).
  await expect(page.locator(".cm-lp-h1")).toBeVisible();

  // The [[ideas]] wikilink renders as a clickable widget; clicking opens ideas.md.
  await page.locator(".cm-lp-wikilink", { hasText: "ideas" }).first().click();
  await expect(page.locator(".cm-editor .cm-content")).toContainText("Ideas");

  // Toggle to Source shows raw markdown (the literal "# Ideas").
  await page.getByRole("button", { name: /^source$/i }).click();
  await expect(page.locator(".cm-content")).toContainText("# Ideas");
});
```

- [ ] **Step 2b: Run the e2e**

Run (from `web/`): `pnpm e2e`
Expected: PASS (both tests). If port 5173 is held, `lsof -ti:5173 | xargs kill` and retry. If a selector is flaky: the live-preview widget has class `cm-lp-wikilink`; the heading style class is `cm-lp-h1`; raw source text appears in `.cm-content`. Adjust selectors to those classes if needed; do NOT weaken the intent (heading styled, wikilink clickable+opens, source shows raw).

- [ ] **Step 3: Commit**

```bash
git add web/e2e/skeleton.spec.ts
git commit -m "test(e2e): live-preview heading/wikilink + source toggle"
```

---

## Done criteria

- Notes open **editable** in a live-preview CodeMirror: headings/bold/italic/strikethrough/inline-code styled inline, syntax markers hidden and **revealed when the cursor enters**, `[[wikilinks]]` rendered as clickable widgets (resolved → open the note, unresolved muted). A **Source** toggle shows raw markdown. The buffer is always literal markdown; autosave/commit unchanged.
- The read-only rendered view + react-markdown stack are removed; `editorMode` is `"livepreview" | "source"` default `"livepreview"`.
- `buildLivePreviewDecorations` is unit-tested headlessly; interactive behavior is covered by e2e. `pnpm test`/`typecheck`/`lint`/`format:check`/`build` clean; both e2e tests pass. Tauri/desktop unaffected.
```
