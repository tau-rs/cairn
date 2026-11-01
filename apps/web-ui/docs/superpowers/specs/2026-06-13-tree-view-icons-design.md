# Tree-view icons (Notion-style) — design

**Date:** 2026-06-13
**Status:** Approved (brainstorm), pre-plan
**Area:** `web/src/components/tree/` + persistence + a new icon-picker component

## 1. Goal

Let the user assign a custom icon to any item in the folder-tree sidebar — both
**notes** and **folders** — the way Notion lets you set a page icon. Icons may be
an **emoji** or a **line-icon** (lucide), and line-icons can be **colored**.
Independently, improve the **visual differentiation** between folders and notes so
the tree reads clearly even when both carry custom icons.

Scope is the **left sidebar tree only** (`FolderTree`). No engine changes.

## 2. Decisions (locked during brainstorm)

| Question | Decision |
|---|---|
| Which items get icons | **Both** notes and folders |
| Icon sources | **Emoji** and **lucide line-icons**, in a **tab-separated** picker |
| Line-icon color | Yes — a fixed color palette; **default = theme accent**; applies to **lucide only** (emoji are already multicolor) |
| Folder vs. note differentiation | **Restrained**: filled folder glyph vs. outline doc glyph + chevron (folders only) + full-strength folder label. **No** background bands, **no** bold, **no** open/closed swap. |
| Per-folder color | Optional **thin left accent bar** (off by default) — an accent, not a row fill |
| Storage | **Frontend-only** (localStorage), keyed by path, for both notes and folders — mirrors `treePersistence.ts` |

### Rejected alternatives (and why)

- **Always-on folder background tint / colored row fills** — tried in mockup; reads
  busy and muddy, fights the icon-color feature. Dropped in favor of restraint.
- **Bold folder names + open/closed folder icon** — redundant once filled-vs-outline
  + chevron carry the distinction. Dropped (YAGNI).
- **Chevron negative-indent (cue 5)** — explicitly rejected by user.
- **Note icons in frontmatter (portable)** — defers; needs an engine round-trip and a
  second storage path. Frontend-only is consistent with the existing tree persistence.
  Frontmatter portability is a possible **future** enhancement, out of scope here.

## 3. UX specification

### 3.1 Tree rendering

Every row renders a fixed-width **leading icon column**, always visible (no hover
affordance, no dashed placeholder). Layout per row, left→right:

- **Chevron slot** (fixed width): folders show `▸`/`▾`; notes render an invisible
  spacer of the same width so the icon column stays vertically aligned.
- **Icon slot** (fixed width, ~18px):
  - If a custom icon is set → render it (emoji as text; lucide as the colored SVG).
  - Else → the **default glyph**: folders get a **filled folder** glyph in a single
    muted accent; notes get a **thin outline doc** glyph in the faint tone.
- **Label**: folder labels render at full-strength text color; note labels at the
  muted tone. (Active note keeps today's active styling.)
- **Optional folder color bar**: if a folder has a color set, a ~2.5px rounded
  vertical bar in that color is drawn at the row's left edge. Off by default.

Existing behaviors are preserved: collapse/expand, inline rename, drag-to-move,
hover "+ new note" (folders) and "✕ delete" (notes), active-note highlight.

### 3.2 Opening the picker

The leading **icon slot is the click target** that opens the picker popover,
anchored to that row. (Clicking the icon must not trigger open-note / toggle-folder —
the icon button stops propagation, like the existing rename input does.)

The picker is a popover built on the existing `@radix-ui/react-dialog`-style overlay
pattern already used for the command palette (no new overlay primitive).

### 3.3 Picker contents

- **Two tabs: `Emoji` | `Icons`.** Each tab has its own search box and its own
  scrollable, categorized grid.
- **Emoji tab**: a curated emoji dataset with keyword search. Grid of emoji; click to set.
- **Icons tab**:
  - An **Icon color** swatch row (fixed palette; first swatch = theme accent, selected
    by default). Selecting a color tints the chosen lucide icon.
  - A searchable grid of lucide icons; click to set (uses the currently-selected color).
- **Folder color footer** — shown **only when the target is a folder**: a swatch row
  (with a leading `∅` = none) that sets the optional left-bar accent. Independent of the
  icon's own color.
- **Remove** action (top-right): clears the custom icon, reverting to the default glyph.
  (For folders, the folder color is cleared by selecting `∅`.)

### 3.4 Palettes

- **Icon color** and **folder color** use the same fixed, theme-independent palette
  (so they render correctly in dark/light/nord): accent + grey/red/orange/green/blue/
  purple/pink. Stored as hex (or a palette key). Exact list finalized in the plan.

## 4. Data model & storage

A single persisted map, keyed by item path, holding per-item appearance:

```
type IconRef =
  | { kind: "emoji"; value: string }              // e.g. "📚"
  | { kind: "lucide"; name: string; color: string }; // name + hex/palette key

type TreeItemStyle = {
  icon?: IconRef;
  folderColor?: string;   // folders only; the left-bar accent
};

type TreeStyleMap = Record<string /* path */, TreeItemStyle>;
```

- Persisted in **localStorage** under a new key (e.g. `cairn.treeIcons`), in a new
  module `web/src/components/tree/treeIcons.ts`, mirroring the shape and defensive
  parse/try-catch of `treePersistence.ts` (`loadStyles()` / `saveStyles()`).
- Keys are note paths (`a/b.md`) and folder paths (`a/b`).
- **No engine involvement.** This is purely a frontend presentation layer.

### 4.1 Rename / move remap

Icons must follow their item across rename and move. `store.applyRenames(ops)` already
remaps open notes / tabs / active path from `{from, to}` note-path ops. The style map
remap hooks the **same** path: when renames are applied, remap style-map keys —

- **Note keys**: remapped directly from each `{from, to}` op.
- **Folder keys**: a folder rename/move emits ops for its descendant *notes*; derive the
  folder prefix change(s) from those ops and remap any folder-keyed (and nested-folder)
  style entries by prefix.

Exact remap derivation is an implementation detail for the plan, but it must be a pure,
unit-tested function (input: ops + current map → output: remapped map), kept in
`treeIcons.ts` and invoked from `applyRenames`. Deleting a note/folder drops its key(s).

## 5. New dependencies

The app currently uses **no icon library** (glyphs are inline unicode) and **no emoji
picker**. This feature introduces:

- **`lucide-react`** — the line-icon source for the Icons tab and the default
  folder/doc glyphs. Tree-shakeable; we expose a **curated subset** (not all ~1000
  icons) with name-based search. Default glyphs (`Folder`, `FileText`, etc.) come from it.
- **Emoji data** — a **curated static emoji list** with keyword tags shipped as a small
  module (no heavy `emoji-mart`-style dependency). Enough common emoji + search for v1;
  a fuller dataset can come later.

Both choices favor a light bundle. If the user prefers the full emoji set later,
swapping in a dataset package is isolated behind the Emoji tab.

## 6. Components & boundaries

New / changed units, each independently testable:

- **`treeIcons.ts`** (new, pure + persistence): types, `loadStyles`/`saveStyles`,
  and the rename/move `remapStyles(ops, map)` function. Pure logic unit-tested.
- **`IconPicker.tsx`** (new): the tabbed popover. Props: `target` (path + kind),
  current `style`, `onChange(style)`, `onClose`. Knows nothing about persistence or the
  tree — pure controlled component.
- **`EmojiGrid.tsx` / `IconGrid.tsx`** (new, or internal to IconPicker): each owns its
  search + grid for one source. Isolated so each is simple and testable.
- **`TreeItemIcon.tsx`** (new, small): given a `TreeItemStyle` + node kind, renders the
  correct leading glyph (custom or default). Used by `FolderTreeView`.
- **`FolderTreeView.tsx`** (changed): render the chevron/icon column, wire the icon
  slot to open the picker, draw the optional folder color bar. Reads the style map and
  an `onSetStyle(path, style)` callback from props (state lives in the store/persistence,
  passed down like `paths`/`onApplyRenames` today).
- **`store.ts`** (changed): own the `TreeStyleMap` (load on init, expose `setTreeStyle`,
  persist on change) and call `remapStyles` inside `applyRenames`. Wiring only.
- **`Sidebar.tsx`** (changed): pass the style map + `setTreeStyle` down to `FolderTree`.

## 7. Testing

Part of "done":

- `treeIcons.ts`: load/save round-trip; defensive parse of bad localStorage; `remapStyles`
  for note rename, folder rename, folder move, nested folders, and delete (key dropped).
- `IconPicker`: tab switching; emoji search filters; icon search filters; selecting an
  emoji vs. a colored lucide produces the right `IconRef`; Remove clears; folder-color
  footer present for folders and absent for notes.
- `TreeItemIcon`: renders custom emoji, custom lucide (with color), default folder glyph,
  default note glyph.
- `FolderTreeView`: clicking the icon slot opens the picker without triggering
  open/toggle; folder color bar renders when set. (Extends the existing
  `FolderTreeView.test.tsx` / `.dnd.test.tsx`.)
- `store`: setting a style persists; `applyRenames` remaps style keys.

## 8. Out of scope

- Frontmatter / engine-backed (portable) icons.
- Icon support anywhere outside the sidebar tree (tabs, breadcrumbs, graph nodes).
- Image / custom-upload icons (Notion's third tab).
- Per-note color bars (notes get icon + icon color only).
