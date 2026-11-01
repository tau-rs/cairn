# Tree-view Icons Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let users assign a custom icon (emoji or colored lucide line-icon) to any note or folder in the sidebar tree, with restrained folder/note visual differentiation and an optional per-folder color bar.

**Architecture:** A frontend-only presentation layer. A path-keyed `TreeStyleMap` lives in the Zustand store, persists to localStorage (mirroring `treePersistence.ts`), and remaps on rename/move via a pure function hooked into the existing `applyRenames`. A tabbed Radix popover (`IconPicker`) anchored to each row's leading icon sets the style. `FolderTreeView` renders an always-visible icon column where folders get a filled glyph + chevron and notes get an outline glyph.

**Tech Stack:** React + TypeScript, Zustand, Radix (`@radix-ui/react-popover` — new), `lucide-react` (new), Vitest + Testing Library, Tailwind tokens (`text-muted`, `text-faint`, `text-accent`, `border`, `surface`).

---

## File Structure

**New files (all under `web/src/components/tree/`):**
- `treeIcons.ts` — types (`IconRef`, `TreeItemStyle`, `TreeStyleMap`), `loadStyles`/`saveStyles` persistence, and pure `remapStyles(ops, map)`.
- `iconCatalog.ts` — curated lucide subset with names/keywords; `searchIcons`, `iconByName`.
- `emojiCatalog.ts` — curated emoji dataset with keywords; `searchEmoji`.
- `TreeItemIcon.tsx` — renders the correct leading glyph (custom emoji / custom lucide / default folder / default note).
- `IconPicker.tsx` — the tabbed Radix popover (Emoji | Icons tabs, icon-color row, folder-color footer, Remove).
- Test files alongside each: `treeIcons.test.ts`, `iconCatalog.test.ts`, `emojiCatalog.test.ts`, `TreeItemIcon.test.tsx`, `IconPicker.test.tsx`.

**Modified files:**
- `web/src/store/store.ts` — add `treeStyles` state + `setTreeStyle` action, load on init, `remapStyles` in `applyRenames`, drop key in `deleteNote`.
- `web/src/components/tree/FolderTreeView.tsx` — icon column, picker wiring, folder color bar; new `styles` + `onSetStyle` props.
- `web/src/components/Sidebar.tsx` — pass `styles` + `onSetStyle` down.

**Dependencies to add:** `lucide-react`, `@radix-ui/react-popover` (same family as the existing `@radix-ui/react-dialog`).

---

## Task 0: Add dependencies

**Files:**
- Modify: `web/package.json` (via pnpm)

- [ ] **Step 1: Install**

Run from `web/`:
```bash
pnpm add lucide-react @radix-ui/react-popover
```
Expected: both added to `dependencies`, lockfile updated.

- [ ] **Step 2: Verify install**

Run: `pnpm ls lucide-react @radix-ui/react-popover`
Expected: both resolve to a version (no "missing").

- [ ] **Step 3: Commit**

```bash
git add web/package.json web/pnpm-lock.yaml
git commit -m "build(tree): add lucide-react + radix-popover for tree icons"
```

---

## Task 1: `treeIcons.ts` — types + persistence

**Files:**
- Create: `web/src/components/tree/treeIcons.ts`
- Test: `web/src/components/tree/treeIcons.test.ts`

- [ ] **Step 1: Write the failing test**

```ts
import { describe, it, expect, beforeEach } from "vitest";
import { loadStyles, saveStyles, type TreeStyleMap } from "./treeIcons";

beforeEach(() => localStorage.clear());

describe("treeIcons persistence", () => {
  it("returns {} when nothing stored", () => {
    expect(loadStyles()).toEqual({});
  });

  it("round-trips a saved map", () => {
    const map: TreeStyleMap = {
      "notes/a.md": { icon: { kind: "emoji", value: "📚" } },
      notes: { folderColor: "#46b3e6" },
    };
    saveStyles(map);
    expect(loadStyles()).toEqual(map);
  });

  it("returns {} on malformed JSON", () => {
    localStorage.setItem("cairn.treeIcons", "{not json");
    expect(loadStyles()).toEqual({});
  });

  it("returns {} when stored value is not an object", () => {
    localStorage.setItem("cairn.treeIcons", "[1,2,3]");
    expect(loadStyles()).toEqual({});
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm vitest run src/components/tree/treeIcons.test.ts`
Expected: FAIL — cannot find module `./treeIcons`.

- [ ] **Step 3: Write minimal implementation**

```ts
export type IconRef =
  | { kind: "emoji"; value: string }
  | { kind: "lucide"; name: string; color: string };

export interface TreeItemStyle {
  icon?: IconRef;
  folderColor?: string; // folders only; the left-bar accent
}

export type TreeStyleMap = Record<string, TreeItemStyle>;

const STORAGE_KEY = "cairn.treeIcons";

export function loadStyles(): TreeStyleMap {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return {};
    const val = JSON.parse(raw) as unknown;
    if (!val || typeof val !== "object" || Array.isArray(val)) return {};
    return val as TreeStyleMap;
  } catch {
    return {};
  }
}

export function saveStyles(map: TreeStyleMap): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(map));
  } catch {
    // ignore (private mode / quota)
  }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm vitest run src/components/tree/treeIcons.test.ts`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add web/src/components/tree/treeIcons.ts web/src/components/tree/treeIcons.test.ts
git commit -m "feat(tree): add treeIcons types + localStorage persistence"
```

---

## Task 2: `treeIcons.ts` — `remapStyles` for rename/move

**Files:**
- Modify: `web/src/components/tree/treeIcons.ts`
- Test: `web/src/components/tree/treeIcons.test.ts`

`Rename` is `{ from: string; to: string }` (from `treeMoves.ts`). Note keys remap directly; folder keys remap by prefix derived from the parent-dir change in each op.

- [ ] **Step 1: Write the failing test (append to the existing describe block file)**

```ts
import { remapStyles } from "./treeIcons";

describe("remapStyles", () => {
  it("returns the same map for no ops", () => {
    const map = { "a.md": { icon: { kind: "emoji" as const, value: "🧠" } } };
    expect(remapStyles([], map)).toEqual(map);
  });

  it("remaps a renamed note key directly", () => {
    const map = { "a.md": { icon: { kind: "emoji" as const, value: "🧠" } } };
    const out = remapStyles([{ from: "a.md", to: "b.md" }], map);
    expect(out).toEqual({ "b.md": { icon: { kind: "emoji", value: "🧠" } } });
  });

  it("remaps a folder key and its descendants when a folder is renamed", () => {
    // folder "notes" -> "docs": engine emits ops for each descendant note
    const map = {
      notes: { folderColor: "#46b3e6" },
      "notes/a.md": { icon: { kind: "lucide" as const, name: "star", color: "#fff" } },
      "notes/sub": { folderColor: "#e5484d" },
      "other.md": { icon: { kind: "emoji" as const, value: "📌" } },
    };
    const ops = [
      { from: "notes/a.md", to: "docs/a.md" },
      { from: "notes/sub/b.md", to: "docs/sub/b.md" },
    ];
    const out = remapStyles(ops, map);
    expect(out).toEqual({
      docs: { folderColor: "#46b3e6" },
      "docs/a.md": { icon: { kind: "lucide", name: "star", color: "#fff" } },
      "docs/sub": { folderColor: "#e5484d" },
      "other.md": { icon: { kind: "emoji", value: "📌" } },
    });
  });

  it("remaps a moved folder (prefix change at the top level)", () => {
    const map = { "a/b": { folderColor: "#30a46c" } };
    const out = remapStyles([{ from: "a/b/x.md", to: "c/b/x.md" }], map);
    expect(out).toEqual({ "c/b": { folderColor: "#30a46c" } });
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm vitest run src/components/tree/treeIcons.test.ts`
Expected: FAIL — `remapStyles` is not exported.

- [ ] **Step 3: Write minimal implementation (append to `treeIcons.ts`)**

```ts
import type { Rename } from "./treeMoves";

/** Remap style-map keys so icons follow notes/folders across rename & move.
 *  Note keys remap directly from each op; folder keys remap by the parent-dir
 *  change derived from the ops (a folder rename/move emits descendant-note ops). */
export function remapStyles(ops: Rename[], map: TreeStyleMap): TreeStyleMap {
  if (ops.length === 0) return map;

  const noteMap = new Map(ops.map((o) => [o.from, o.to] as const));

  // Distinct (oldDir -> newDir) prefix changes from each op's parent segments.
  const prefixPairs: Array<[string, string]> = [];
  const seen = new Set<string>();
  for (const { from, to } of ops) {
    const f = from.split("/");
    const t = to.split("/");
    for (let i = 1; i < f.length; i++) {
      const fp = f.slice(0, i).join("/");
      const tp = t.slice(0, i).join("/");
      if (fp !== tp && !seen.has(fp)) {
        seen.add(fp);
        prefixPairs.push([fp, tp]);
      }
    }
  }

  const remapKey = (key: string): string => {
    const direct = noteMap.get(key);
    if (direct) return direct;
    for (const [fp, tp] of prefixPairs) {
      if (key === fp) return tp;
      if (key.startsWith(fp + "/")) return tp + key.slice(fp.length);
    }
    return key;
  };

  const out: TreeStyleMap = {};
  for (const [key, style] of Object.entries(map)) out[remapKey(key)] = style;
  return out;
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm vitest run src/components/tree/treeIcons.test.ts`
Expected: PASS (all tests).

- [ ] **Step 5: Commit**

```bash
git add web/src/components/tree/treeIcons.ts web/src/components/tree/treeIcons.test.ts
git commit -m "feat(tree): remap tree icon styles across rename/move"
```

---

## Task 3: `iconCatalog.ts` — curated lucide subset + search

**Files:**
- Create: `web/src/components/tree/iconCatalog.ts`
- Test: `web/src/components/tree/iconCatalog.test.ts`

- [ ] **Step 1: Write the failing test**

```ts
import { describe, it, expect } from "vitest";
import { ICON_CATALOG, iconByName, searchIcons } from "./iconCatalog";
import { FileText } from "lucide-react";

describe("iconCatalog", () => {
  it("has a non-empty catalog of named icons", () => {
    expect(ICON_CATALOG.length).toBeGreaterThan(10);
    expect(ICON_CATALOG.every((i) => i.name && i.Component)).toBe(true);
  });

  it("looks up an icon component by name", () => {
    expect(iconByName("star")).toBe(
      ICON_CATALOG.find((i) => i.name === "star")!.Component,
    );
  });

  it("falls back to FileText for an unknown name", () => {
    expect(iconByName("does-not-exist")).toBe(FileText);
  });

  it("searches by name and keyword (case-insensitive)", () => {
    const byName = searchIcons("star").map((i) => i.name);
    expect(byName).toContain("star");
    const byKeyword = searchIcons("favorite").map((i) => i.name);
    expect(byKeyword).toContain("star");
  });

  it("returns the full catalog for an empty query", () => {
    expect(searchIcons("").length).toBe(ICON_CATALOG.length);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm vitest run src/components/tree/iconCatalog.test.ts`
Expected: FAIL — cannot find module `./iconCatalog`.

- [ ] **Step 3: Write minimal implementation**

```ts
import {
  Folder,
  FileText,
  Star,
  Bookmark,
  Tag,
  Calendar,
  Clock,
  CheckCircle,
  Flag,
  Heart,
  Lightbulb,
  Zap,
  Target,
  Rocket,
  Box,
  Pin,
  Link,
  Hash,
  Layers,
  Code,
  Terminal,
  Database,
  Settings,
  Search,
  Music,
  Image,
  Coffee,
  Briefcase,
  type LucideIcon,
} from "lucide-react";

export interface CatalogIcon {
  name: string;
  Component: LucideIcon;
  keywords: string[];
}

export const ICON_CATALOG: CatalogIcon[] = [
  { name: "folder", Component: Folder, keywords: ["directory"] },
  { name: "file", Component: FileText, keywords: ["document", "note"] },
  { name: "star", Component: Star, keywords: ["favorite", "important"] },
  { name: "bookmark", Component: Bookmark, keywords: ["save", "read"] },
  { name: "tag", Component: Tag, keywords: ["label"] },
  { name: "calendar", Component: Calendar, keywords: ["date", "schedule"] },
  { name: "clock", Component: Clock, keywords: ["time", "recent"] },
  { name: "check", Component: CheckCircle, keywords: ["done", "task", "todo"] },
  { name: "flag", Component: Flag, keywords: ["milestone", "priority"] },
  { name: "heart", Component: Heart, keywords: ["love", "like"] },
  { name: "idea", Component: Lightbulb, keywords: ["lightbulb", "think"] },
  { name: "zap", Component: Zap, keywords: ["fast", "energy", "action"] },
  { name: "target", Component: Target, keywords: ["goal", "aim"] },
  { name: "rocket", Component: Rocket, keywords: ["launch", "ship"] },
  { name: "box", Component: Box, keywords: ["package", "archive"] },
  { name: "pin", Component: Pin, keywords: ["map", "location"] },
  { name: "link", Component: Link, keywords: ["url", "reference"] },
  { name: "hash", Component: Hash, keywords: ["number", "tag"] },
  { name: "layers", Component: Layers, keywords: ["stack", "group"] },
  { name: "code", Component: Code, keywords: ["dev", "snippet"] },
  { name: "terminal", Component: Terminal, keywords: ["shell", "cli"] },
  { name: "database", Component: Database, keywords: ["data", "store"] },
  { name: "settings", Component: Settings, keywords: ["gear", "config"] },
  { name: "search", Component: Search, keywords: ["find", "magnify"] },
  { name: "music", Component: Music, keywords: ["audio", "song"] },
  { name: "image", Component: Image, keywords: ["photo", "picture"] },
  { name: "coffee", Component: Coffee, keywords: ["break", "cafe"] },
  { name: "work", Component: Briefcase, keywords: ["job", "business"] },
];

const BY_NAME = new Map(ICON_CATALOG.map((i) => [i.name, i.Component]));

export function iconByName(name: string): LucideIcon {
  return BY_NAME.get(name) ?? FileText;
}

export function searchIcons(query: string): CatalogIcon[] {
  const q = query.trim().toLowerCase();
  if (!q) return ICON_CATALOG;
  return ICON_CATALOG.filter(
    (i) =>
      i.name.includes(q) || i.keywords.some((k) => k.includes(q)),
  );
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm vitest run src/components/tree/iconCatalog.test.ts`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
git add web/src/components/tree/iconCatalog.ts web/src/components/tree/iconCatalog.test.ts
git commit -m "feat(tree): curated lucide icon catalog + search"
```

---

## Task 4: `emojiCatalog.ts` — curated emoji dataset + search

**Files:**
- Create: `web/src/components/tree/emojiCatalog.ts`
- Test: `web/src/components/tree/emojiCatalog.test.ts`

- [ ] **Step 1: Write the failing test**

```ts
import { describe, it, expect } from "vitest";
import { EMOJI_CATALOG, searchEmoji } from "./emojiCatalog";

describe("emojiCatalog", () => {
  it("has a non-empty catalog with chars + groups", () => {
    expect(EMOJI_CATALOG.length).toBeGreaterThan(20);
    expect(EMOJI_CATALOG.every((e) => e.char && e.name && e.group)).toBe(true);
  });

  it("searches by name and keyword (case-insensitive)", () => {
    expect(searchEmoji("book").some((e) => e.char === "📚")).toBe(true);
    expect(searchEmoji("idea").some((e) => e.char === "💡")).toBe(true);
  });

  it("returns the full catalog for an empty query", () => {
    expect(searchEmoji("").length).toBe(EMOJI_CATALOG.length);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm vitest run src/components/tree/emojiCatalog.test.ts`
Expected: FAIL — cannot find module `./emojiCatalog`.

- [ ] **Step 3: Write minimal implementation**

```ts
export interface EmojiEntry {
  char: string;
  name: string;
  keywords: string[];
  group: string;
}

export const EMOJI_CATALOG: EmojiEntry[] = [
  // Frequently used
  { char: "📁", name: "folder", keywords: ["directory"], group: "Frequently used" },
  { char: "📚", name: "books", keywords: ["book", "library", "read"], group: "Frequently used" },
  { char: "🧠", name: "brain", keywords: ["think", "mind"], group: "Frequently used" },
  { char: "📝", name: "memo", keywords: ["note", "write"], group: "Frequently used" },
  { char: "⭐", name: "star", keywords: ["favorite"], group: "Frequently used" },
  { char: "🔥", name: "fire", keywords: ["hot", "trending"], group: "Frequently used" },
  { char: "💡", name: "bulb", keywords: ["idea", "light"], group: "Frequently used" },
  { char: "✅", name: "check", keywords: ["done", "task"], group: "Frequently used" },
  // Objects
  { char: "📦", name: "package", keywords: ["box", "archive"], group: "Objects" },
  { char: "📌", name: "pushpin", keywords: ["pin", "location"], group: "Objects" },
  { char: "🔖", name: "bookmark", keywords: ["save", "read"], group: "Objects" },
  { char: "📅", name: "calendar", keywords: ["date", "schedule"], group: "Objects" },
  { char: "🗂️", name: "dividers", keywords: ["files", "organize"], group: "Objects" },
  { char: "📊", name: "chart", keywords: ["graph", "data"], group: "Objects" },
  { char: "🧩", name: "puzzle", keywords: ["piece", "plugin"], group: "Objects" },
  { char: "🔗", name: "link", keywords: ["url", "chain"], group: "Objects" },
  { char: "⚙️", name: "gear", keywords: ["settings", "config"], group: "Objects" },
  { char: "🔑", name: "key", keywords: ["password", "secret"], group: "Objects" },
  { char: "💼", name: "briefcase", keywords: ["work", "business"], group: "Objects" },
  { char: "🎯", name: "target", keywords: ["goal", "aim"], group: "Objects" },
  { char: "🚀", name: "rocket", keywords: ["launch", "ship"], group: "Objects" },
  { char: "⏰", name: "alarm", keywords: ["clock", "time"], group: "Objects" },
  { char: "📷", name: "camera", keywords: ["photo", "picture"], group: "Objects" },
  // Symbols
  { char: "❤️", name: "heart", keywords: ["love", "like"], group: "Symbols" },
  { char: "⚡", name: "zap", keywords: ["fast", "energy"], group: "Symbols" },
  { char: "🏷️", name: "label", keywords: ["tag"], group: "Symbols" },
  { char: "❓", name: "question", keywords: ["help", "ask"], group: "Symbols" },
  { char: "❗", name: "exclamation", keywords: ["important", "alert"], group: "Symbols" },
  // Nature
  { char: "🌱", name: "seedling", keywords: ["plant", "grow", "new"], group: "Nature" },
  { char: "🌍", name: "globe", keywords: ["world", "earth"], group: "Nature" },
  { char: "☕", name: "coffee", keywords: ["break", "cafe"], group: "Nature" },
];

export function searchEmoji(query: string): EmojiEntry[] {
  const q = query.trim().toLowerCase();
  if (!q) return EMOJI_CATALOG;
  return EMOJI_CATALOG.filter(
    (e) =>
      e.name.includes(q) || e.keywords.some((k) => k.includes(q)),
  );
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm vitest run src/components/tree/emojiCatalog.test.ts`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add web/src/components/tree/emojiCatalog.ts web/src/components/tree/emojiCatalog.test.ts
git commit -m "feat(tree): curated emoji catalog + search"
```

---

## Task 5: `TreeItemIcon.tsx` — leading glyph renderer

**Files:**
- Create: `web/src/components/tree/TreeItemIcon.tsx`
- Test: `web/src/components/tree/TreeItemIcon.test.tsx`

Renders the leading glyph: custom emoji (text), custom lucide (colored), or the default (filled `Folder` for folders / outline `FileText` for notes). lucide-react renders an `<svg>` with a class like `lucide-folder` / `lucide-file-text`, which the tests assert against.

- [ ] **Step 1: Write the failing test**

```tsx
import { describe, it, expect } from "vitest";
import { render } from "@testing-library/react";
import { TreeItemIcon } from "./TreeItemIcon";

describe("TreeItemIcon", () => {
  it("renders a custom emoji", () => {
    const { getByText } = render(
      <TreeItemIcon kind="note" style={{ icon: { kind: "emoji", value: "📚" } }} />,
    );
    expect(getByText("📚")).toBeInTheDocument();
  });

  it("renders a custom lucide icon with its color", () => {
    const { container } = render(
      <TreeItemIcon
        kind="note"
        style={{ icon: { kind: "lucide", name: "star", color: "rgb(70, 179, 230)" } }}
      />,
    );
    const svg = container.querySelector("svg.lucide-star");
    expect(svg).toBeTruthy();
    expect(svg!.getAttribute("color")).toBe("rgb(70, 179, 230)");
  });

  it("renders the default filled folder glyph for a folder with no icon", () => {
    const { container } = render(<TreeItemIcon kind="folder" />);
    expect(container.querySelector("svg.lucide-folder")).toBeTruthy();
  });

  it("renders the default outline doc glyph for a note with no icon", () => {
    const { container } = render(<TreeItemIcon kind="note" />);
    expect(container.querySelector("svg.lucide-file-text")).toBeTruthy();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm vitest run src/components/tree/TreeItemIcon.test.tsx`
Expected: FAIL — cannot find module `./TreeItemIcon`.

- [ ] **Step 3: Write minimal implementation**

```tsx
import { Folder, FileText } from "lucide-react";
import { iconByName } from "./iconCatalog";
import type { TreeItemStyle } from "./treeIcons";

export function TreeItemIcon({
  kind,
  style,
}: {
  kind: "folder" | "note";
  style?: TreeItemStyle;
}) {
  const icon = style?.icon;

  if (icon?.kind === "emoji") {
    return (
      <span aria-hidden className="text-[15px] leading-none">
        {icon.value}
      </span>
    );
  }

  if (icon?.kind === "lucide") {
    const Cmp = iconByName(icon.name);
    return <Cmp aria-hidden size={16} color={icon.color} />;
  }

  if (kind === "folder") {
    // filled folder, single muted accent
    return <Folder aria-hidden size={16} fill="currentColor" className="text-muted" />;
  }
  return <FileText aria-hidden size={16} className="text-faint" />;
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm vitest run src/components/tree/TreeItemIcon.test.tsx`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add web/src/components/tree/TreeItemIcon.tsx web/src/components/tree/TreeItemIcon.test.tsx
git commit -m "feat(tree): TreeItemIcon glyph renderer (custom + default)"
```

---

## Task 6: `IconPicker.tsx` — tabbed Radix popover

**Files:**
- Create: `web/src/components/tree/IconPicker.tsx`
- Test: `web/src/components/tree/IconPicker.test.tsx`

The picker wraps a caller-supplied `trigger` in a Radix `Popover`. Content: tabs **Emoji | Icons**, each with a search box + grid; the Icons tab has an **Icon color** swatch row; folders get a **Folder color** footer; a **Remove** button clears the icon. `onChange(style)` receives the *full* new `TreeItemStyle` (merging icon/folderColor). Color palette is a shared constant.

- [ ] **Step 1: Write the failing test**

```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { IconPicker } from "./IconPicker";

function open(kind: "folder" | "note", onChange = vi.fn()) {
  render(
    <IconPicker
      targetKind={kind}
      value={{}}
      onChange={onChange}
      trigger={<button>set icon</button>}
    />,
  );
  return onChange;
}

describe("IconPicker", () => {
  it("opens on trigger click and shows both tabs", async () => {
    open("note");
    await userEvent.click(screen.getByText("set icon"));
    expect(screen.getByRole("tab", { name: "Emoji" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Icons" })).toBeInTheDocument();
  });

  it("selecting an emoji calls onChange with an emoji IconRef", async () => {
    const onChange = open("note");
    await userEvent.click(screen.getByText("set icon"));
    await userEvent.click(screen.getByRole("button", { name: "books 📚" }));
    expect(onChange).toHaveBeenCalledWith({ icon: { kind: "emoji", value: "📚" } });
  });

  it("filters emoji by search", async () => {
    open("note");
    await userEvent.click(screen.getByText("set icon"));
    await userEvent.type(screen.getByPlaceholderText("Search emoji…"), "idea");
    expect(screen.getByRole("button", { name: "bulb 💡" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "books 📚" })).not.toBeInTheDocument();
  });

  it("selecting a lucide icon uses the selected color", async () => {
    const onChange = open("note");
    await userEvent.click(screen.getByText("set icon"));
    await userEvent.click(screen.getByRole("tab", { name: "Icons" }));
    await userEvent.click(screen.getByRole("button", { name: "color #e5484d" }));
    await userEvent.click(screen.getByRole("button", { name: "icon star" }));
    expect(onChange).toHaveBeenCalledWith({
      icon: { kind: "lucide", name: "star", color: "#e5484d" },
    });
  });

  it("Remove clears the icon", async () => {
    const onChange = open("note");
    await userEvent.click(screen.getByText("set icon"));
    await userEvent.click(screen.getByRole("button", { name: "Remove" }));
    expect(onChange).toHaveBeenCalledWith({});
  });

  it("shows the Folder color footer only for folders", async () => {
    open("folder");
    await userEvent.click(screen.getByText("set icon"));
    expect(screen.getByText("Folder color")).toBeInTheDocument();
  });

  it("hides the Folder color footer for notes", async () => {
    open("note");
    await userEvent.click(screen.getByText("set icon"));
    expect(screen.queryByText("Folder color")).not.toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm vitest run src/components/tree/IconPicker.test.tsx`
Expected: FAIL — cannot find module `./IconPicker`.

- [ ] **Step 3: Write minimal implementation**

```tsx
import { useState } from "react";
import type { ReactNode } from "react";
import * as Popover from "@radix-ui/react-popover";
import { searchEmoji } from "./emojiCatalog";
import { ICON_CATALOG, searchIcons } from "./iconCatalog";
import type { TreeItemStyle } from "./treeIcons";

/** Shared palette (theme-independent so it reads in dark/light/nord). */
export const ICON_COLORS = [
  "#5b8def", // accent
  "#9ca0a8",
  "#e5484d",
  "#f5a623",
  "#30a46c",
  "#46b3e6",
  "#8e7bef",
  "#e668c3",
];

export function IconPicker({
  targetKind,
  value,
  onChange,
  trigger,
}: {
  targetKind: "folder" | "note";
  value: TreeItemStyle;
  onChange: (style: TreeItemStyle) => void;
  trigger: ReactNode;
}) {
  const [tab, setTab] = useState<"emoji" | "icons">("emoji");
  const [query, setQuery] = useState("");
  const [color, setColor] = useState(ICON_COLORS[0]);

  const setIcon = (icon: TreeItemStyle["icon"]) =>
    onChange({ ...value, icon });

  return (
    <Popover.Root onOpenChange={() => setQuery("")}>
      <Popover.Trigger asChild>{trigger}</Popover.Trigger>
      <Popover.Portal>
        <Popover.Content
          align="start"
          sideOffset={4}
          className="z-50 w-[300px] overflow-hidden rounded-xl border border-border bg-surface text-text shadow-2xl focus:outline-none"
        >
          <div className="flex items-center gap-1 px-2 pt-2">
            <button
              role="tab"
              aria-selected={tab === "emoji"}
              className={`rounded-t px-3 py-1.5 text-sm ${tab === "emoji" ? "bg-surface-2 text-text" : "text-muted"}`}
              onClick={() => setTab("emoji")}
            >
              Emoji
            </button>
            <button
              role="tab"
              aria-selected={tab === "icons"}
              className={`rounded-t px-3 py-1.5 text-sm ${tab === "icons" ? "bg-surface-2 text-text" : "text-muted"}`}
              onClick={() => setTab("icons")}
            >
              Icons
            </button>
            <span className="flex-1" />
            <button
              className="px-2 py-1 text-xs text-faint hover:text-danger"
              onClick={() => onChange({ ...value, icon: undefined })}
            >
              Remove
            </button>
          </div>

          <input
            className="m-2 w-[calc(100%-1rem)] rounded border border-border bg-surface-2 px-2.5 py-1.5 text-sm text-text outline-none focus:border-accent"
            placeholder={tab === "emoji" ? "Search emoji…" : "Search icons…"}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />

          {tab === "emoji" ? (
            <div className="grid max-h-[190px] grid-cols-8 gap-0.5 overflow-y-auto px-2 pb-2">
              {searchEmoji(query).map((e) => (
                <button
                  key={e.char}
                  aria-label={`${e.name} ${e.char}`}
                  className="flex aspect-square items-center justify-center rounded text-[17px] hover:bg-surface-2"
                  onClick={() => setIcon({ kind: "emoji", value: e.char })}
                >
                  {e.char}
                </button>
              ))}
            </div>
          ) : (
            <>
              <div className="px-3 pb-1 pt-0.5 text-[11px] uppercase tracking-wide text-faint">
                Icon color
              </div>
              <div className="flex flex-wrap gap-1.5 px-3 pb-2">
                {ICON_COLORS.map((c) => (
                  <button
                    key={c}
                    aria-label={`color ${c}`}
                    onClick={() => setColor(c)}
                    style={{ background: c }}
                    className={`h-5 w-5 rounded-full border-2 ${color === c ? "border-text" : "border-transparent"}`}
                  />
                ))}
              </div>
              <div className="grid max-h-[170px] grid-cols-7 gap-0.5 overflow-y-auto px-2 pb-2">
                {searchIcons(query).map(({ name, Component }) => (
                  <button
                    key={name}
                    aria-label={`icon ${name}`}
                    className="flex aspect-square items-center justify-center rounded hover:bg-surface-2"
                    onClick={() => setIcon({ kind: "lucide", name, color })}
                  >
                    <Component size={17} color={color} />
                  </button>
                ))}
              </div>
            </>
          )}

          {targetKind === "folder" && (
            <div className="border-t border-border px-3 py-2">
              <div className="pb-1 text-[11px] uppercase tracking-wide text-faint">
                Folder color
              </div>
              <div className="flex flex-wrap gap-1.5">
                <button
                  aria-label="folder color none"
                  onClick={() => onChange({ ...value, folderColor: undefined })}
                  className={`flex h-5 w-5 items-center justify-center rounded-full border-2 bg-surface-2 text-[11px] text-faint ${value.folderColor ? "border-transparent" : "border-text"}`}
                >
                  ∅
                </button>
                {ICON_COLORS.slice(2).map((c) => (
                  <button
                    key={c}
                    aria-label={`folder color ${c}`}
                    onClick={() => onChange({ ...value, folderColor: c })}
                    style={{ background: c }}
                    className={`h-5 w-5 rounded-full border-2 ${value.folderColor === c ? "border-text" : "border-transparent"}`}
                  />
                ))}
              </div>
            </div>
          )}
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm vitest run src/components/tree/IconPicker.test.tsx`
Expected: PASS (7 tests). If Radix Popover content doesn't mount in jsdom on click, ensure `@testing-library/user-event` is used (it is) — Radix opens on pointer/click events it dispatches.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/tree/IconPicker.tsx web/src/components/tree/IconPicker.test.tsx
git commit -m "feat(tree): tabbed IconPicker popover (emoji/icons/color)"
```

---

## Task 7: Store wiring — `treeStyles` state, `setTreeStyle`, remap, delete cleanup

**Files:**
- Modify: `web/src/store/store.ts`
- Test: `web/src/store/store.test.ts`

The store auto-exposes function members as actions (see `cairnStore.ts`), so adding `setTreeStyle` surfaces it through `useActions()` automatically. `treeStyles` is read via `useCairn`.

- [ ] **Step 1: Write the failing test (append to `store.test.ts`)**

Match the existing test harness in `store.test.ts` for constructing a store (reuse its existing `makeStore`/setup helper — check the top of the file). The assertions:

```ts
import { remapStyles } from "../components/tree/treeIcons"; // (only if needed)

describe("treeStyles", () => {
  it("setTreeStyle stores and persists a style", () => {
    const store = makeStore(); // use the file's existing helper
    store.getState().setTreeStyle("a.md", { icon: { kind: "emoji", value: "📚" } });
    expect(store.getState().treeStyles["a.md"]).toEqual({
      icon: { kind: "emoji", value: "📚" },
    });
    expect(JSON.parse(localStorage.getItem("cairn.treeIcons")!)["a.md"]).toEqual({
      icon: { kind: "emoji", value: "📚" },
    });
  });

  it("setTreeStyle with an empty style deletes the key", () => {
    const store = makeStore();
    store.getState().setTreeStyle("a.md", { icon: { kind: "emoji", value: "📚" } });
    store.getState().setTreeStyle("a.md", {});
    expect(store.getState().treeStyles["a.md"]).toBeUndefined();
  });
});
```

(If `store.test.ts` has no reusable store-builder, follow its existing pattern for instantiating `createCairnStore` with the fake client/host already used by the other tests in that file.)

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm vitest run src/store/store.test.ts -t treeStyles`
Expected: FAIL — `setTreeStyle` / `treeStyles` undefined.

- [ ] **Step 3: Implement in `store.ts`**

3a. Add the import near the other tree imports at the top:
```ts
import {
  loadStyles,
  saveStyles,
  remapStyles,
  type TreeStyleMap,
  type TreeItemStyle,
} from "../components/tree/treeIcons";
```

3b. Add to the `CairnState` interface (near `ui: UiState;` ~line 109 and the action signatures ~line 133-154):
```ts
  treeStyles: TreeStyleMap;
  setTreeStyle(path: string, style: TreeItemStyle): void;
```

3c. Add to the initial state object (near `ui: DEFAULT_UI,` ~line 400):
```ts
      treeStyles: loadStyles(),
```

3d. Add the action implementation (alongside `setUi`, ~line 832):
```ts
      setTreeStyle(path, style) {
        set((s) => {
          const next = { ...s.treeStyles };
          if (!style.icon && !style.folderColor) delete next[path];
          else next[path] = style;
          saveStyles(next);
          return { treeStyles: next };
        });
      },
```

3e. In `applyRenames`, after the `for` loop and before the final `persist();` (~line 597), add:
```ts
        set((s) => {
          const treeStyles = remapStyles(ops, s.treeStyles);
          saveStyles(treeStyles);
          return { treeStyles };
        });
```

3f. In the `deleteNote` action (grep `deleteNote(` in `store.ts`), after the note is removed from state, drop its style key — add inside that action's `set(...)` updater or as a follow-up:
```ts
        set((s) => {
          if (!(path in s.treeStyles)) return {};
          const next = { ...s.treeStyles };
          delete next[path];
          saveStyles(next);
          return { treeStyles: next };
        });
```
(Use the delete action's existing parameter name for the path; adjust `path` if it differs.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `pnpm vitest run src/store/store.test.ts -t treeStyles`
Expected: PASS. Also run the full store suite to catch regressions: `pnpm vitest run src/store/store.test.ts` → all PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/store/store.ts web/src/store/store.test.ts
git commit -m "feat(store): own treeStyles map (set/persist/remap/delete)"
```

---

## Task 8: `FolderTreeView.tsx` — icon column, picker, color bar

**Files:**
- Modify: `web/src/components/tree/FolderTreeView.tsx`
- Test: `web/src/components/tree/FolderTreeView.test.tsx`

Add two props and render an always-visible icon column. The icon is the `IconPicker` trigger. Notes render an invisible chevron spacer to keep the icon column aligned. Folders with a `folderColor` get a thin left bar.

- [ ] **Step 1: Write the failing test (append to `FolderTreeView.test.tsx`; extend `setup` to pass the new props)**

```tsx
import { TreeItemIcon } from "./TreeItemIcon"; // not needed; illustrative

// Extend setup()'s default props with:
//   styles: {},
//   onSetStyle: vi.fn(),

it("renders a default folder glyph and note glyph", () => {
  setup();
  // 'notes' folder -> filled folder glyph; 'index' note -> doc glyph
  expect(document.querySelector("svg.lucide-folder")).toBeTruthy();
  expect(document.querySelector("svg.lucide-file-text")).toBeTruthy();
});

it("renders a custom emoji from styles", () => {
  setup({ styles: { "index.md": { icon: { kind: "emoji", value: "📚" } } } });
  expect(screen.getByText("📚")).toBeInTheDocument();
});

it("opens the icon picker when the icon trigger is clicked", async () => {
  setup();
  const trigger = screen.getByRole("button", { name: "set icon for index.md" });
  await userEvent.click(trigger);
  expect(screen.getByRole("tab", { name: "Emoji" })).toBeInTheDocument();
});

it("draws a folder color bar when folderColor is set", () => {
  setup({ styles: { notes: { folderColor: "#46b3e6" } } });
  expect(document.querySelector('[data-folder-bar="true"]')).toBeTruthy();
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm vitest run src/components/tree/FolderTreeView.test.tsx`
Expected: FAIL — new props/behaviour not present (e.g. no "set icon for index.md" button).

- [ ] **Step 3: Implement the changes in `FolderTreeView.tsx`**

3a. Add imports:
```ts
import { TreeItemIcon } from "./TreeItemIcon";
import { IconPicker } from "./IconPicker";
import type { TreeStyleMap, TreeItemStyle } from "./treeIcons";
```

3b. Add to the `FolderTree` props type:
```ts
  styles: TreeStyleMap;
  onSetStyle: (path: string, style: TreeItemStyle) => void;
```

3c. Add a small helper inside the component (before `renderNodes`) to render the icon trigger + picker for any node:
```ts
  const iconCell = (path: string, kind: "folder" | "note") => (
    <IconPicker
      targetKind={kind}
      value={props.styles[path] ?? {}}
      onChange={(style) => props.onSetStyle(path, style)}
      trigger={
        <button
          aria-label={`set icon for ${path}`}
          className="flex h-[18px] w-[18px] flex-none items-center justify-center"
          onClick={(e) => e.stopPropagation()}
        >
          <TreeItemIcon kind={kind} style={props.styles[path]} />
        </button>
      }
    />
  );
```

3d. In the **folder** branch of `renderNodes`, insert `iconCell` between the chevron and the name, and add the color bar + `position: relative`. Replace the folder's inner button block so the row reads: chevron toggle → icon cell → name. Concretely, change the folder `<button>` (the one with the chevron + name) to keep the chevron + name as the toggle, and place `iconCell` *outside* that toggle button (a button cannot nest a button). Restructure the folder row's left side to:
```tsx
              <div className="flex min-w-0 flex-1 items-center gap-1" style={pad}>
                <button
                  aria-label={`toggle ${node.path}`}
                  className="flex flex-none items-center text-faint"
                  onClick={() => toggle(node.path)}
                >
                  <span aria-hidden>{isCollapsed ? "▸" : "▾"}</span>
                </button>
                {iconCell(node.path, "folder")}
                <button
                  className="min-w-0 flex-1 truncate py-1 text-left"
                  title={node.path}
                  onClick={() => toggle(node.path)}
                  onDoubleClick={() => setEditingPath(node.path)}
                  onKeyDown={(e) => {
                    if (e.key === "F2") {
                      e.preventDefault();
                      setEditingPath(node.path);
                    }
                  }}
                >
                  <span className="truncate text-text">{node.name}</span>
                </button>
              </div>
```
And add `style={{ position: "relative" }}` to the folder row's outer `<div>`, plus the bar element as its first child:
```tsx
              {props.styles[node.path]?.folderColor && (
                <span
                  data-folder-bar="true"
                  aria-hidden
                  className="absolute bottom-1 left-0.5 top-1 w-[2.5px] rounded"
                  style={{ background: props.styles[node.path]!.folderColor }}
                />
              )}
```
(Keep the existing `editing ? <RenameInput/> : ...` branch — render the above non-editing block only when `!editing`.)

3e. In the **note** branch, add a chevron spacer + `iconCell` before the name button. The note's name `<button>` stays as-is (opens the note); prepend:
```tsx
          {!editing && (
            <span className="flex flex-none items-center gap-1" style={pad}>
              <span aria-hidden className="w-[11px]" /> {/* chevron spacer */}
              {iconCell(node.path, "note")}
            </span>
          )}
```
and remove `style={pad}` from the note's name button (the wrapper now owns the indent), letting the name button be `className="min-w-0 flex-1 truncate py-1 text-left"` without padding. Ensure the row remains a flex container.

> Note: a button must not contain another button. `iconCell` renders a `<button>` trigger, so it must always sit **outside** the toggle/open buttons, as shown.

- [ ] **Step 4: Run tests to verify they pass**

Run: `pnpm vitest run src/components/tree/FolderTreeView.test.tsx`
Expected: PASS, including the existing rename/collapse/drag tests. Also run the dnd test: `pnpm vitest run src/components/tree/FolderTreeView.dnd.test.tsx` → PASS. If the existing tests query the folder by `getByRole("button", { name: "notes" })`, update them to the new accessible name (the toggle button label) or keep the name button's text query — adjust the existing assertions to match the restructured markup as needed.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/tree/FolderTreeView.tsx web/src/components/tree/FolderTreeView.test.tsx
git commit -m "feat(tree): icon column + picker + folder color bar in the tree"
```

---

## Task 9: `Sidebar.tsx` — pass styles + onSetStyle

**Files:**
- Modify: `web/src/components/Sidebar.tsx`

- [ ] **Step 1: Wire the new props**

Add a selector and pass the props:
```tsx
  const treeStyles = useCairn((s) => s.treeStyles);
```
and on `<FolderTree ...>`:
```tsx
        styles={treeStyles}
        onSetStyle={actions.setTreeStyle}
```

- [ ] **Step 2: Typecheck + full gate**

Run from `web/`:
```bash
pnpm tsc --noEmit && pnpm vitest run && pnpm lint && pnpm format:check
```
Expected: all green. (Per the `ci-local-gates` learning, `format:check` is easy to miss — run it.)

- [ ] **Step 3: Commit**

```bash
git add web/src/components/Sidebar.tsx
git commit -m "feat(tree): pass treeStyles + setTreeStyle into the sidebar tree"
```

---

## Task 10: Manual verification

- [ ] **Step 1: Run the app**

Run from `web/`: `pnpm dev` (or the project's run skill). Open the sidebar.

- [ ] **Step 2: Verify each behavior**
  - Default state: folders show a filled folder glyph + chevron; notes show an outline doc glyph. Clearly distinguishable.
  - Click a note's icon → picker opens; pick an emoji → it appears in the tree and persists across reload.
  - Pick a lucide icon with a color on the Icons tab → renders colored.
  - On a folder, set a Folder color → a thin left bar appears.
  - Rename a note/folder and move a folder (drag) → icons follow.
  - Remove → reverts to the default glyph.

- [ ] **Step 3: Commit (if any tweaks were needed)** — otherwise done.

---

## Self-Review notes (for the implementer)

- **Spec coverage:** both notes+folders (T5/T8), emoji+icons tabs (T6), icon color (T6), folder differentiation via filled-vs-outline glyph (T5) + chevron (T8), optional folder color bar (T6/T8), frontend-only localStorage (T1), rename/move remap (T2/T7), Remove (T6). All covered.
- **Type consistency:** `IconRef`, `TreeItemStyle`, `TreeStyleMap` defined in T1 and used unchanged through T5–T8. `setTreeStyle(path, style)` signature matches store (T7) and Sidebar (T9). `searchIcons`/`iconByName` (T3), `searchEmoji` (T4) names consistent with their consumers.
- **Existing-test risk (flagged in T8):** restructuring the folder row changes accessible names; update the existing `FolderTreeView.test.tsx` assertions in the same task rather than leaving them red.
