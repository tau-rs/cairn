# Cairn Web UI — Keyboard Shortcuts Design Spec

**Date:** 2026-06-10
**Status:** approved, ready for implementation planning
**Sub-project:** Phase 5 (Shell polish) — a real per-command keyboard-shortcut system.
**Builds on:** the ⌘K command palette (`command-palette/`), the existing ad-hoc
global keydown handler + `COMMANDS`/`runCommand` in `App.tsx`, the `SettingsDialog`,
and the localStorage persistence pattern (`tabsPersistence.ts` / `treePersistence.ts`).

---

## 1. Purpose

Replace the two disconnected keyboard mechanisms — an ad-hoc `window` keydown handler
(⌘K, ⌘W, ⌃Tab, ⌘1-9) and a separate palette `COMMANDS` list — with **one command
registry** (`{ id, label, defaultBinding }`) that drives both the palette (now showing
each command's shortcut inline) and the global dispatch. Bindings are **user-rebindable**
in Settings, with conflict detection and per-command reset, persisted to localStorage.

### Locked decisions (from brainstorming)

| Decision | Choice |
|---|---|
| Source of truth | One `COMMAND_DEFS` registry: `{ id, label, defaultBinding }`. Drives palette hints + global dispatch + the Settings list. |
| Rebindable commands (7) | Command palette `⌘K` · New note `⌘N` · Commit `⌘↵` · Toggle Graph/Editor `⌘⇧G` · Toggle Source/Live-preview `⌘E` · Open Settings `⌘,` · Close tab `⌘W`. |
| Built-in / fixed | Tab cycle `⌃Tab` / `⌃⇧Tab` and jump-to-tab `⌘1–9` stay hard-coded (parameterized); shown read-only in Settings, not rebindable. |
| "Mod" | `Mod` = ⌘ on macOS, Ctrl elsewhere (matches today's `metaKey \|\| ctrlKey`); one binding string, rendered per-platform. |
| Activation | Always active, **modifier-gated** (every binding needs Mod, ± Shift/Alt) — never collides with typing, works from inside the editor. On match: `preventDefault` + run. |
| Rebinding | In **Settings ▸ Keyboard shortcuts**: click **rebind** → "press keys…" capture; next modifier-bearing chord assigns (Esc cancels; bare keys rejected). |
| Conflicts | A chord already bound elsewhere is **blocked with an inline "already bound to <Command>" warning**; an explicit **Force** action unbinds the other command, then assigns. |
| Reset | Per-row **Reset** restores the default (clears the override). |
| Persistence | Only the **overrides** (`commandId → chord`, or `null` = explicitly unbound) persist to `localStorage` (`cairn.keybindings`). Defaults live in code; effective = override ?? default. |
| Out of scope | Rebinding the built-in tab cycle/jump; chords without a modifier; multi-key sequences (chords only); persisting the unrelated auto-commit Settings (separate concern). |

---

## 2. Architecture

Pure modules (registry, chord parsing/matching, persistence) + a Settings-section
component + thin App wiring. `runCommand`'s dispatch stays in `App` (it needs the React
setters). **No store/contract changes.**

```
web/src/components/shortcuts/keybinding.ts             NEW (pure) — eventToChord / isValidBinding / formatChord.
web/src/components/shortcuts/commands.ts               NEW (pure) — COMMAND_DEFS + effective/inverted maps + conflict.
web/src/components/shortcuts/keybindingPersistence.ts  NEW (pure) — overrides ↔ localStorage.
web/src/components/shortcuts/KeyboardShortcuts.tsx      NEW — Settings section: list + rebind capture + conflict + reset.
web/src/components/command-palette/CommandPalette.tsx   MODIFY — render an optional per-command shortcut hint.
web/src/components/SettingsDialog.tsx                   MODIFY — render <KeyboardShortcuts> below <Settings>; new props.
web/src/app/App.tsx                                     MODIFY — overrides state; registry-driven keydown; runCommand open-palette; pass hints/keybindings down.
(+ a .test beside each new/affected module)
```

### 2.1 `keybinding.ts` (pure)

```ts
// Canonical chord, e.g. "Mod+K" | "Mod+Shift+G" | "Mod+Enter" | "Mod+,". "Mod" is
// emitted when metaKey||ctrlKey. Letters upper-cased; named keys ("Enter","Tab",
// ",", "/"). Returns null when there's no non-modifier key.
export function eventToChord(e: KeyboardEvent): string | null;

// A bindable chord MUST include "Mod" (reject bare keys / Shift-only).
export function isValidBinding(chord: string): boolean;

// Display: "Mod+Shift+G" → "⌘⇧G" (mac) / "Ctrl+Shift+G" (other). Mod→⌘/Ctrl,
// Shift→⇧/Shift, Alt→⌥/Alt, "Enter"→↵.
export function formatChord(chord: string, isMac: boolean): string;
```

### 2.2 `commands.ts` (pure)

```ts
export interface CommandDef { id: string; label: string; defaultBinding: string | null }
export type Overrides = Record<string, string | null>; // id → chord, or null = explicitly unbound

export const COMMAND_DEFS: CommandDef[]; // the 7 rebindable commands, with defaults

export function effectiveBinding(id: string, overrides: Overrides): string | null; // override ?? default
export function chordToId(overrides: Overrides): Record<string, string>; // inverted, skips unbound — for matching
export function findConflict(overrides: Overrides, chord: string, exceptId: string): string | null; // id holding `chord`, if any
```

Defaults: `open-palette`→`Mod+K`, `new-note`→`Mod+N`, `commit`→`Mod+Enter`,
`toggle-view`→`Mod+Shift+G`, `toggle-editor-mode`→`Mod+E`, `open-settings`→`Mod+,`,
`close-tab`→`Mod+W`.

### 2.3 `keybindingPersistence.ts` (pure)

`loadOverrides(): Overrides` / `saveOverrides(o: Overrides): void` on `cairn.keybindings`
(guarded; mirrors the tabs/tree pattern). `null` values round-trip (explicit unbind).

### 2.4 `KeyboardShortcuts.tsx`

`props: { overrides: Overrides; onChange: (next: Overrides) => void }`.

- Computes `isMac` once (`navigator.platform`/`userAgent`). Renders one row per
  `COMMAND_DEF`: label + `formatChord(effectiveBinding(id), isMac)` (or "—" when unbound)
  + **rebind** + (if overridden) **Reset**.
- **Rebind capture:** clicking rebind puts the row in capture mode; a keydown handler
  reads `eventToChord`; `Esc` cancels; an invalid (no-Mod) chord is ignored with a hint;
  a valid chord runs `findConflict(overrides, chord, id)` → if none, assign
  (`onChange({ ...overrides, [id]: chord })`); if conflict, show "already bound to
  <label>" + a **Force** button that sets the other id to `null` and this id to `chord`.
- **Reset:** `onChange` with the id key removed (back to default).
- A read-only **Built-in** footer lists ⌃Tab / ⌃⇧Tab / ⌘1-9 (informational).

### 2.5 `CommandPalette.tsx` change

`PaletteCommand` gains an optional `hint?: string`. Each command row renders the hint
(monospace, right-aligned) when present. Notes have no hint. No behavior change.

### 2.6 `SettingsDialog.tsx` change

Add props `keybindingOverrides: Overrides` + `onKeybindingsChange: (o: Overrides) => void`;
render `<KeyboardShortcuts overrides={…} onChange={…} />` below `<Settings/>` (a divider
between). `Settings.tsx` is unchanged.

### 2.7 `App.tsx` wiring

- `const [overrides, setOverrides] = useState<Overrides>(() => loadOverrides());`
- `const chordMap = useMemo(() => chordToId(overrides), [overrides]);`
- A `runCommandRef` (`useRef`) holds the latest `runCommand` (assigned each render) so the
  keydown effect never goes stale without re-subscribing on every state change.
- **Global keydown effect** (deps `[chordMap]`): `const id = chordMap[eventToChord(e) ?? ""]`
  → if set, `e.preventDefault(); runCommandRef.current(id)`. Then the built-in branch:
  `⌃Tab`→`cycleTab(±1)`, `⌘1-9`→`jumpToTab(n)` (unchanged).
- `runCommand` gains `case "open-palette": setPaletteOpen((o) => !o); return;` (the early
  `return` skips the trailing `setPaletteOpen(false)` so the toggle isn't immediately closed).
- The palette's `commands` = `COMMAND_DEFS` **minus `open-palette`** (you don't open the
  palette from the palette), each mapped to `{ id, label, hint: formatChord(effectiveBinding(id, overrides), isMac) }`.
- SettingsDialog gets `keybindingOverrides={overrides}` + `onKeybindingsChange={(o) => { setOverrides(o); saveOverrides(o); }}`.

---

## 3. Testing

- **Unit (Vitest):**
  - `keybinding`: `eventToChord` for Mod+letter, Mod+Shift+letter, Mod+Enter, Mod+",",
    and `null`/no-Mod cases; `isValidBinding` rejects bare keys; `formatChord` mac vs non-mac.
  - `commands`: `effectiveBinding` (override beats default, `null` unbinds); `chordToId`
    inverts + skips unbound + reflects overrides; `findConflict` finds the holder and
    ignores `exceptId`.
  - `keybindingPersistence`: round-trips overrides incl. `null`; empty/malformed → `{}`.
  - `KeyboardShortcuts` (Testing Library): renders default bindings; rebind capture sets a
    new chord (fire a keydown); an invalid bare key is rejected; binding to a used chord
    shows the conflict warning and does NOT assign; **Force** unbinds the other and assigns;
    **Reset** restores the default.
  - `CommandPalette`: a command with `hint` renders it.
- **e2e (Playwright):** open Settings → rebind **Open Settings** to a free chord
  (e.g. `Control+Shift+S`) → close the dialog → press that chord → the Settings dialog
  reopens (a clean observable). Reload → reopen Settings → the row shows the new chord
  (override persisted). Keep existing e2e green (the built-in ⌘K/⌘W/⌃Tab/⌘1-9 still work
  via the registry + built-ins).
- All existing unit + e2e stay green; Tauri unaffected.

---

## 4. Files & dependencies

| File | Change |
|---|---|
| `web/src/components/shortcuts/keybinding.ts` (+test) | **New.** Chord parse/format/validate. |
| `web/src/components/shortcuts/commands.ts` (+test) | **New.** Registry + maps + conflict. |
| `web/src/components/shortcuts/keybindingPersistence.ts` (+test) | **New.** Overrides localStorage. |
| `web/src/components/shortcuts/KeyboardShortcuts.tsx` (+test) | **New.** Settings section. |
| `web/src/components/command-palette/CommandPalette.tsx` (+test) | **Modify.** Per-command hint. |
| `web/src/components/SettingsDialog.tsx` | **Modify.** Render KeyboardShortcuts + props. |
| `web/src/app/App.tsx` | **Modify.** Overrides state + registry keydown + open-palette + hints. |
| `web/e2e/skeleton.spec.ts` | **Modify.** Rebind e2e. |

No new npm dependencies. No store/contract changes. Reuses the localStorage pattern.

---

## 5. Risks

- **Stale closure in the keydown effect.** `runCommand` closes over React state
  (`editorMode`, `view`, setters). The effect must call it via a `useRef` updated each
  render (not capture it directly with `[]` deps), or it will dispatch with stale state.
  Mitigation: `runCommandRef.current = runCommand` each render; effect calls `.current`.
- **open-palette self-close.** `runCommand` ends with `setPaletteOpen(false)`; the
  `open-palette` case must `return` before that so ⌘K actually toggles open. It's excluded
  from the palette's own command list.
- **Browser-reserved chords.** ⌘N/⌘W (and others) are reserved in a plain browser;
  `preventDefault` reclaims them where the browser allows, and Cairn is Tauri-first (the
  app owns accelerators). The Settings list lets the user rebind anything that fights the host.
- **Conflict + Force correctness.** Force must unbind exactly the conflicting command
  (set its override to `null`) and assign the new one atomically (single `onChange`), so the
  map can't transiently hold a duplicate.
- **Mod = meta OR ctrl.** On macOS both ⌘K and Ctrl+K map to `Mod+K` (as today). Accepted —
  keeps one cross-platform binding; document it.
- **Capture-mode key leakage.** While a row is capturing, its keydown handler must
  `preventDefault`/`stopPropagation` so the captured chord doesn't also fire the global
  dispatch (e.g. capturing ⌘K shouldn't toggle the palette). Mitigation: capture handler
  swallows the event.
- **Plain DOM, jsdom-safe.** All new UI is plain DOM — fully unit-testable.
```
