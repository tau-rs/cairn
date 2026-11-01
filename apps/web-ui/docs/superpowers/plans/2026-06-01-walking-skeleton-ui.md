# Cairn Web UI Walking-Skeleton Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a working three-pane Cairn note UI (list/create/edit/delete, search, backlinks, autosave, configurable auto-commit, manual commit, live event refresh) running against a faithful in-browser mock of the engine.

**Architecture:** The UI is written against a single transport-abstracted `CairnClient` interface (`sendCommand` / `runQuery` / `subscribe`). The skeleton's only implementation is `MockClient`, an in-memory cairn that reproduces the engine's exact semantics (wikilink-by-stem backlinks, case-insensitive substring search over body+path, `note_changed`→`reindexed` event ordering). A Zustand store orchestrates all engine interaction; React components are pure consumers. Swapping in a `TauriClient` at Phase 2 touches only the composition root.

**Tech Stack:** React 18 + Vite 5 + TypeScript 5 + Tailwind 3 + Zustand 4 + react-router-dom 6; CodeMirror 6 via `@uiw/react-codemirror` + `@codemirror/lang-markdown`; Vitest + Testing Library + jsdom for unit/component, Playwright for e2e; pnpm. App lives in `web/` (Phase 2 adds a Rust/Tauri sibling).

**Reference:** Spec at `docs/superpowers/specs/2026-06-01-walking-skeleton-ui-design.md`. Engine contract source: `tau-rs/cairn` @ commit `166b0eae622ee219b11b23046b85f369da1cb316`, bindings at `crates/cairn-contract/bindings/`. The `cairn` engine repo is assumed to be a sibling of this repo (`../cairn`).

---

## File Structure

```
web/
  package.json              pnpm scripts + deps
  tsconfig.json             TS config (app)
  tsconfig.node.json        TS config (vite/config files)
  vite.config.ts            Vite + Vitest config
  tailwind.config.ts        Tailwind content globs
  postcss.config.js         Tailwind/autoprefixer
  .eslintrc.cjs             eslint
  .prettierrc               prettier
  playwright.config.ts      e2e config
  index.html                Vite entry
  src/
    main.tsx                React mount
    index.css               Tailwind directives
    vitest.setup.ts         jest-dom matchers
    contract/
      Command.ts            vendored from engine
      Query.ts              vendored from engine
      Event.ts              vendored from engine
      source.ts             pinned source commit constant
      index.ts              barrel re-export
    client/
      types.ts              CairnClient, QueryResult, CommandResult, Unsubscribe
      wikilink.ts           extractLinks(), stem()
      fixtures.ts           seed fixture cairn
      mock.ts               MockClient
    util/
      timer.ts              debounce()
    store/
      store.ts              createCairnStore(client) + CairnState
    app/
      cairnStore.ts         app store instance + useCairn hook
      makeClient.ts         composition root: chooses MockClient
      App.tsx               top-level layout wiring
    components/
      Shell.tsx             three-pane + top bar frame
      TopBar.tsx            SearchBar + CommitBar
      SearchBar.tsx         query input
      SearchResults.tsx     results overlay
      CommitBar.tsx         save/commit status + manual commit
      NoteList.tsx          notes + new/delete
      Editor.tsx            rich (CodeMirror) / raw toggle
      Backlinks.tsx         backlinks of active note
      Settings.tsx          auto-commit + editor settings panel
      ErrorToast.tsx        non-blocking error surface
  e2e/
    skeleton.spec.ts        full-loop e2e
scripts/
  sync-contract.sh          copy bindings from ../cairn, record commit
.github/workflows/ci.yml    typecheck + lint + unit + build + e2e
```

All commands below are run from `web/` unless stated otherwise.

---

## Task 1: Scaffold the web app

**Files:**
- Create: `web/package.json`, `web/tsconfig.json`, `web/tsconfig.node.json`, `web/vite.config.ts`, `web/tailwind.config.ts`, `web/postcss.config.js`, `web/.eslintrc.cjs`, `web/.prettierrc`, `web/index.html`, `web/src/main.tsx`, `web/src/index.css`, `web/src/vitest.setup.ts`, `web/src/app/App.tsx`

- [ ] **Step 1: Create `web/package.json`**

```json
{
  "name": "cairn-web-ui",
  "private": true,
  "type": "module",
  "packageManager": "pnpm@10.14.0",
  "scripts": {
    "dev": "vite",
    "build": "tsc -b && vite build",
    "preview": "vite preview --port 5173",
    "test": "vitest run",
    "test:watch": "vitest",
    "e2e": "playwright test",
    "typecheck": "tsc --noEmit",
    "lint": "eslint .",
    "format": "prettier --write .",
    "format:check": "prettier --check ."
  },
  "dependencies": {
    "@codemirror/lang-markdown": "^6",
    "@uiw/react-codemirror": "^4",
    "react": "^18",
    "react-dom": "^18",
    "react-router-dom": "^6.30.4",
    "zustand": "^4"
  },
  "devDependencies": {
    "@playwright/test": "^1.45",
    "@testing-library/jest-dom": "^6",
    "@testing-library/react": "^16",
    "@testing-library/user-event": "^14",
    "@types/react": "^18",
    "@types/react-dom": "^18",
    "@typescript-eslint/eslint-plugin": "^8",
    "@typescript-eslint/parser": "^8",
    "@vitejs/plugin-react": "^4",
    "autoprefixer": "^10.5.0",
    "eslint": "^8.57.0",
    "eslint-config-prettier": "^9.1.2",
    "eslint-plugin-react-hooks": "^4.6.2",
    "eslint-plugin-react-refresh": "^0.4.26",
    "jsdom": "^24",
    "postcss": "^8.5.15",
    "prettier": "^3.8.3",
    "tailwindcss": "^3.4.19",
    "typescript": "^5.4",
    "vite": "^5",
    "vitest": "^2"
  }
}
```

- [ ] **Step 2: Create config files**

`web/tsconfig.json`:
```json
{
  "compilerOptions": {
    "target": "ES2020",
    "useDefineForClassFields": true,
    "lib": ["ES2020", "DOM", "DOM.Iterable"],
    "module": "ESNext",
    "skipLibCheck": true,
    "moduleResolution": "bundler",
    "resolveJsonModule": true,
    "isolatedModules": true,
    "noEmit": true,
    "jsx": "react-jsx",
    "strict": true,
    "noUnusedLocals": true,
    "noUnusedParameters": true,
    "noFallthroughCasesInSwitch": true,
    "types": ["vitest/globals", "@testing-library/jest-dom"]
  },
  "include": ["src"],
  "references": [{ "path": "./tsconfig.node.json" }]
}
```

`web/tsconfig.node.json`:
```json
{
  "compilerOptions": {
    "composite": true,
    "skipLibCheck": true,
    "module": "ESNext",
    "moduleResolution": "bundler",
    "allowSyntheticDefaultImports": true
  },
  "include": ["vite.config.ts"]
}
```

`web/vite.config.ts`:
```ts
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  test: {
    globals: true,
    environment: "jsdom",
    setupFiles: ["./src/vitest.setup.ts"],
    exclude: ["e2e/**", "node_modules/**"],
  },
});
```

`web/tailwind.config.ts`:
```ts
import type { Config } from "tailwindcss";

export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: { extend: {} },
  plugins: [],
} satisfies Config;
```

`web/postcss.config.js`:
```js
export default { plugins: { tailwindcss: {}, autoprefixer: {} } };
```

`web/.eslintrc.cjs`:
```js
module.exports = {
  root: true,
  env: { browser: true, es2020: true },
  extends: [
    "eslint:recommended",
    "plugin:@typescript-eslint/recommended",
    "plugin:react-hooks/recommended",
    "prettier",
  ],
  parser: "@typescript-eslint/parser",
  ignorePatterns: ["dist", "node_modules", ".eslintrc.cjs"],
  plugins: ["react-refresh"],
  rules: {
    "react-refresh/only-export-components": ["warn", { allowConstantExport: true }],
  },
};
```

`web/.prettierrc`:
```json
{ "semi": true, "singleQuote": false, "trailingComma": "all" }
```

- [ ] **Step 3: Create entry files**

`web/index.html`:
```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>Cairn</title>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
```

`web/src/index.css`:
```css
@tailwind base;
@tailwind components;
@tailwind utilities;

html, body, #root { height: 100%; margin: 0; }
```

`web/src/vitest.setup.ts`:
```ts
import "@testing-library/jest-dom/vitest";
```

`web/src/app/App.tsx` (placeholder, fleshed out in Task 8):
```tsx
export default function App() {
  return <div>Cairn</div>;
}
```

`web/src/main.tsx`:
```tsx
import React from "react";
import ReactDOM from "react-dom/client";
import { BrowserRouter } from "react-router-dom";
import App from "./app/App";
import "./index.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <BrowserRouter>
      <App />
    </BrowserRouter>
  </React.StrictMode>,
);
```

- [ ] **Step 4: Install deps and verify build + typecheck**

Run (from `web/`): `pnpm install && pnpm typecheck && pnpm build`
Expected: install succeeds; typecheck passes; `vite build` produces `dist/` with no errors.

- [ ] **Step 5: Initialize Playwright**

Run: `pnpm exec playwright install --with-deps chromium`
Create `web/playwright.config.ts`:
```ts
import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./e2e",
  use: { baseURL: "http://localhost:5173" },
  webServer: {
    command: "pnpm dev --port 5173",
    url: "http://localhost:5173",
    reuseExistingServer: !process.env.CI,
  },
});
```

- [ ] **Step 6: Commit**

```bash
git add web/ && git commit -m "chore: scaffold Vite + React + TS + Tailwind web app"
```

---

## Task 2: Vendor the engine contract types

> **Revised (re-pin):** the engine advanced to `079f9f9` and now ships the full
> contract — `CommandResponse`, `QueryResponse`, `ContractError`, `NoteSummary`,
> `GraphEdge`, plus `list_notes`/`get_graph` queries. Vendor ALL binding files,
> pinned at `079f9f9`. A prior commit on this branch vendored the stale
> `166b0eae` 3-file contract; this task overwrites it.

**Files:**
- Create/overwrite: `scripts/sync-contract.sh`, all of `web/src/contract/*.ts` (`Command.ts`, `Query.ts`, `Event.ts`, `CommandResponse.ts`, `QueryResponse.ts`, `ContractError.ts`, `NoteSummary.ts`, `GraphEdge.ts`, `source.ts`), `web/src/contract/index.ts`

- [ ] **Step 1: Create the sync script (copies ALL bindings)**

`scripts/sync-contract.sh`:
```bash
#!/usr/bin/env bash
# Vendor the generated TS contract from the cairn engine repo.
# Usage: scripts/sync-contract.sh [path-to-cairn-repo]  (default: ../cairn)
set -euo pipefail
SRC="${1:-../cairn}"
BINDINGS="$SRC/crates/cairn-contract/bindings"
DEST="web/src/contract"

[ -d "$BINDINGS" ] || { echo "bindings not found at $BINDINGS"; exit 1; }
mkdir -p "$DEST"
# Copy every generated binding (the set grows as the contract evolves).
cp "$BINDINGS"/*.ts "$DEST/"

COMMIT="$(git -C "$SRC" rev-parse HEAD)"
cat > "$DEST/source.ts" <<EOF
// Generated by scripts/sync-contract.sh — do not edit by hand.
export const CONTRACT_SOURCE_COMMIT = "$COMMIT";
EOF
echo "synced contract from $SRC @ $COMMIT"
```

Run: `chmod +x scripts/sync-contract.sh`

- [ ] **Step 2: Run the sync script**

Run (from repo root `/Users/titouanlebocq/code/cairn-ui`): `scripts/sync-contract.sh ../cairn`
Expected: prints "synced contract from ../cairn @ 079f9f9…". `web/src/contract/` now holds: `Command.ts`, `Query.ts`, `Event.ts`, `CommandResponse.ts`, `QueryResponse.ts`, `ContractError.ts`, `NoteSummary.ts`, `GraphEdge.ts`, `source.ts`.

Verify the vendored types: `Query` includes `list_notes` and `get_graph`; `QueryResponse` has `note`/`paths`/`notes`/`graph`; `CommandResponse` has `done`/`committed`; `ContractError` has `not_found`/`invalid_request`/`internal`. If `../cairn` is unavailable, STOP and report BLOCKED (do not hand-fabricate).

- [ ] **Step 3: Create the barrel re-export**

`web/src/contract/index.ts`:
```ts
export type { Command } from "./Command";
export type { Query } from "./Query";
export type { Event } from "./Event";
export type { CommandResponse } from "./CommandResponse";
export type { QueryResponse } from "./QueryResponse";
export type { ContractError } from "./ContractError";
export type { NoteSummary } from "./NoteSummary";
export type { GraphEdge } from "./GraphEdge";
export { CONTRACT_SOURCE_COMMIT } from "./source";
```

- [ ] **Step 4: Verify typecheck**

Run (from `web/`): `pnpm typecheck`
Expected: PASS (the vendored files are valid TS).

- [ ] **Step 5: Commit**

```bash
git add scripts/ web/src/contract/ && git commit -m "feat: vendor full engine contract (079f9f9) incl. response DTOs"
```

---

## Task 3: Define the CairnClient interface

> **Revised:** uses the real vendored DTOs (`CommandResponse`, `QueryResponse`,
> `ContractError`) instead of invented result types. Methods reject with a
> `ContractError`.

**Files:**
- Create: `web/src/client/types.ts`

- [ ] **Step 1: Write the interface**

`web/src/client/types.ts`:
```ts
import type { Command, Query, Event, CommandResponse, QueryResponse } from "../contract";

export type Unsubscribe = () => void;

/**
 * The single transport-abstracted contract the whole UI is written against.
 * `sendCommand`/`runQuery` reject with a `ContractError` (from "../contract")
 * on failure — the same typed error the daemon and cairn-service produce.
 */
export interface CairnClient {
  sendCommand(c: Command): Promise<CommandResponse>;
  runQuery(q: Query): Promise<QueryResponse>;
  subscribe(cb: (e: Event) => void): Unsubscribe;
}
```

- [ ] **Step 2: Verify typecheck**

Run (from `web/`): `pnpm typecheck`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add web/src/client/types.ts && git commit -m "feat: define CairnClient over the real contract DTOs"
```

---

## Task 4: Wikilink parser (matches engine extraction + stem rules)

**Files:**
- Create: `web/src/client/wikilink.ts`, `web/src/client/wikilink.test.ts`

- [ ] **Step 1: Write the failing test**

`web/src/client/wikilink.test.ts`:
```ts
import { describe, it, expect } from "vitest";
import { extractLinks, stem } from "./wikilink";

describe("extractLinks", () => {
  it("extracts plain and aliased links in order with duplicates", () => {
    expect(extractLinks("see [[Alpha]] and [[Beta|the second]] then [[Alpha]]")).toEqual([
      "Alpha",
      "Beta",
      "Alpha",
    ]);
  });

  it("ignores unclosed and whitespace-only links", () => {
    expect(extractLinks("[[ ]] and [[unclosed")).toEqual([]);
  });
});

describe("stem", () => {
  it("strips directory and .md extension", () => {
    expect(stem("dir/sub/note.md")).toBe("note");
    expect(stem("b.md")).toBe("b");
    expect(stem("noext")).toBe("noext");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test -- wikilink`
Expected: FAIL — cannot find module `./wikilink`.

- [ ] **Step 3: Write minimal implementation**

`web/src/client/wikilink.ts`:
```ts
/** Extract `[[target]]` / `[[target|alias]]` targets from body text,
 *  in order, with duplicates. Mirrors cairn-domain `extract_links`. */
export function extractLinks(body: string): string[] {
  const out: string[] = [];
  let i = 0;
  while (i + 1 < body.length) {
    if (body[i] === "[" && body[i + 1] === "[") {
      const close = body.indexOf("]]", i + 2);
      if (close !== -1) {
        const inner = body.slice(i + 2, close);
        const target = inner.split("|")[0].trim();
        if (target.length > 0) out.push(target);
        i = close + 2;
        continue;
      }
    }
    i += 1;
  }
  return out;
}

/** File stem: filename without directory or `.md`. Mirrors cairn-domain `stem`. */
export function stem(path: string): string {
  const afterSlash = path.split("/").pop() ?? path;
  return afterSlash.endsWith(".md") ? afterSlash.slice(0, -3) : afterSlash;
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm test -- wikilink`
Expected: PASS (5 assertions).

- [ ] **Step 5: Commit**

```bash
git add web/src/client/wikilink.ts web/src/client/wikilink.test.ts && git commit -m "feat: wikilink extraction + stem matching engine rules"
```

---

## Task 5: MockClient + fixture cairn

**Files:**
- Create: `web/src/client/fixtures.ts`, `web/src/client/mock.ts`, `web/src/client/mock.test.ts`

- [ ] **Step 1: Create the fixture cairn**

`web/src/client/fixtures.ts`:
```ts
/** A small interlinked fixture cairn used by the mock and dev. */
export const FIXTURE_NOTES: Record<string, string> = {
  "index.md": "# Index\n\nStart at [[ideas]] or the [[todo]] list.",
  "ideas.md": "# Ideas\n\nA thought that links back to [[index]].",
  "todo.md": "# Todo\n\n- review [[ideas]]\n- nothing links here yet",
};
```

- [ ] **Step 2: Write the failing test**

`web/src/client/mock.test.ts`:
```ts
import { describe, it, expect, vi } from "vitest";
import { MockClient } from "./mock";
import type { Event } from "../contract";

function freshNotes() {
  return { "a.md": "links to [[b]]", "b.md": "target note" };
}

describe("MockClient", () => {
  it("get_note returns the note variant", async () => {
    const c = new MockClient(freshNotes());
    expect(await c.runQuery({ type: "get_note", path: "a.md" })).toEqual({
      type: "note",
      contents: "links to [[b]]",
    });
  });

  it("get_note rejects with not_found for a missing note", async () => {
    const c = new MockClient(freshNotes());
    await expect(c.runQuery({ type: "get_note", path: "missing.md" })).rejects.toEqual({
      type: "not_found",
      what: "missing.md",
    });
  });

  it("search matches body and path, case-insensitive, sorted by path", async () => {
    const c = new MockClient({ "zeta.md": "alpha note", "alpha.md": "zeta body" });
    expect(await c.runQuery({ type: "search", query: "ALPHA" })).toEqual({
      type: "paths",
      paths: ["alpha.md", "zeta.md"],
    });
  });

  it("get_backlinks resolves by stem, sorted and deduped", async () => {
    const c = new MockClient(freshNotes());
    expect(await c.runQuery({ type: "get_backlinks", path: "b.md" })).toEqual({
      type: "paths",
      paths: ["a.md"],
    });
  });

  it("list_notes returns a NoteSummary per note with display titles, sorted by path", async () => {
    const c = new MockClient({
      "a.md": "---\ntitle: Alpha\n---\nbody",
      "b.md": "# Heading B\ntext",
      "c.md": "no title here",
    });
    expect(await c.runQuery({ type: "list_notes" })).toEqual({
      type: "notes",
      notes: [
        { path: "a.md", title: "Alpha" },
        { path: "b.md", title: "Heading B" },
        { path: "c.md", title: "c" },
      ],
    });
  });

  it("get_graph returns sorted nodes and resolved directed edges", async () => {
    const c = new MockClient(freshNotes());
    expect(await c.runQuery({ type: "get_graph" })).toEqual({
      type: "graph",
      nodes: ["a.md", "b.md"],
      edges: [{ from: "a.md", to: "b.md" }],
    });
  });

  it("write_note upserts and emits note_changed then reindexed; returns done", async () => {
    const c = new MockClient(freshNotes());
    const events: Event[] = [];
    c.subscribe((e) => events.push(e));
    const res = await c.sendCommand({ type: "write_note", path: "c.md", contents: "new [[a]]" });
    expect(res).toEqual({ type: "done" });
    await vi.waitFor(() =>
      expect(events).toEqual([
        { type: "note_changed", path: "c.md" },
        { type: "reindexed", count: 3 },
      ]),
    );
  });

  it("delete_note removes and emits note_deleted then reindexed; returns done", async () => {
    const c = new MockClient(freshNotes());
    const events: Event[] = [];
    c.subscribe((e) => events.push(e));
    const res = await c.sendCommand({ type: "delete_note", path: "b.md" });
    expect(res).toEqual({ type: "done" });
    await vi.waitFor(() =>
      expect(events).toEqual([
        { type: "note_deleted", path: "b.md" },
        { type: "reindexed", count: 1 },
      ]),
    );
    expect(await c.runQuery({ type: "search", query: "target" })).toEqual({
      type: "paths",
      paths: [],
    });
  });

  it("commit returns committed with a short id and emits committed", async () => {
    const c = new MockClient(freshNotes());
    const events: Event[] = [];
    c.subscribe((e) => events.push(e));
    const res = await c.sendCommand({ type: "commit", message: "first" });
    expect(res).toEqual({ type: "committed", commit: "c0001" });
    await vi.waitFor(() => expect(events).toContainEqual({ type: "committed", commit: "c0001" }));
  });
});
```

- [ ] **Step 3: Run test to verify it fails**

Run: `pnpm test -- mock`
Expected: FAIL — cannot find module `./mock`.

- [ ] **Step 4: Write the implementation**

`web/src/client/mock.ts`:
```ts
import type {
  Command,
  Query,
  Event,
  CommandResponse,
  QueryResponse,
  ContractError,
  NoteSummary,
  GraphEdge,
} from "../contract";
import type { CairnClient, Unsubscribe } from "./types";
import { extractLinks, stem } from "./wikilink";

/** Split a leading `---\n...\n---\n` frontmatter block. Mirrors cairn-domain
 *  Note::parse (frontmatter is the YAML between fences; body is the rest). */
function splitFrontmatter(raw: string): { frontmatter: string | null; body: string } {
  if (!raw.startsWith("---\n")) return { frontmatter: null, body: raw };
  const rest = raw.slice(4);
  if (rest.startsWith("---\n")) return { frontmatter: "", body: rest.slice(4) };
  const end = rest.indexOf("\n---\n");
  if (end === -1) return { frontmatter: null, body: raw };
  return { frontmatter: rest.slice(0, end), body: rest.slice(end + 5) };
}

/** display_title: frontmatter `title:`, else first `# ` heading, else stem.
 *  Mirrors cairn-domain Note::display_title. */
function displayTitle(path: string, raw: string): string {
  const { frontmatter, body } = splitFrontmatter(raw);
  if (frontmatter !== null) {
    for (const line of frontmatter.split("\n")) {
      const t = line.trimStart();
      if (t.startsWith("title:")) {
        // Two-pass quote strip mirrors Rust's trim_matches('"').trim_matches('\'').
        const v = t.slice("title:".length).trim().replace(/^"+|"+$/g, "").replace(/^'+|'+$/g, "").trim();
        if (v) return v;
      }
    }
  }
  for (const line of body.split("\n")) {
    const t = line.trimStart();
    if (t.startsWith("# ")) {
      const v = t.slice(2).trim();
      if (v) return v;
    }
  }
  return stem(path);
}

/** In-memory faithful mock of the cairn engine + cairn-service dispatch. */
export class MockClient implements CairnClient {
  private notes: Map<string, string>;
  private subscribers = new Set<(e: Event) => void>();
  private commitSeq = 0;

  constructor(seed: Record<string, string> = {}) {
    this.notes = new Map(Object.entries(seed));
  }

  subscribe(cb: (e: Event) => void): Unsubscribe {
    this.subscribers.add(cb);
    return () => this.subscribers.delete(cb);
  }

  private emit(e: Event): void {
    // Asynchronous so subscribers see push-after-the-fact timing.
    queueMicrotask(() => this.subscribers.forEach((cb) => cb(e)));
  }

  private stemIndex(): Map<string, string> {
    const byStem = new Map<string, string>();
    for (const path of this.notes.keys()) byStem.set(stem(path), path);
    return byStem;
  }

  async sendCommand(c: Command): Promise<CommandResponse> {
    switch (c.type) {
      case "write_note":
        this.notes.set(c.path, c.contents);
        this.emit({ type: "note_changed", path: c.path });
        this.emit({ type: "reindexed", count: this.notes.size });
        return { type: "done" };
      case "delete_note": {
        // The real store errors when deleting a missing note (fs NotFound ->
        // PortError::NotFound -> ContractError::NotFound).
        if (!this.notes.has(c.path)) {
          const err: ContractError = { type: "not_found", what: c.path };
          throw err;
        }
        this.notes.delete(c.path);
        this.emit({ type: "note_deleted", path: c.path });
        this.emit({ type: "reindexed", count: this.notes.size });
        return { type: "done" };
      }
      case "commit": {
        this.commitSeq += 1;
        const commit = `c${String(this.commitSeq).padStart(4, "0")}`;
        this.emit({ type: "committed", commit });
        return { type: "committed", commit };
      }
    }
  }

  async runQuery(q: Query): Promise<QueryResponse> {
    switch (q.type) {
      case "get_note": {
        const contents = this.notes.get(q.path);
        if (contents === undefined) {
          const err: ContractError = { type: "not_found", what: q.path };
          throw err;
        }
        return { type: "note", contents };
      }
      case "search": {
        const needle = q.query.toLowerCase();
        const paths = [...this.notes.entries()]
          .filter(
            ([path, raw]) =>
              splitFrontmatter(raw).body.toLowerCase().includes(needle) ||
              path.toLowerCase().includes(needle),
          )
          .map(([path]) => path)
          .sort();
        return { type: "paths", paths };
      }
      case "get_backlinks": {
        const byStem = this.stemIndex();
        const paths = [
          ...new Set(
            [...this.notes.entries()]
              .filter(([, raw]) =>
                extractLinks(splitFrontmatter(raw).body).some((t) => byStem.get(t) === q.path),
              )
              .map(([path]) => path),
          ),
        ].sort();
        return { type: "paths", paths };
      }
      case "list_notes": {
        const notes: NoteSummary[] = [...this.notes.entries()]
          .map(([path, raw]) => ({ path, title: displayTitle(path, raw) }))
          .sort((a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0));
        return { type: "notes", notes };
      }
      case "get_graph": {
        const byStem = this.stemIndex();
        const nodes = [...this.notes.keys()].sort();
        const seen = new Set<string>();
        const edges: GraphEdge[] = [];
        for (const [from, raw] of this.notes.entries()) {
          for (const target of extractLinks(splitFrontmatter(raw).body)) {
            const to = byStem.get(target);
            if (to && !seen.has(`${from} ${to}`)) {
              seen.add(`${from} ${to}`);
              edges.push({ from, to });
            }
          }
        }
        edges.sort((a, b) =>
          a.from === b.from ? (a.to < b.to ? -1 : a.to > b.to ? 1 : 0) : a.from < b.from ? -1 : 1,
        );
        return { type: "graph", nodes, edges };
      }
    }
  }

  /** Test/dev helper: current note paths. */
  paths(): string[] {
    return [...this.notes.keys()].sort();
  }
}
```

Note: `get_note` on a missing note **rejects** with a `ContractError`
(`not_found`), exactly as `dispatch_query` does in `cairn-service`. `get_graph`
is implemented for fidelity (the skeleton UI doesn't render it yet but the
Phase-4 graph view will use it).

- [ ] **Step 5: Run test to verify it passes**

Run: `pnpm test -- mock`
Expected: PASS (all cases).

- [ ] **Step 6: Commit**

```bash
git add web/src/client/fixtures.ts web/src/client/mock.ts web/src/client/mock.test.ts && git commit -m "feat: faithful in-memory MockClient + fixture cairn"
```

---

## Task 6: Debounce utility

**Files:**
- Create: `web/src/util/timer.ts`, `web/src/util/timer.test.ts`

- [ ] **Step 1: Write the failing test**

`web/src/util/timer.test.ts`:
```ts
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { debounce } from "./timer";

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

describe("debounce", () => {
  it("invokes once after the delay, coalescing rapid calls", () => {
    const fn = vi.fn();
    const d = debounce(fn, 1000);
    d();
    d();
    d();
    expect(fn).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1000);
    expect(fn).toHaveBeenCalledTimes(1);
  });

  it("cancel() prevents a pending invocation", () => {
    const fn = vi.fn();
    const d = debounce(fn, 1000);
    d();
    d.cancel();
    vi.advanceTimersByTime(1000);
    expect(fn).not.toHaveBeenCalled();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test -- timer`
Expected: FAIL — cannot find module `./timer`.

- [ ] **Step 3: Write minimal implementation**

`web/src/util/timer.ts`:
```ts
export interface Debounced {
  (): void;
  cancel(): void;
}

/** Debounce a zero-arg function: invoke `fn` `ms` after the last call. */
export function debounce(fn: () => void, ms: number): Debounced {
  let handle: ReturnType<typeof setTimeout> | null = null;
  const d = (() => {
    if (handle) clearTimeout(handle);
    handle = setTimeout(() => {
      handle = null;
      fn();
    }, ms);
  }) as Debounced;
  d.cancel = () => {
    if (handle) clearTimeout(handle);
    handle = null;
  };
  return d;
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm test -- timer`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/util/timer.ts web/src/util/timer.test.ts && git commit -m "feat: debounce utility"
```

---

## Task 7: Zustand store (state + event reactions + save/commit orchestration)

**Files:**
- Create: `web/src/store/store.ts`, `web/src/store/store.test.ts`

- [ ] **Step 1: Write the failing test**

`web/src/store/store.test.ts`:
```ts
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { createCairnStore, DEFAULT_SETTINGS } from "./store";
import { MockClient } from "../client/mock";

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

function setup() {
  const client = new MockClient({ "a.md": "links to [[b]]", "b.md": "target note" });
  const store = createCairnStore(client);
  return { client, store };
}

describe("cairn store", () => {
  it("init loads the note list", async () => {
    const { store } = setup();
    await store.getState().init();
    expect(store.getState().notePaths).toEqual(["a.md", "b.md"]);
  });

  it("openNote loads contents and backlinks", async () => {
    const { store } = setup();
    await store.getState().init();
    await store.getState().openNote("b.md");
    expect(store.getState().activePath).toBe("b.md");
    expect(store.getState().activeContents).toBe("target note");
    expect(store.getState().backlinks).toEqual(["a.md"]);
  });

  it("editBuffer schedules a debounced autosave that writes the note", async () => {
    const { client, store } = setup();
    await store.getState().init();
    await store.getState().openNote("a.md");
    store.getState().editBuffer("edited body [[b]]");
    expect(store.getState().dirty).toBe(true);
    await vi.advanceTimersByTimeAsync(DEFAULT_SETTINGS.autosaveMs);
    const res = await client.runQuery({ type: "get_note", path: "a.md" });
    expect(res).toEqual({ type: "note", contents: "edited body [[b]]" });
    expect(store.getState().dirty).toBe(false);
  });

  it("runSearch populates results; closeSearch clears them", async () => {
    const { store } = setup();
    await store.getState().init();
    await store.getState().runSearch("target");
    expect(store.getState().searchResults).toEqual(["b.md"]);
    store.getState().closeSearch();
    expect(store.getState().searchResults).toBeNull();
  });

  it("commitManual commits and records the id", async () => {
    const { store } = setup();
    await store.getState().init();
    await store.getState().commitManual("snapshot");
    expect(store.getState().lastCommit).toBe("c0001");
  });

  it("reacts to a note_changed event by refreshing the note list", async () => {
    // Real timers here: vi.waitFor polls on real timers and the mock emits via
    // queueMicrotask, so mixing fake timers would hang.
    vi.useRealTimers();
    const { client, store } = setup();
    await store.getState().init();
    await client.sendCommand({ type: "write_note", path: "c.md", contents: "hi" });
    await vi.waitFor(() => expect(store.getState().notePaths).toContain("c.md"));
  });

  it("surfaces errors from a failing command", async () => {
    const { client, store } = setup();
    vi.spyOn(client, "sendCommand").mockRejectedValueOnce(new Error("boom"));
    await store.getState().init();
    await store.getState().commitManual("x");
    expect(store.getState().error).toBe("boom");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test -- store`
Expected: FAIL — cannot find module `./store`.

- [ ] **Step 3: Write the implementation**

`web/src/store/store.ts`:
```ts
import { createStore, type StoreApi } from "zustand/vanilla";
import type { CairnClient } from "../client/types";
import type { ContractError } from "../contract";
import { debounce, type Debounced } from "../util/timer";

export interface Settings {
  autosaveMs: number;
  idleAutoCommit: boolean;
  idleAutoCommitMs: number;
  intervalAutoCommit: boolean;
  intervalAutoCommitMin: number;
  editorMode: "rich" | "raw";
}

export const DEFAULT_SETTINGS: Settings = {
  autosaveMs: 1000,
  idleAutoCommit: true,
  idleAutoCommitMs: 5000,
  intervalAutoCommit: true,
  intervalAutoCommitMin: 5,
  editorMode: "rich",
};

export interface CairnState {
  notePaths: string[];
  activePath: string | null;
  activeContents: string;
  dirty: boolean;
  saving: boolean;
  uncommitted: boolean;
  lastCommit: string | null;
  committing: boolean;
  query: string;
  searchResults: string[] | null;
  backlinks: string[];
  settings: Settings;
  error: string | null;

  init(): Promise<void>;
  refreshNotePaths(): Promise<void>;
  openNote(path: string): Promise<void>;
  editBuffer(contents: string): void;
  saveActive(): Promise<void>;
  createNote(path: string): Promise<void>;
  deleteNote(path: string): Promise<void>;
  runSearch(query: string): Promise<void>;
  setQuery(query: string): void;
  closeSearch(): void;
  refreshBacklinks(): Promise<void>;
  commitManual(message: string): Promise<void>;
  autoCommit(): Promise<void>;
  setSettings(patch: Partial<Settings>): void;
  rearmInterval(): void;
  dismissError(): void;
}

export function createCairnStore(client: CairnClient): StoreApi<CairnState> {
  let started = false;
  let autosave: Debounced | null = null;
  let idleCommit: Debounced | null = null;
  let intervalHandle: ReturnType<typeof setInterval> | null = null;

  const store = createStore<CairnState>()((set, get) => ({
    notePaths: [],
    activePath: null,
    activeContents: "",
    dirty: false,
    saving: false,
    uncommitted: false,
    lastCommit: null,
    committing: false,
    query: "",
    searchResults: null,
    backlinks: [],
    settings: DEFAULT_SETTINGS,
    error: null,

    async init() {
      if (started) return; // idempotent: StrictMode double-mounts effects
      started = true;
      client.subscribe((e) => {
        if (e.type === "note_changed" || e.type === "note_deleted") {
          void get().refreshNotePaths();
          if (get().searchResults !== null) void get().runSearch(get().query);
          if (get().activePath) void get().refreshBacklinks();
        } else if (e.type === "committed") {
          set({ lastCommit: e.commit, uncommitted: false });
        }
      });
      await get().refreshNotePaths();
      get().rearmInterval();
    },

    async refreshNotePaths() {
      try {
        const res = await client.runQuery({ type: "list_notes" });
        if (res.type === "notes") set({ notePaths: res.notes.map((n) => n.path) });
      } catch (err) {
        set({ error: errMsg(err) });
      }
    },

    async openNote(path) {
      try {
        const res = await client.runQuery({ type: "get_note", path });
        if (res.type === "note") {
          set({ activePath: path, activeContents: res.contents, dirty: false });
          await get().refreshBacklinks();
        }
      } catch (err) {
        set({ error: errMsg(err) });
      }
    },

    editBuffer(contents) {
      set({ activeContents: contents, dirty: true });
      autosave?.cancel();
      autosave = debounce(() => void get().saveActive(), get().settings.autosaveMs);
      autosave();
      const s = get().settings;
      if (s.idleAutoCommit) {
        idleCommit?.cancel();
        idleCommit = debounce(() => void get().autoCommit(), s.idleAutoCommitMs);
        idleCommit();
      }
    },

    async saveActive() {
      const path = get().activePath;
      if (!path || !get().dirty) return;
      const snapshot = get().activeContents;
      set({ saving: true });
      try {
        await client.sendCommand({ type: "write_note", path, contents: snapshot });
        set((s) => ({
          saving: false,
          uncommitted: true,
          // Stay dirty if the note changed during the write (the pending debounce
          // will save it); don't touch dirty if the user navigated to another note.
          dirty: s.activePath === path ? s.activeContents !== snapshot : s.dirty,
        }));
      } catch (err) {
        set({ saving: false, error: errMsg(err) });
      }
    },

    async createNote(path) {
      try {
        await client.sendCommand({ type: "write_note", path, contents: "" });
        await get().openNote(path);
      } catch (err) {
        set({ error: errMsg(err) });
      }
    },

    async deleteNote(path) {
      try {
        await client.sendCommand({ type: "delete_note", path });
        if (get().activePath === path)
          set({ activePath: null, activeContents: "", backlinks: [], dirty: false, saving: false });
      } catch (err) {
        set({ error: errMsg(err) });
      }
    },

    async runSearch(query) {
      try {
        const res = await client.runQuery({ type: "search", query });
        if (res.type === "paths") set({ query, searchResults: res.paths });
      } catch (err) {
        set({ error: errMsg(err) });
      }
    },

    setQuery(query) {
      set({ query });
    },

    closeSearch() {
      set({ searchResults: null });
    },

    async refreshBacklinks() {
      const path = get().activePath;
      if (!path) return set({ backlinks: [] });
      try {
        const res = await client.runQuery({ type: "get_backlinks", path });
        if (res.type === "paths") set({ backlinks: res.paths });
      } catch (err) {
        set({ error: errMsg(err) });
      }
    },

    async commitManual(message) {
      if (get().committing) return;
      set({ committing: true });
      try {
        const res = await client.sendCommand({ type: "commit", message });
        if (res.type === "committed") set({ lastCommit: res.commit, uncommitted: false });
      } catch (err) {
        set({ error: errMsg(err) });
      } finally {
        set({ committing: false });
      }
    },

    async autoCommit() {
      if (!get().uncommitted || get().committing) return;
      const path = get().activePath;
      const message = path ? `cairn: update ${path}` : "cairn: auto-commit";
      await get().commitManual(message);
    },

    setSettings(patch) {
      set({ settings: { ...get().settings, ...patch } });
      if ("intervalAutoCommit" in patch || "intervalAutoCommitMin" in patch) {
        get().rearmInterval();
      }
    },

    rearmInterval() {
      if (intervalHandle) clearInterval(intervalHandle);
      intervalHandle = null;
      const { intervalAutoCommit, intervalAutoCommitMin } = get().settings;
      if (intervalAutoCommit) {
        intervalHandle = setInterval(() => void get().autoCommit(), intervalAutoCommitMin * 60_000);
      }
    },

    dismissError() {
      set({ error: null });
    },
  }));

  return store;
}

function errMsg(err: unknown): string {
  // ContractError (rejected by the client) is a tagged object.
  if (err && typeof err === "object" && "type" in err) {
    const e = err as ContractError;
    if (e.type === "not_found") return `Not found: ${e.what}`;
    return e.message;
  }
  return err instanceof Error ? err.message : String(err);
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm test -- store`
Expected: PASS (all cases).

- [ ] **Step 5: Commit**

```bash
git add web/src/store/ && git commit -m "feat: zustand store with event reactions and save/commit orchestration"
```

---

## Task 8: App store instance, client composition root, and Shell layout

**Files:**
- Create: `web/src/app/makeClient.ts`, `web/src/app/cairnStore.ts`, `web/src/components/Shell.tsx`
- Modify: `web/src/app/App.tsx`

- [ ] **Step 1: Create the composition root**

`web/src/app/makeClient.ts`:
```ts
import type { CairnClient } from "../client/types";
import { MockClient } from "../client/mock";
import { FIXTURE_NOTES } from "../client/fixtures";

/** The single place the transport is chosen. Phase 2 swaps this for TauriClient. */
export function makeClient(): CairnClient {
  return new MockClient(FIXTURE_NOTES);
}
```

`web/src/app/cairnStore.ts`:
```ts
import { useStore } from "zustand";
import { createCairnStore, type CairnState } from "../store/store";
import { makeClient } from "./makeClient";

export const cairnStore = createCairnStore(makeClient());

export function useCairn<T>(selector: (s: CairnState) => T): T {
  return useStore(cairnStore, selector);
}
```

- [ ] **Step 2: Write the failing test for Shell**

`web/src/components/Shell.test.tsx`:
```tsx
import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { Shell } from "./Shell";

describe("Shell", () => {
  it("renders the three regions and top bar", () => {
    render(
      <Shell
        topBar={<div>top</div>}
        list={<div>list</div>}
        editor={<div>editor</div>}
        backlinks={<div>backlinks</div>}
      />,
    );
    expect(screen.getByText("top")).toBeInTheDocument();
    expect(screen.getByText("list")).toBeInTheDocument();
    expect(screen.getByText("editor")).toBeInTheDocument();
    expect(screen.getByText("backlinks")).toBeInTheDocument();
  });
});
```

- [ ] **Step 3: Run test to verify it fails**

Run: `pnpm test -- Shell`
Expected: FAIL — cannot find module `./Shell`.

- [ ] **Step 4: Implement Shell**

`web/src/components/Shell.tsx`:
```tsx
import type { ReactNode } from "react";

export function Shell(props: {
  topBar: ReactNode;
  list: ReactNode;
  editor: ReactNode;
  backlinks: ReactNode;
}) {
  return (
    <div className="flex h-full flex-col bg-neutral-900 text-neutral-100">
      <header className="flex items-center gap-2 border-b border-neutral-800 px-3 py-2">
        {props.topBar}
      </header>
      <div className="flex min-h-0 flex-1">
        <aside className="w-56 shrink-0 overflow-auto border-r border-neutral-800 p-2">
          {props.list}
        </aside>
        <main className="min-w-0 flex-1 overflow-auto p-3">{props.editor}</main>
        <aside className="w-56 shrink-0 overflow-auto border-l border-neutral-800 p-2">
          {props.backlinks}
        </aside>
      </div>
    </div>
  );
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `pnpm test -- Shell`
Expected: PASS.

- [ ] **Step 6: Wire App (init the store, compose Shell with placeholders)**

`web/src/app/App.tsx`:
```tsx
import { useEffect } from "react";
import { Shell } from "../components/Shell";
import { cairnStore } from "./cairnStore";

export default function App() {
  useEffect(() => {
    void cairnStore.getState().init();
  }, []);

  return (
    <Shell
      topBar={<span className="text-sm text-neutral-400">Cairn</span>}
      list={<div>notes</div>}
      editor={<div>editor</div>}
      backlinks={<div>backlinks</div>}
    />
  );
}
```

- [ ] **Step 7: Verify it runs**

Run: `pnpm dev` then open http://localhost:5173 — the three-pane shell renders. Stop the server.

- [ ] **Step 8: Commit**

```bash
git add web/src/app/ web/src/components/Shell.tsx web/src/components/Shell.test.tsx && git commit -m "feat: composition root, app store, three-pane Shell"
```

---

## Task 9: NoteList (select, new, delete)

**Files:**
- Create: `web/src/components/NoteList.tsx`, `web/src/components/NoteList.test.tsx`
- Modify: `web/src/app/App.tsx`

- [ ] **Step 1: Write the failing test**

`web/src/components/NoteList.test.tsx`:
```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { NoteList } from "./NoteList";

describe("NoteList", () => {
  it("lists notes and fires onOpen when one is clicked", async () => {
    const onOpen = vi.fn();
    render(
      <NoteList
        paths={["a.md", "b.md"]}
        activePath="a.md"
        onOpen={onOpen}
        onNew={vi.fn()}
        onDelete={vi.fn()}
      />,
    );
    await userEvent.click(screen.getByText("b.md"));
    expect(onOpen).toHaveBeenCalledWith("b.md");
  });

  it("calls onNew with a path from the prompt", async () => {
    const onNew = vi.fn();
    vi.spyOn(window, "prompt").mockReturnValue("new.md");
    render(
      <NoteList paths={[]} activePath={null} onOpen={vi.fn()} onNew={onNew} onDelete={vi.fn()} />,
    );
    await userEvent.click(screen.getByRole("button", { name: /new note/i }));
    expect(onNew).toHaveBeenCalledWith("new.md");
  });

  it("calls onDelete for a note", async () => {
    const onDelete = vi.fn();
    render(
      <NoteList
        paths={["a.md"]}
        activePath={null}
        onOpen={vi.fn()}
        onNew={vi.fn()}
        onDelete={onDelete}
      />,
    );
    await userEvent.click(screen.getByRole("button", { name: /delete a\.md/i }));
    expect(onDelete).toHaveBeenCalledWith("a.md");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test -- NoteList`
Expected: FAIL — cannot find module `./NoteList`.

- [ ] **Step 3: Implement NoteList**

`web/src/components/NoteList.tsx`:
```tsx
export function NoteList(props: {
  paths: string[];
  activePath: string | null;
  onOpen: (path: string) => void;
  onNew: (path: string) => void;
  onDelete: (path: string) => void;
}) {
  return (
    <div className="flex flex-col gap-1 text-sm">
      <div className="mb-1 flex items-center justify-between">
        <span className="text-xs uppercase tracking-wide text-neutral-500">Notes</span>
        <button
          className="rounded px-1 text-neutral-300 hover:bg-neutral-800"
          onClick={() => {
            const path = window.prompt("New note path (e.g. notes/idea.md)");
            if (path) props.onNew(path);
          }}
        >
          + New note
        </button>
      </div>
      {props.paths.map((path) => (
        <div
          key={path}
          className={`group flex items-center justify-between rounded px-2 py-1 hover:bg-neutral-800 ${
            path === props.activePath ? "bg-neutral-800 text-white" : "text-neutral-300"
          }`}
        >
          <button className="min-w-0 flex-1 truncate text-left" onClick={() => props.onOpen(path)}>
            {path}
          </button>
          <button
            className="ml-1 hidden text-neutral-500 hover:text-red-400 group-hover:block"
            aria-label={`delete ${path}`}
            onClick={() => props.onDelete(path)}
          >
            ✕
          </button>
        </div>
      ))}
    </div>
  );
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm test -- NoteList`
Expected: PASS.

- [ ] **Step 5: Wire NoteList into App**

In `web/src/app/App.tsx`, replace the `list={<div>notes</div>}` prop. Add imports and a selector; the file becomes:
```tsx
import { useEffect } from "react";
import { Shell } from "../components/Shell";
import { NoteList } from "../components/NoteList";
import { cairnStore, useCairn } from "./cairnStore";

export default function App() {
  useEffect(() => {
    void cairnStore.getState().init();
  }, []);

  const notePaths = useCairn((s) => s.notePaths);
  const activePath = useCairn((s) => s.activePath);
  const actions = cairnStore.getState();

  return (
    <Shell
      topBar={<span className="text-sm text-neutral-400">Cairn</span>}
      list={
        <NoteList
          paths={notePaths}
          activePath={activePath}
          onOpen={actions.openNote}
          onNew={actions.createNote}
          onDelete={actions.deleteNote}
        />
      }
      editor={<div>editor</div>}
      backlinks={<div>backlinks</div>}
    />
  );
}
```

- [ ] **Step 6: Commit**

```bash
git add web/src/components/NoteList.tsx web/src/components/NoteList.test.tsx web/src/app/App.tsx && git commit -m "feat: NoteList with open/new/delete"
```

---

## Task 10: Editor (rich CodeMirror + raw toggle)

**Files:**
- Create: `web/src/components/Editor.tsx`, `web/src/components/Editor.test.tsx`
- Modify: `web/src/app/App.tsx`

- [ ] **Step 1: Write the failing test**

CodeMirror is awkward in jsdom, so the rich editor is exercised in e2e; the unit test covers raw mode and the toggle.

`web/src/components/Editor.test.tsx`:
```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Editor } from "./Editor";

describe("Editor", () => {
  it("shows a placeholder when no note is open", () => {
    render(<Editor path={null} value="" mode="raw" onChange={vi.fn()} onToggleMode={vi.fn()} />);
    expect(screen.getByText(/no note open/i)).toBeInTheDocument();
  });

  it("raw mode edits call onChange", async () => {
    const onChange = vi.fn();
    render(<Editor path="a.md" value="hi" mode="raw" onChange={onChange} onToggleMode={vi.fn()} />);
    const area = screen.getByRole("textbox");
    await userEvent.type(area, "!");
    expect(onChange).toHaveBeenCalled();
  });

  it("toggle button switches mode", async () => {
    const onToggleMode = vi.fn();
    render(
      <Editor path="a.md" value="hi" mode="raw" onChange={vi.fn()} onToggleMode={onToggleMode} />,
    );
    await userEvent.click(screen.getByRole("button", { name: /rich|raw/i }));
    expect(onToggleMode).toHaveBeenCalled();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test -- Editor`
Expected: FAIL — cannot find module `./Editor`.

- [ ] **Step 3: Implement Editor**

`web/src/components/Editor.tsx`:
```tsx
import CodeMirror from "@uiw/react-codemirror";
import { markdown } from "@codemirror/lang-markdown";

export function Editor(props: {
  path: string | null;
  value: string;
  mode: "rich" | "raw";
  onChange: (value: string) => void;
  onToggleMode: () => void;
}) {
  if (!props.path) {
    return <div className="text-sm text-neutral-500">No note open. Pick one from the list.</div>;
  }
  return (
    <div className="flex h-full flex-col">
      <div className="mb-2 flex items-center justify-between">
        <span className="text-sm text-neutral-300">{props.path}</span>
        <button
          className="rounded border border-neutral-700 px-2 py-0.5 text-xs text-neutral-300 hover:bg-neutral-800"
          onClick={props.onToggleMode}
        >
          {props.mode === "rich" ? "Switch to raw" : "Switch to rich"}
        </button>
      </div>
      {props.mode === "rich" ? (
        <CodeMirror
          value={props.value}
          height="100%"
          theme="dark"
          extensions={[markdown()]}
          onChange={props.onChange}
        />
      ) : (
        <textarea
          className="h-full w-full resize-none bg-neutral-950 p-2 font-mono text-sm text-neutral-100 outline-none"
          value={props.value}
          onChange={(e) => props.onChange(e.target.value)}
        />
      )}
    </div>
  );
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm test -- Editor`
Expected: PASS.

- [ ] **Step 5: Wire Editor into App**

In `web/src/app/App.tsx`: add selectors for `activeContents` and `settings.editorMode`, and replace the `editor` prop. Add these selectors after the existing ones and update the prop:
```tsx
  const activeContents = useCairn((s) => s.activeContents);
  const editorMode = useCairn((s) => s.settings.editorMode);
```
Replace `editor={<div>editor</div>}` with:
```tsx
      editor={
        <Editor
          path={activePath}
          value={activeContents}
          mode={editorMode}
          onChange={actions.editBuffer}
          onToggleMode={() =>
            actions.setSettings({ editorMode: editorMode === "rich" ? "raw" : "rich" })
          }
        />
      }
```
Add the import: `import { Editor } from "../components/Editor";`

- [ ] **Step 6: Commit**

```bash
git add web/src/components/Editor.tsx web/src/components/Editor.test.tsx web/src/app/App.tsx && git commit -m "feat: Editor with rich CodeMirror and raw textarea modes"
```

---

## Task 11: Backlinks panel

**Files:**
- Create: `web/src/components/Backlinks.tsx`, `web/src/components/Backlinks.test.tsx`
- Modify: `web/src/app/App.tsx`

- [ ] **Step 1: Write the failing test**

`web/src/components/Backlinks.test.tsx`:
```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Backlinks } from "./Backlinks";

describe("Backlinks", () => {
  it("shows an empty state when there are none", () => {
    render(<Backlinks paths={[]} onOpen={vi.fn()} />);
    expect(screen.getByText(/no backlinks/i)).toBeInTheDocument();
  });

  it("lists backlinks and opens one on click", async () => {
    const onOpen = vi.fn();
    render(<Backlinks paths={["a.md"]} onOpen={onOpen} />);
    await userEvent.click(screen.getByText("a.md"));
    expect(onOpen).toHaveBeenCalledWith("a.md");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test -- Backlinks`
Expected: FAIL — cannot find module `./Backlinks`.

- [ ] **Step 3: Implement Backlinks**

`web/src/components/Backlinks.tsx`:
```tsx
export function Backlinks(props: { paths: string[]; onOpen: (path: string) => void }) {
  return (
    <div className="flex flex-col gap-1 text-sm">
      <span className="mb-1 text-xs uppercase tracking-wide text-neutral-500">Backlinks</span>
      {props.paths.length === 0 ? (
        <span className="text-neutral-600">No backlinks</span>
      ) : (
        props.paths.map((path) => (
          <button
            key={path}
            className="truncate rounded px-2 py-1 text-left text-neutral-300 hover:bg-neutral-800"
            onClick={() => props.onOpen(path)}
          >
            {path}
          </button>
        ))
      )}
    </div>
  );
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm test -- Backlinks`
Expected: PASS.

- [ ] **Step 5: Wire Backlinks into App**

In `web/src/app/App.tsx`: add `const backlinks = useCairn((s) => s.backlinks);`, import `Backlinks`, and replace `backlinks={<div>backlinks</div>}` with:
```tsx
      backlinks={<Backlinks paths={backlinks} onOpen={actions.openNote} />}
```

- [ ] **Step 6: Commit**

```bash
git add web/src/components/Backlinks.tsx web/src/components/Backlinks.test.tsx web/src/app/App.tsx && git commit -m "feat: Backlinks panel"
```

---

## Task 12: SearchBar + SearchResults

**Files:**
- Create: `web/src/components/SearchBar.tsx`, `web/src/components/SearchBar.test.tsx`, `web/src/components/SearchResults.tsx`, `web/src/components/SearchResults.test.tsx`
- Modify: `web/src/app/App.tsx`

- [ ] **Step 1: Write the failing tests**

`web/src/components/SearchBar.test.tsx`:
```tsx
import { describe, it, expect, vi } from "vitest";
import { useState } from "react";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SearchBar } from "./SearchBar";

describe("SearchBar", () => {
  it("submits the live query on Enter", async () => {
    // Stateful harness so the controlled input actually updates as the user types.
    const onSearch = vi.fn();
    function Harness() {
      const [v, setV] = useState("");
      return <SearchBar value={v} onChange={setV} onSearch={onSearch} />;
    }
    render(<Harness />);
    await userEvent.type(screen.getByPlaceholderText(/search/i), "target{enter}");
    expect(onSearch).toHaveBeenCalledWith("target");
  });
});
```

`web/src/components/SearchResults.test.tsx`:
```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SearchResults } from "./SearchResults";

describe("SearchResults", () => {
  it("renders nothing when results are null", () => {
    const { container } = render(
      <SearchResults results={null} onOpen={vi.fn()} onClose={vi.fn()} />,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("opens a result and can be closed", async () => {
    const onOpen = vi.fn();
    const onClose = vi.fn();
    render(<SearchResults results={["b.md"]} onOpen={onOpen} onClose={onClose} />);
    await userEvent.click(screen.getByText("b.md"));
    expect(onOpen).toHaveBeenCalledWith("b.md");
    await userEvent.click(screen.getByRole("button", { name: /close/i }));
    expect(onClose).toHaveBeenCalled();
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `pnpm test -- Search`
Expected: FAIL — modules not found.

- [ ] **Step 3: Implement both components**

`web/src/components/SearchBar.tsx`:
```tsx
export function SearchBar(props: {
  value: string;
  onChange: (value: string) => void;
  onSearch: (query: string) => void;
}) {
  return (
    <input
      className="w-64 rounded bg-neutral-800 px-2 py-1 text-sm text-neutral-100 outline-none"
      placeholder="Search…"
      value={props.value}
      onChange={(e) => props.onChange(e.target.value)}
      onKeyDown={(e) => {
        if (e.key === "Enter") props.onSearch(props.value);
      }}
    />
  );
}
```

`web/src/components/SearchResults.tsx`:
```tsx
export function SearchResults(props: {
  results: string[] | null;
  onOpen: (path: string) => void;
  onClose: () => void;
}) {
  if (props.results === null) return null;
  return (
    <div
      data-testid="search-results"
      className="absolute left-2 top-12 z-10 w-72 rounded border border-neutral-700 bg-neutral-900 p-2 shadow-lg"
    >
      <div className="mb-1 flex items-center justify-between">
        <span className="text-xs uppercase tracking-wide text-neutral-500">
          Results ({props.results.length})
        </span>
        <button className="text-neutral-400 hover:text-white" aria-label="close" onClick={props.onClose}>
          ✕
        </button>
      </div>
      {props.results.length === 0 ? (
        <span className="text-sm text-neutral-600">No matches</span>
      ) : (
        props.results.map((path) => (
          <button
            key={path}
            className="block w-full truncate rounded px-2 py-1 text-left text-sm text-neutral-300 hover:bg-neutral-800"
            onClick={() => props.onOpen(path)}
          >
            {path}
          </button>
        ))
      )}
    </div>
  );
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `pnpm test -- Search`
Expected: PASS.

- [ ] **Step 5: Wire into App**

The search results overlay is positioned relative to the shell; render it inside the editor region's wrapper. In `web/src/app/App.tsx`, add selectors and handlers, import both components, and put `SearchBar` in the top bar and `SearchResults` next to the editor. Update `App` so the relevant parts read:
```tsx
  const query = useCairn((s) => s.query);
  const searchResults = useCairn((s) => s.searchResults);
```
Top bar prop:
```tsx
      topBar={
        <div className="flex items-center gap-3">
          <span className="text-sm text-neutral-400">Cairn</span>
          <SearchBar value={query} onChange={actions.setQuery} onSearch={actions.runSearch} />
        </div>
      }
```
Wrap the editor prop so the overlay can position over the list:
```tsx
      editor={
        <div className="relative h-full">
          <SearchResults
            results={searchResults}
            onOpen={(p) => {
              void actions.openNote(p);
              actions.closeSearch();
            }}
            onClose={actions.closeSearch}
          />
          <Editor
            path={activePath}
            value={activeContents}
            mode={editorMode}
            onChange={actions.editBuffer}
            onToggleMode={() =>
              actions.setSettings({ editorMode: editorMode === "rich" ? "raw" : "rich" })
            }
          />
        </div>
      }
```
Add imports for `SearchBar` and `SearchResults`.

- [ ] **Step 6: Commit**

```bash
git add web/src/components/Search*.tsx web/src/components/Search*.test.tsx web/src/app/App.tsx && git commit -m "feat: search bar and results overlay"
```

---

## Task 13: CommitBar (status + manual commit)

**Files:**
- Create: `web/src/components/CommitBar.tsx`, `web/src/components/CommitBar.test.tsx`
- Modify: `web/src/app/App.tsx`

- [ ] **Step 1: Write the failing test**

`web/src/components/CommitBar.test.tsx`:
```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { CommitBar } from "./CommitBar";

describe("CommitBar", () => {
  it("shows saving status", () => {
    render(
      <CommitBar saving dirty uncommitted={false} lastCommit={null} committing={false} onCommit={vi.fn()} />,
    );
    expect(screen.getByText(/saving/i)).toBeInTheDocument();
  });

  it("shows last commit id when present", () => {
    render(
      <CommitBar saving={false} dirty={false} uncommitted={false} lastCommit="c0007" committing={false} onCommit={vi.fn()} />,
    );
    expect(screen.getByText(/c0007/)).toBeInTheDocument();
  });

  it("commits with the entered message", async () => {
    const onCommit = vi.fn();
    vi.spyOn(window, "prompt").mockReturnValue("snapshot");
    render(
      <CommitBar saving={false} dirty={false} uncommitted onCommit={onCommit} lastCommit={null} committing={false} />,
    );
    await userEvent.click(screen.getByRole("button", { name: /commit/i }));
    expect(onCommit).toHaveBeenCalledWith("snapshot");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test -- CommitBar`
Expected: FAIL — cannot find module `./CommitBar`.

- [ ] **Step 3: Implement CommitBar**

`web/src/components/CommitBar.tsx`:
```tsx
export function CommitBar(props: {
  saving: boolean;
  dirty: boolean;
  uncommitted: boolean;
  lastCommit: string | null;
  committing: boolean;
  onCommit: (message: string) => void;
}) {
  const status = props.saving
    ? "Saving…"
    : props.dirty
      ? "Unsaved"
      : props.uncommitted
        ? "Saved · uncommitted"
        : "Saved";
  return (
    <div className="flex items-center gap-3 text-xs text-neutral-400">
      <span>{status}</span>
      {props.lastCommit && <span className="text-neutral-500">@{props.lastCommit}</span>}
      <button
        className="rounded border border-neutral-700 px-2 py-0.5 text-neutral-200 hover:bg-neutral-800 disabled:opacity-50"
        disabled={props.committing}
        onClick={() => {
          const message = window.prompt("Commit message");
          if (message) props.onCommit(message);
        }}
      >
        Commit
      </button>
    </div>
  );
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm test -- CommitBar`
Expected: PASS.

- [ ] **Step 5: Wire CommitBar into App's top bar**

In `web/src/app/App.tsx`: add selectors `saving`, `dirty`, `uncommitted`, `lastCommit`, `committing`; import `CommitBar`; place it on the right of the top bar by changing the top bar prop to a justified row:
```tsx
      topBar={
        <div className="flex w-full items-center justify-between">
          <div className="flex items-center gap-3">
            <span className="text-sm text-neutral-400">Cairn</span>
            <SearchBar value={query} onChange={actions.setQuery} onSearch={actions.runSearch} />
          </div>
          <CommitBar
            saving={saving}
            dirty={dirty}
            uncommitted={uncommitted}
            lastCommit={lastCommit}
            committing={committing}
            onCommit={actions.commitManual}
          />
        </div>
      }
```
With the added selectors:
```tsx
  const saving = useCairn((s) => s.saving);
  const dirty = useCairn((s) => s.dirty);
  const uncommitted = useCairn((s) => s.uncommitted);
  const lastCommit = useCairn((s) => s.lastCommit);
  const committing = useCairn((s) => s.committing);
```

- [ ] **Step 6: Commit**

```bash
git add web/src/components/CommitBar.tsx web/src/components/CommitBar.test.tsx web/src/app/App.tsx && git commit -m "feat: CommitBar with save status and manual commit"
```

---

## Task 14: Settings panel (auto-commit + editor mode)

**Files:**
- Create: `web/src/components/Settings.tsx`, `web/src/components/Settings.test.tsx`
- Modify: `web/src/app/App.tsx`

- [ ] **Step 1: Write the failing test**

`web/src/components/Settings.test.tsx`:
```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Settings } from "./Settings";
import { DEFAULT_SETTINGS } from "../store/store";

describe("Settings", () => {
  it("toggles idle auto-commit", async () => {
    const onChange = vi.fn();
    render(<Settings settings={DEFAULT_SETTINGS} onChange={onChange} />);
    await userEvent.click(screen.getByLabelText(/idle auto-commit/i));
    expect(onChange).toHaveBeenCalledWith({ idleAutoCommit: !DEFAULT_SETTINGS.idleAutoCommit });
  });

  it("edits the interval minutes", async () => {
    const onChange = vi.fn();
    render(<Settings settings={DEFAULT_SETTINGS} onChange={onChange} />);
    const input = screen.getByLabelText(/interval \(min\)/i);
    await userEvent.clear(input);
    await userEvent.type(input, "10");
    expect(onChange).toHaveBeenLastCalledWith({ intervalAutoCommitMin: 10 });
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test -- Settings`
Expected: FAIL — cannot find module `./Settings`.

- [ ] **Step 3: Implement Settings**

`web/src/components/Settings.tsx`:
```tsx
import { useState } from "react";
import type { Settings as SettingsType } from "../store/store";

export function Settings(props: {
  settings: SettingsType;
  onChange: (patch: Partial<SettingsType>) => void;
}) {
  const s = props.settings;
  // Local state for the number input so clear+type behaves; onChange still
  // pushes valid values to the store.
  const [intervalStr, setIntervalStr] = useState(String(s.intervalAutoCommitMin));
  return (
    <div className="flex flex-col gap-2 text-sm text-neutral-300">
      <span className="text-xs uppercase tracking-wide text-neutral-500">Auto-commit</span>
      <label className="flex items-center gap-2">
        <input
          type="checkbox"
          checked={s.idleAutoCommit}
          onChange={(e) => props.onChange({ idleAutoCommit: e.target.checked })}
        />
        Idle auto-commit
      </label>
      <label className="flex items-center gap-2">
        <input
          type="checkbox"
          checked={s.intervalAutoCommit}
          onChange={(e) => props.onChange({ intervalAutoCommit: e.target.checked })}
        />
        Interval auto-commit
      </label>
      <label className="flex items-center gap-2">
        Interval (min)
        <input
          type="number"
          min={1}
          className="w-16 rounded bg-neutral-800 px-1"
          value={intervalStr}
          onChange={(e) => {
            setIntervalStr(e.target.value);
            const n = Number(e.target.value);
            if (!isNaN(n) && n > 0) props.onChange({ intervalAutoCommitMin: n });
          }}
        />
      </label>
    </div>
  );
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm test -- Settings`
Expected: PASS.

- [ ] **Step 5: Wire Settings into App (right pane, below backlinks)**

In `web/src/app/App.tsx`: add `const settings = useCairn((s) => s.settings);`, import `Settings`, and change the `backlinks` prop to stack the panel and settings:
```tsx
      backlinks={
        <div className="flex flex-col gap-4">
          <Backlinks paths={backlinks} onOpen={actions.openNote} />
          <Settings settings={settings} onChange={actions.setSettings} />
        </div>
      }
```

- [ ] **Step 6: Commit**

```bash
git add web/src/components/Settings.tsx web/src/components/Settings.test.tsx web/src/app/App.tsx && git commit -m "feat: settings panel for auto-commit and editor mode"
```

---

## Task 15: ErrorToast

**Files:**
- Create: `web/src/components/ErrorToast.tsx`, `web/src/components/ErrorToast.test.tsx`
- Modify: `web/src/app/App.tsx`

- [ ] **Step 1: Write the failing test**

`web/src/components/ErrorToast.test.tsx`:
```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ErrorToast } from "./ErrorToast";

describe("ErrorToast", () => {
  it("renders nothing when message is null", () => {
    const { container } = render(<ErrorToast message={null} onDismiss={vi.fn()} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("shows the message and dismisses", async () => {
    const onDismiss = vi.fn();
    render(<ErrorToast message="boom" onDismiss={onDismiss} />);
    expect(screen.getByText("boom")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /dismiss/i }));
    expect(onDismiss).toHaveBeenCalled();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test -- ErrorToast`
Expected: FAIL — cannot find module `./ErrorToast`.

- [ ] **Step 3: Implement ErrorToast**

`web/src/components/ErrorToast.tsx`:
```tsx
export function ErrorToast(props: { message: string | null; onDismiss: () => void }) {
  if (props.message === null) return null;
  return (
    <div className="fixed bottom-4 right-4 z-20 flex items-center gap-3 rounded border border-red-700 bg-red-950 px-3 py-2 text-sm text-red-200 shadow-lg">
      <span>{props.message}</span>
      <button className="text-red-300 hover:text-white" aria-label="dismiss" onClick={props.onDismiss}>
        ✕
      </button>
    </div>
  );
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm test -- ErrorToast`
Expected: PASS.

- [ ] **Step 5: Wire ErrorToast into App**

In `web/src/app/App.tsx`: add `const error = useCairn((s) => s.error);`, import `ErrorToast`, and wrap the returned `Shell` in a fragment with the toast:
```tsx
  return (
    <>
      <Shell
        /* …existing props unchanged… */
      />
      <ErrorToast message={error} onDismiss={actions.dismissError} />
    </>
  );
```

- [ ] **Step 6: Run the full unit suite + typecheck + lint**

Run: `pnpm test && pnpm typecheck && pnpm lint`
Expected: all PASS.

- [ ] **Step 7: Commit**

```bash
git add web/src/components/ErrorToast.tsx web/src/components/ErrorToast.test.tsx web/src/app/App.tsx && git commit -m "feat: error toast"
```

---

## Task 16: End-to-end test of the full loop

**Files:**
- Create: `web/e2e/skeleton.spec.ts`

- [ ] **Step 1: Write the e2e test**

`web/e2e/skeleton.spec.ts`:
```ts
import { test, expect } from "@playwright/test";

test("create, edit, autosave, search, backlink, commit", async ({ page }) => {
  await page.goto("/");

  // Fixture notes are listed.
  await expect(page.getByText("index.md")).toBeVisible();
  await expect(page.getByText("ideas.md")).toBeVisible();

  // Open a note; its backlinks show (index.md links to ideas).
  await page.getByRole("button", { name: "ideas.md" }).click();
  await expect(page.getByText("Backlinks")).toBeVisible();
  await expect(
    page.locator("aside").last().getByRole("button", { name: "index.md" }),
  ).toBeVisible();

  // Create a new note that links to ideas.
  page.once("dialog", (d) => d.accept("fresh.md"));
  await page.getByRole("button", { name: /new note/i }).click();
  // Switch to raw mode for deterministic typing in the textarea.
  await page.getByRole("button", { name: /switch to raw/i }).click();
  // `locator("textarea")` (not getByRole("textbox")) — the Search input is also a textbox.
  await page.locator("textarea").fill("a new note pointing at [[ideas]]");

  // Autosave fires after the debounce; status returns to Saved.
  await expect(page.getByText(/saved/i)).toBeVisible({ timeout: 5000 });

  // Search finds the new note by body text.
  await page.getByPlaceholder("Search…").fill("pointing");
  await page.getByPlaceholder("Search…").press("Enter");
  const results = page.getByTestId("search-results");
  await expect(results.getByText(/Results/)).toBeVisible();
  // Scope to the overlay: "fresh.md" also exists in the note list.
  await results.getByRole("button", { name: "fresh.md" }).click();

  // ideas.md now has fresh.md as a backlink.
  await page.getByRole("button", { name: "ideas.md" }).click();
  await expect(
    page.locator("aside").last().getByRole("button", { name: "fresh.md" }),
  ).toBeVisible();

  // Manual commit records a commit id.
  page.once("dialog", (d) => d.accept("e2e snapshot"));
  await page.getByRole("button", { name: /^commit$/i }).click();
  await expect(page.getByText(/@c\d{4}/)).toBeVisible();
});
```

- [ ] **Step 2: Run the e2e test**

Run: `pnpm e2e`
Expected: PASS. (Playwright starts the dev server via `webServer` config.)

If the autosave assertion is flaky, confirm `DEFAULT_SETTINGS.autosaveMs` (1000ms) is well under the 5000ms timeout.

- [ ] **Step 3: Commit**

```bash
git add web/e2e/ && git commit -m "test: e2e of the full skeleton loop"
```

---

## Task 17: CI workflow

**Files:**
- Create: `.github/workflows/ci.yml`

- [ ] **Step 1: Write the workflow**

`.github/workflows/ci.yml`:
```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:

jobs:
  web:
    runs-on: ubuntu-latest
    defaults:
      run:
        working-directory: web
    steps:
      - uses: actions/checkout@v4
      - uses: pnpm/action-setup@v4
        with:
          version: 10.14.0
      - uses: actions/setup-node@v4
        with:
          node-version: 20
          cache: pnpm
          cache-dependency-path: web/pnpm-lock.yaml
      - run: pnpm install --frozen-lockfile
      - run: pnpm typecheck
      - run: pnpm lint
      - run: pnpm test
      - run: pnpm build
      - run: pnpm exec playwright install --with-deps chromium
      - run: pnpm e2e
```

- [ ] **Step 2: Verify the workflow is valid locally**

Run (from `web/`): `pnpm install --frozen-lockfile && pnpm typecheck && pnpm lint && pnpm test && pnpm build`
Expected: all PASS (this mirrors the CI steps minus the e2e browser install).

- [ ] **Step 3: Commit and push**

```bash
git add .github/workflows/ci.yml && git commit -m "ci: typecheck, lint, unit, build, e2e for web"
git push
```

- [ ] **Step 4: Confirm CI is green**

Run: `gh run watch` (or check the Actions tab). Expected: the `web` job passes.

---

## Done criteria

- Three-pane UI renders with the fixture cairn.
- Create / open / edit / delete notes; edits autosave (debounced); idle and interval auto-commit fire; manual commit works; backlinks and search reflect live state via the event stream; errors surface as a toast.
- All unit + component tests pass; the e2e full-loop test passes; typecheck, lint, build clean; CI green.
- The UI imports only `CairnClient`; the only Phase-2 change to render real notes is swapping `makeClient()` to return a `TauriClient`.
