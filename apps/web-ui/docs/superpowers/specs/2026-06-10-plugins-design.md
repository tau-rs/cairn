# Cairn Web UI — Plugins Design Spec

**Date:** 2026-06-10
**Status:** approved, ready for implementation planning
**Sub-project:** Surface the engine's plugin system — plugin-contributed commands in the
⌘K palette + a read-only Plugins panel in Settings.
**Builds on:** the synced contract (`ListPlugins`/`InvokePluginCommand`/`PluginSummary`),
the command palette + its `PaletteCommand` shape, the `loadTags` store pattern, the
`SettingsDialog` (Settings + KeyboardShortcuts sections), and `ErrorToast`.

---

## 1. Purpose

The engine exposes `ListPlugins` (loaded plugins + their commands) and
`InvokePluginCommand { plugin, command, args }` (runs a plugin command, may emit note
events, returns a `result` JSON), but the UI never exposes them. This loads plugins,
**flattens their commands into the ⌘K palette** (invoked with no args), surfaces the
result/errors briefly, and adds a **read-only Plugins panel** in Settings.

### Locked decisions (from brainstorming)

| Decision | Choice |
|---|---|
| Discovery | Store slice `plugins: PluginSummary[]`, loaded via `list_plugins` on init (like `loadTags`). Empty in a fresh real cairn (no host); the mock seeds a demo plugin for dev/e2e. |
| Palette | Each plugin command → a `PaletteCommand` in the existing "Commands" group, labeled `<Plugin name>: <Command title>`, id `plugin:<pluginId>/<commandId>`. |
| Invoke | Selecting one sends `invoke_plugin_command { plugin, command, args: null }` — **no args** (the contract declares no schema). |
| Side effects | Note changes the command makes emit `note_changed`/`note_deleted`, which the existing subscribe handler already uses to refresh list/graph/backlinks — automatic. |
| Result/errors | The returned `result` shows as a brief **notice toast** (a `string` result shown directly; otherwise `Ran <command>`). Failures use the existing `ErrorToast`. |
| Plugins panel | A read-only **Plugins** section in Settings (below keyboard shortcuts): per plugin, `name` + `version` and its commands' titles; "No plugins loaded" when empty. |
| Out of scope | Args input (no schema to drive it), a JSON result panel beyond the toast, installing/enabling plugins, plugin-contributed views. |

---

## 2. Architecture

A pure flatten/parse helper + a store slice + two small components + palette wiring.
**No engine/contract change** — `list_plugins`/`invoke_plugin_command` are synced and the
client bridge (`runQuery`/`sendCommand`) is generic.

```
web/src/components/plugins/pluginCommands.ts (+test)  NEW (pure) — toPaletteCommands + parsePluginCommandId.
web/src/client/mock.ts (+test)                        MODIFY — seed a demo plugin; list_plugins + invoke_plugin_command.
web/src/store/store.ts (+test)                        MODIFY — plugins + notice; loadPlugins / invokePlugin / dismissNotice.
web/src/components/plugins/PluginsPanel.tsx (+test)   NEW — read-only Settings list.
web/src/components/NoticeToast.tsx (+test)            NEW — info toast (mirrors ErrorToast).
web/src/components/SettingsDialog.tsx                 MODIFY — render <PluginsPanel>; new prop.
web/src/app/App.tsx                                   MODIFY — merge plugin commands into the palette; dispatch plugin ids; render NoticeToast.
web/e2e/skeleton.spec.ts                              MODIFY — invoke a plugin command + Plugins panel.
```

### 2.1 `pluginCommands.ts` (pure)

```ts
import type { PluginSummary } from "../../contract";
import type { PaletteCommand } from "../command-palette/CommandPalette";

// Each plugin command → {id:`plugin:${p.id}/${c.id}`, label:`${p.name}: ${c.title}`}.
export function toPaletteCommands(plugins: PluginSummary[]): PaletteCommand[];

// Parse a palette command id back to {plugin, command}; null if not a plugin id.
// Splits on the FIRST "/" after the "plugin:" prefix (command ids may contain "/").
export function parsePluginCommandId(
  id: string,
): { plugin: string; command: string } | null;
```

### 2.2 Mock

Add a `private plugins: PluginSummary[]` seeded in the constructor with one demo plugin:
`{ id:"demo", name:"Demo plugin", version:"1.0.0", commands:[{ id:"stamp", title:"Insert stamp note" }] }`.
- `runQuery` `list_plugins` → `{ type:"plugins", plugins: this.plugins }`.
- `sendCommand` `invoke_plugin_command`: for `demo`/`stamp`, `this.notes.set("stamp.md", "# Stamp\n")`,
  emit `note_changed`(stamp.md) + `reindexed`, return `{ type:"plugin_result", result:"stamp.md" }`;
  unknown plugin or command → throw `{ type:"invalid_request", message:"unknown plugin command" }`.

### 2.3 Store

Add state `plugins: PluginSummary[]` (init `[]`) and `notice: string | null` (init `null`).
Actions:
- `loadPlugins()` → `runQuery({type:"list_plugins"})` → on `{type:"plugins"}` set `plugins`. Called in `init()` after `loadTags()` (errors swallowed via `errMsg`).
- `invokePlugin(plugin, command)` → `sendCommand({type:"invoke_plugin_command", plugin, command, args: null})`; on `{type:"plugin_result", result}` set `notice = typeof result === "string" ? result : \`Ran ${command}\``; errors → `set({error: errMsg(err)})`. (Side effects refresh via the existing event subscription.)
- `dismissNotice()` → `set({ notice: null })`.
- `openCairn` reset adds `plugins: []`, `notice: null`.

`PluginSummary`/`PluginCommandSummary` are already barrel-exported? If not, add
`export type { PluginSummary } from "./PluginSummary";` (and `PluginCommandSummary`) to
`src/contract/index.ts`.

### 2.4 Components

- `NoticeToast.tsx`: props `{ message: string | null; onDismiss: () => void }`. Returns null when
  `message === null`; otherwise the same fixed bottom-right toast as `ErrorToast` but **info-styled**
  (`border-border bg-surface-2 text-text` instead of danger), with a `✕` `IconButton` (label "dismiss notice").
- `PluginsPanel.tsx`: props `{ plugins: PluginSummary[] }`. A `SectionLabel` "Plugins"; if empty, a
  muted "No plugins loaded"; else per plugin a row `name` + ` v<version>` (faint) and, indented, each
  command's `title`. Read-only (no buttons). Plain DOM.

### 2.5 App + SettingsDialog

- App: `const plugins = useCairn((s) => s.plugins); const notice = useCairn((s) => s.notice);`
- The palette command list appends plugin commands:
  `const COMMANDS = [...<existing static commands>, ...toPaletteCommands(plugins)];`
- `runCommand(id)`: at the top, `const p = parsePluginCommandId(id); if (p) { void actions.invokePlugin(p.plugin, p.command); setPaletteOpen(false); return; }` then the existing `switch`.
- Render `<NoticeToast message={notice} onDismiss={actions.dismissNotice} />` next to `<ErrorToast>`.
- `SettingsDialog` gains a `plugins: PluginSummary[]` prop and renders `<PluginsPanel plugins={plugins} />` below `<KeyboardShortcuts>` (with a divider); App passes `plugins={plugins}`.

---

## 3. Testing

- **Unit (Vitest):**
  - `pluginCommands`: `toPaletteCommands` flattens (id `plugin:demo/stamp`, label `Demo plugin: Insert stamp note`); `parsePluginCommandId` round-trips and returns `null` for non-plugin ids (e.g. `new-note`) and handles a command id containing `/`.
  - Mock: `list_plugins` returns the demo plugin; `invoke_plugin_command demo/stamp` writes `stamp.md` (emits) + returns `{type:"plugin_result", result:"stamp.md"}`; an unknown command → `invalid_request`.
  - Store: `loadPlugins` populates `plugins` (with the mock's demo); `invokePlugin("demo","stamp")` sets `notice` to `"stamp.md"` and the side-effect note appears in `notePaths` (via the event); `dismissNotice` clears it.
  - `PluginsPanel`: renders the plugin name/version + command title; empty → "No plugins loaded".
  - `NoticeToast`: null message → nothing; a message renders + dismiss fires.
- **e2e (Playwright):** open ⌘K, run **"Demo plugin: Insert stamp note"** → a `stamp` note appears in the tree (side effect) and a notice toast shows; open Settings → the **Plugins** panel lists "Demo plugin". Keep all existing e2e green.
- All existing unit + e2e stay green.

---

## 4. Files & dependencies

| File | Change |
|---|---|
| `web/src/components/plugins/pluginCommands.ts` (+test) | **New.** Flatten/parse. |
| `web/src/client/mock.ts` (+test) | **Modify.** Demo plugin + list_plugins/invoke. |
| `web/src/store/store.ts` (+test) | **Modify.** plugins/notice + loadPlugins/invokePlugin/dismissNotice. |
| `web/src/components/plugins/PluginsPanel.tsx` (+test) | **New.** Settings list. |
| `web/src/components/NoticeToast.tsx` (+test) | **New.** Info toast. |
| `web/src/contract/index.ts` | **Modify (maybe).** Barrel-export `PluginSummary`/`PluginCommandSummary` if absent. |
| `web/src/components/SettingsDialog.tsx` | **Modify.** Render PluginsPanel. |
| `web/src/app/App.tsx` | **Modify.** Palette merge + dispatch + NoticeToast. |
| `web/e2e/skeleton.spec.ts` | **Modify.** Plugin e2e. |

No new npm dependencies. No engine/contract change.

---

## 5. Risks

- **No args / no result schema.** `PluginCommandSummary` is `{id,title}` only. v1 invokes with
  `args: null` and shows the `result` as a toast string (or `Ran <command>`). Arg-taking commands and
  structured-result rendering are deferred — documented. Most palette commands are no-arg actions.
- **Empty in real cairn.** A fresh cairn has no plugin host → `list_plugins` returns `[]`, so the
  palette/panel show nothing (correct). The mock seeds a demo plugin so dev/e2e exercise the flow.
- **Side-effect refresh.** Invoking can change notes; the engine emits events the existing subscribe
  handler already consumes (notePaths/graph/backlinks/tags) — no new event wiring. The mock's invoke
  must `emit` the same events to mirror this (the e2e relies on the new note appearing).
- **Plugin-id parsing.** Command ids could contain `/`; `parsePluginCommandId` splits on the first
  `/` after the `plugin:` prefix so the plugin id is unambiguous and the command id keeps any `/`.
  The `plugin:` prefix can't collide with static command ids (none start with it).
- **Barrel export.** `src/contract/index.ts` is hand-written; `PluginSummary` (and
  `PluginCommandSummary`) must be exported there for `../contract` imports to resolve — add if missing.
- **Notice vs error toasts.** `NoticeToast` (info) and `ErrorToast` (danger) are separate, both
  bottom-right; a plugin error sets `error` (ErrorToast), a result sets `notice` (NoticeToast). They
  don't conflict (distinct state); stacking is acceptable.
- **Plain DOM, jsdom-safe.** `pluginCommands` is pure; the panel/toast are plain DOM.
