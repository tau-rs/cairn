# Cairn Editor Rework (Rendered-Default) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the note editor open in a beautiful rendered markdown view (GitHub-README style) by default, with CodeMirror source editing as a secondary toggled mode, and clickable `[[wikilinks]]` in the rendered view.

**Architecture:** A new pure `MarkdownView` component renders the live note buffer via `react-markdown` + `remark-gfm` + a small custom `remarkWikiLink` plugin + `rehype-highlight`, styled with `@tailwindcss/typography`. `Editor` toggles between `MarkdownView` (mode `"rendered"`, default) and CodeMirror (mode `"source"`); the plain textarea is removed. The markdown source stays the single source of truth; both modes read the same `activeContents` buffer; autosave fires from source edits.

**Tech Stack:** React 18 + TS + Tailwind 3 (existing); add `react-markdown`, `remark-gfm`, `rehype-highlight`, `unist-util-visit`, `@tailwindcss/typography`, `highlight.js`. Vitest + Testing Library; Playwright e2e.

**Reference:** Spec `docs/superpowers/specs/2026-06-01-editor-rework-design.md`. Decision: custom `remarkWikiLink` plugin (not third-party `remark-wiki-link`) — the `[[…]]` grammar is simple and we already encode the rules in `web/src/client/wikilink.ts` (`stem()`). All work is under `web/`; run commands from `web/`.

---

## File Structure

```
web/src/components/
  remarkWikiLink.ts        NEW  custom remark plugin: [[target]] -> <a class="wikilink resolved|unresolved" data-wikilink-target=…>
  MarkdownView.tsx         NEW  rendered markdown view (react-markdown + plugins + wikilink <a> renderer + prose wrapper)
  MarkdownView.test.tsx    NEW
  Editor.tsx               MOD  rendered⇄source toggle; render MarkdownView | CodeMirror; drop textarea; +notePaths/onOpenNote props
  Editor.test.tsx          MOD
web/src/store/store.ts     MOD  editorMode: "rendered"|"source"; DEFAULT_SETTINGS.editorMode = "rendered"
web/src/store/store.test.ts MOD adds DEFAULT_SETTINGS.editorMode assertion
web/src/app/App.tsx        MOD  pass notePaths/openNote to Editor; toggle flips rendered/source
web/src/main.tsx           MOD  import highlight.js github-dark theme CSS
web/tailwind.config.ts     MOD  add @tailwindcss/typography plugin
web/package.json           MOD  add deps
web/e2e/skeleton.spec.ts   MOD  edit step toggles to source + types into CodeMirror; assert rendered view
```

---

## Task 1: Add dependencies, typography plugin, and highlight theme

**Files:** Modify `web/package.json`, `web/tailwind.config.ts`, `web/src/main.tsx`.

- [ ] **Step 1: Install dependencies**

From `web/`:
```bash
pnpm add react-markdown remark-gfm rehype-highlight unist-util-visit highlight.js
pnpm add -D @tailwindcss/typography @types/mdast
```

- [ ] **Step 2: Enable the Tailwind Typography plugin**

Edit `web/tailwind.config.ts` to:
```ts
import type { Config } from "tailwindcss";
import typography from "@tailwindcss/typography";

export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: { extend: {} },
  plugins: [typography],
} satisfies Config;
```

- [ ] **Step 3: Import the highlight.js theme**

In `web/src/main.tsx`, add this import (with the other imports, after `./index.css`):
```ts
import "highlight.js/styles/github-dark.css";
```

- [ ] **Step 4: Verify build + typecheck**

Run (from `web/`): `pnpm typecheck && pnpm build`
Expected: PASS (deps resolve; typography plugin loads; theme CSS bundles). The chunk-size advisory is expected.

- [ ] **Step 5: Commit**

```bash
git add web/package.json web/pnpm-lock.yaml web/tailwind.config.ts web/src/main.tsx
git commit -m "build: add react-markdown + tailwind typography + highlight.js theme"
```

---

## Task 2: remarkWikiLink plugin + MarkdownView component

**Files:** Create `web/src/components/remarkWikiLink.ts`, `web/src/components/MarkdownView.tsx`, `web/src/components/MarkdownView.test.tsx`.

- [ ] **Step 1: Write the failing test**

`web/src/components/MarkdownView.test.tsx`:
```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MarkdownView } from "./MarkdownView";

describe("MarkdownView", () => {
  it("renders GFM markdown as HTML", () => {
    render(
      <MarkdownView contents={"# Title\n\n- a\n- b"} notePaths={[]} onOpenNote={vi.fn()} />,
    );
    expect(screen.getByRole("heading", { name: "Title" })).toBeInTheDocument();
    expect(screen.getAllByRole("listitem")).toHaveLength(2);
  });

  it("renders a resolved [[wikilink]] that opens the note on click", async () => {
    const onOpenNote = vi.fn();
    render(
      <MarkdownView contents={"see [[ideas]] here"} notePaths={["ideas.md"]} onOpenNote={onOpenNote} />,
    );
    await userEvent.click(screen.getByText("ideas"));
    expect(onOpenNote).toHaveBeenCalledWith("ideas.md");
  });

  it("renders an unresolved [[wikilink]] without opening anything", async () => {
    const onOpenNote = vi.fn();
    render(
      <MarkdownView contents={"see [[missing]]"} notePaths={["ideas.md"]} onOpenNote={onOpenNote} />,
    );
    await userEvent.click(screen.getByText("missing"));
    expect(onOpenNote).not.toHaveBeenCalled();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test -- MarkdownView`
Expected: FAIL — cannot find module `./MarkdownView`.

- [ ] **Step 3: Write the remark plugin**

`web/src/components/remarkWikiLink.ts`:
```ts
import { visit, SKIP } from "unist-util-visit";
import type { Root, Text } from "mdast";

export interface WikiLinkOptions {
  /** Resolve a wikilink target to a note path, or null if it doesn't exist. */
  resolve: (target: string) => string | null;
}

const WIKILINK = /\[\[([^\]]+?)\]\]/g;

/** Turn `[[target]]` / `[[target|alias]]` text into link nodes carrying a
 *  `wikilink resolved|unresolved` class and a `data-wikilink-target` path. */
export function remarkWikiLink({ resolve }: WikiLinkOptions) {
  return (tree: Root) => {
    visit(tree, "text", (node: Text, index, parent) => {
      if (!parent || index === null || index === undefined) return;
      const value = node.value;
      if (!value.includes("[[")) return;

      const out: unknown[] = [];
      let last = 0;
      let m: RegExpExecArray | null;
      WIKILINK.lastIndex = 0;
      while ((m = WIKILINK.exec(value)) !== null) {
        const inner = m[1];
        const target = inner.split("|")[0].trim();
        if (!target) continue;
        const alias = inner.includes("|") ? inner.slice(inner.indexOf("|") + 1).trim() : target;
        if (m.index > last) out.push({ type: "text", value: value.slice(last, m.index) });
        const path = resolve(target);
        out.push({
          type: "link",
          url: "#",
          children: [{ type: "text", value: alias }],
          data: {
            hName: "a",
            hProperties: {
              className: ["wikilink", path ? "resolved" : "unresolved"],
              "data-wikilink-target": path ?? "",
            },
          },
        });
        last = m.index + m[0].length;
      }
      if (out.length === 0) return;
      if (last < value.length) out.push({ type: "text", value: value.slice(last) });

      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      parent.children.splice(index, 1, ...(out as any[]));
      return [SKIP, index + out.length];
    });
  };
}
```

- [ ] **Step 4: Write the MarkdownView component**

`web/src/components/MarkdownView.tsx`:
```tsx
import { useMemo } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypeHighlight from "rehype-highlight";
import { remarkWikiLink } from "./remarkWikiLink";
import { stem } from "../client/wikilink";

export function MarkdownView(props: {
  contents: string;
  notePaths: string[];
  onOpenNote: (path: string) => void;
}) {
  const resolve = useMemo(() => {
    const byStem = new Map<string, string>();
    for (const p of props.notePaths) byStem.set(stem(p), p);
    return (target: string) => byStem.get(stem(target)) ?? null;
  }, [props.notePaths]);

  return (
    <div className="prose prose-invert max-w-none">
      <ReactMarkdown
        remarkPlugins={[remarkGfm, [remarkWikiLink, { resolve }]]}
        rehypePlugins={[rehypeHighlight]}
        components={{
          a({ className, children, href, ...rest }) {
            const cls = Array.isArray(className) ? className.join(" ") : (className ?? "");
            if (cls.includes("wikilink")) {
              const target = (rest as Record<string, unknown>)["data-wikilink-target"] as string;
              if (cls.includes("unresolved") || !target) {
                return (
                  <span className="text-neutral-500 underline decoration-dotted">{children}</span>
                );
              }
              return (
                <a
                  className="cursor-pointer text-sky-400 no-underline hover:underline"
                  onClick={(e) => {
                    e.preventDefault();
                    props.onOpenNote(target);
                  }}
                >
                  {children}
                </a>
              );
            }
            return (
              <a href={href} target="_blank" rel="noreferrer">
                {children}
              </a>
            );
          },
        }}
      >
        {props.contents}
      </ReactMarkdown>
    </div>
  );
}
```
Note: if TS objects to the `remarkPlugins` tuple type, cast that array to `as never` or import `PluggableList` and annotate; the runtime shape (`[plugin, options]`) is correct. If react-markdown's `components.a` prop type doesn't include `data-wikilink-target`, reading it via `(rest as Record<string, unknown>)[…]` (as written) avoids the type error.

- [ ] **Step 5: Run test to verify it passes**

Run: `pnpm test -- MarkdownView`
Expected: PASS (3 tests). `react-markdown` and `rehype-highlight` run synchronously under jsdom.

- [ ] **Step 6: Commit**

```bash
git add web/src/components/remarkWikiLink.ts web/src/components/MarkdownView.tsx web/src/components/MarkdownView.test.tsx
git commit -m "feat: MarkdownView rendered view with GFM + clickable wikilinks"
```

---

## Task 3: Rename editorMode to rendered/source, default rendered

**Files:** Modify `web/src/store/store.ts`, `web/src/store/store.test.ts`.

- [ ] **Step 1: Write the failing test**

Add to `web/src/store/store.test.ts` (inside `describe("cairn store", ...)`):
```ts
  it("defaults the editor to the rendered view", () => {
    expect(DEFAULT_SETTINGS.editorMode).toBe("rendered");
  });
```
(`DEFAULT_SETTINGS` is already imported in this test file.)

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test -- store`
Expected: FAIL — `editorMode` is currently `"rich"`.

- [ ] **Step 3: Change the type and default**

In `web/src/store/store.ts`:
- In the `Settings` interface, change `editorMode: "rich" | "raw";` to `editorMode: "rendered" | "source";`.
- In `DEFAULT_SETTINGS`, change `editorMode: "rich",` to `editorMode: "rendered",`.

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm test -- store`
Expected: PASS. (Other store tests are unaffected — none assert the old value.)

- [ ] **Step 5: Commit**

```bash
git add web/src/store/store.ts web/src/store/store.test.ts
git commit -m "feat: editorMode is rendered|source, defaulting to rendered"
```

---

## Task 4: Editor — rendered⇄source toggle, drop textarea

**Files:** Modify `web/src/components/Editor.tsx`, `web/src/components/Editor.test.tsx`.

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
  mode: "rendered" as "rendered" | "source",
  onChange: vi.fn(),
  onOpenNote: vi.fn(),
  onToggleMode: vi.fn(),
};

describe("Editor", () => {
  it("shows a placeholder when no note is open", () => {
    render(<Editor {...base} path={null} />);
    expect(screen.getByText(/no note open/i)).toBeInTheDocument();
  });

  it("rendered mode shows the rendered markdown (a heading), not a source editor", () => {
    render(<Editor {...base} mode="rendered" />);
    expect(screen.getByRole("heading", { name: "Hi" })).toBeInTheDocument();
  });

  it("the toggle button flips the mode", async () => {
    const onToggleMode = vi.fn();
    render(<Editor {...base} mode="rendered" onToggleMode={onToggleMode} />);
    await userEvent.click(screen.getByRole("button", { name: /edit source/i }));
    expect(onToggleMode).toHaveBeenCalled();
  });

  it("source mode renders the CodeMirror editor", () => {
    const { container } = render(<Editor {...base} mode="source" />);
    expect(container.querySelector(".cm-editor")).not.toBeNull();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test -- Editor`
Expected: FAIL — current `Editor` has no `notePaths`/`onOpenNote` props, mode `"rich"|"raw"`, and renders a textarea, not `MarkdownView`.

- [ ] **Step 3: Rewrite the Editor (replace the file)**

Replace `web/src/components/Editor.tsx` with:
```tsx
import CodeMirror from "@uiw/react-codemirror";
import { markdown } from "@codemirror/lang-markdown";
import { MarkdownView } from "./MarkdownView";

export function Editor(props: {
  path: string | null;
  value: string;
  mode: "rendered" | "source";
  notePaths: string[];
  onChange: (value: string) => void;
  onOpenNote: (path: string) => void;
  onToggleMode: () => void;
}) {
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
          {props.mode === "rendered" ? "Edit source" : "Done"}
        </button>
      </div>
      {props.mode === "rendered" ? (
        <div className="h-full overflow-auto">
          <MarkdownView
            contents={props.value}
            notePaths={props.notePaths}
            onOpenNote={props.onOpenNote}
          />
        </div>
      ) : (
        <CodeMirror
          value={props.value}
          height="100%"
          theme="dark"
          extensions={[markdown()]}
          onChange={props.onChange}
        />
      )}
    </div>
  );
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm test -- Editor`
Expected: PASS. If the `.cm-editor` assertion is flaky under jsdom (CodeMirror mount timing), replace that test's body with an assertion that the heading is NOT present in source mode (`expect(screen.queryByRole("heading")).toBeNull()`) — but try `.cm-editor` first; @uiw/react-codemirror mounts synchronously.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/Editor.tsx web/src/components/Editor.test.tsx
git commit -m "feat: Editor toggles rendered view and CodeMirror source (no textarea)"
```

---

## Task 5: Wire the Editor in App + full gate

**Files:** Modify `web/src/app/App.tsx`.

- [ ] **Step 1: Update the Editor wiring**

In `web/src/app/App.tsx`, replace the `<Editor … />` element (inside the `editor={…}` Shell prop) with:
```tsx
            <Editor
              path={activePath}
              value={activeContents}
              mode={editorMode}
              notePaths={notePaths}
              onChange={actions.editBuffer}
              onOpenNote={actions.openNote}
              onToggleMode={() =>
                actions.setSettings({
                  editorMode: editorMode === "rendered" ? "source" : "rendered",
                })
              }
            />
```
(`notePaths`, `activeContents`, `editorMode`, and `actions` are already selected/defined in `App`. No new imports needed — `Editor` is already imported.)

- [ ] **Step 2: Run the full gate**

Run (from `web/`): `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check && pnpm build`
Expected: all PASS. If `format:check` fails, run `pnpm format` and include the changes. If `lint` flags the `eslint-disable` comment in `remarkWikiLink.ts` as unused, remove it.

- [ ] **Step 3: Commit**

```bash
git add web/src/app/App.tsx
git commit -m "feat: wire rendered-default editor (notePaths + openNote + mode toggle)"
```

---

## Task 6: Update the e2e for the new edit flow

**Files:** Modify `web/e2e/skeleton.spec.ts`.

- [ ] **Step 1: Update the edit step**

In `web/e2e/skeleton.spec.ts`, the note is now created and shown in the **rendered** view by default. Replace the old "switch to raw / fill textarea" lines with: toggle into source, type into CodeMirror, then (optionally) toggle back to confirm the rendered view. Concretely, replace:
```ts
  // Switch to raw mode for deterministic typing in the textarea.
  await page.getByRole("button", { name: /switch to raw/i }).click();
  // `locator("textarea")` (not getByRole("textbox")) — the Search input is also a textbox.
  await page.locator("textarea").fill("a new note pointing at [[ideas]]");
```
with:
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
Keep every other step/assertion in the test unchanged (fixture notes listed, backlinks, autosave "Saved", search "pointing" → open `fresh.md`, re-open `ideas.md` → `fresh.md` backlink, manual commit → `@c####`).

- [ ] **Step 2: Run the e2e**

Run: `pnpm e2e`
Expected: PASS. If `.cm-content` `.fill()` doesn't register with CodeMirror, use `await cm.click(); await page.keyboard.type("a new note pointing at [[ideas]]");` instead (do NOT loosen any assertion). If port 5173 is held by a stale dev server, `lsof -ti:5173 | xargs kill` and retry.

- [ ] **Step 3: Commit**

```bash
git add web/e2e/skeleton.spec.ts
git commit -m "test(e2e): edit via source toggle + CodeMirror; assert rendered wikilink"
```

---

## Done criteria

- Opening a note shows a **beautiful rendered markdown view** (GFM, prose typography, highlighted code); `[[wikilinks]]` are clickable (resolved → opens the note; unresolved → muted, inert).
- An **"Edit source"** toggle switches to CodeMirror; edits autosave; toggling back re-renders the live buffer. The plain textarea is gone; `editorMode` is `"rendered" | "source"`, default `"rendered"`.
- All web unit/component tests pass; the e2e passes with the new edit flow; `typecheck`/`lint`/`format:check`/`build` clean on the mock. The Tauri/desktop side is unaffected (presentation-only change).
```
