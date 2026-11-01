# Keyboard Shortcuts Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A per-command keyboard-shortcut system: one command registry drives the ⌘K palette hints + the global keydown dispatch, with user-rebindable bindings (conflict detection, Force-override, reset) persisted to localStorage.

**Architecture:** Pure modules — `keybinding` (chord parse/format/validate), `commands` (registry + effective/inverted maps + conflict), `keybindingPersistence` (overrides ↔ localStorage) — plus a `KeyboardShortcuts` Settings section and thin App wiring. Dispatch (`runCommand`) stays in App; the keydown effect calls it through a `useRef` to avoid stale closures.

**Tech Stack:** React 18 + TypeScript, Tailwind, Vitest + Testing Library, Playwright.

**Spec:** `docs/superpowers/specs/2026-06-10-keyboard-shortcuts-design.md`

**Working conventions (read before starting):**
- Run all `pnpm` from `web/`. Git from repo root.
- Per-task gate before commit: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check`. Run `pnpm build` + `pnpm e2e` where a task says so. Run `pnpm format` + re-stage if `format:check` flags files. Ignore stale LSP "cannot find module" noise — trust `pnpm typecheck`'s exit code.
- e2e on port 5273 (`--strictPort`). Baseline: 220 unit, 12 e2e green.
- **Relevant existing code:**
  - `App.tsx` has the global keydown `useEffect` (⌘K/⌘W/⌃Tab/⌘1-9), `COMMANDS: PaletteCommand[]`, and `runCommand(id)` switch (cases: new-note, commit, close-tab, toggle-view, open-settings, toggle-editor-mode), ending with `setPaletteOpen(false)`. `actions = cairnStore.getState()`. The `SettingsDialog` is rendered with `{open, onOpenChange, settings, onChange}`.
  - `CommandPalette` exports `interface PaletteCommand { id; label }` and renders command `Row`s + note `Row`s.
  - `SettingsDialog.tsx` renders `<Settings settings onChange/>` + a Done button.
  - Persistence pattern: `tabsPersistence.ts` (guarded localStorage; jsdom localStorage works via `vitest.setup.ts`).
  - Tailwind tokens: `surface`, `surface-2`, `border`, `text`, `muted`, `faint`, `accent`, `danger`.
  - jsdom's `navigator` is non-mac (no "mac" in platform/UA), so `formatChord(..., isMac=false)` → `Ctrl+…` in unit tests. The Playwright e2e runs in a real browser (macOS dev machine → `⌘…`).

---

## File Structure

| File | Responsibility |
|---|---|
| `web/src/components/shortcuts/keybinding.ts` | Pure: `eventToChord` / `isValidBinding` / `formatChord`. |
| `web/src/components/shortcuts/commands.ts` | Pure: `COMMAND_DEFS` + `effectiveBinding` / `chordToId` / `findConflict`. |
| `web/src/components/shortcuts/keybindingPersistence.ts` | Pure: overrides ↔ localStorage. |
| `web/src/components/shortcuts/KeyboardShortcuts.tsx` | Settings section: list + rebind capture + conflict + reset. |
| `web/src/components/command-palette/CommandPalette.tsx` | Add optional per-command `hint`. |
| `web/src/components/SettingsDialog.tsx` | Render `<KeyboardShortcuts>` + new props. |
| `web/src/app/App.tsx` | Overrides state; registry-driven keydown; `open-palette`; palette hints. |
| `web/e2e/skeleton.spec.ts` | Rebind e2e. |

---

## Task 1: keybinding (pure)

**Files:**
- Create: `web/src/components/shortcuts/keybinding.ts`
- Create: `web/src/components/shortcuts/keybinding.test.ts`

- [ ] **Step 1: Write the failing tests**

Create `web/src/components/shortcuts/keybinding.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import { eventToChord, isValidBinding, formatChord } from "./keybinding";

const ev = (o: Partial<KeyboardEvent>) => o as KeyboardEvent;

describe("eventToChord", () => {
  it("builds Mod+letter from meta or ctrl", () => {
    expect(eventToChord(ev({ key: "k", metaKey: true }))).toBe("Mod+K");
    expect(eventToChord(ev({ key: "k", ctrlKey: true }))).toBe("Mod+K");
  });
  it("includes Shift / Alt and named keys", () => {
    expect(eventToChord(ev({ key: "g", metaKey: true, shiftKey: true }))).toBe(
      "Mod+Shift+G",
    );
    expect(eventToChord(ev({ key: "Enter", metaKey: true }))).toBe("Mod+Enter");
    expect(eventToChord(ev({ key: ",", metaKey: true }))).toBe("Mod+,");
  });
  it("returns null for a pure modifier press", () => {
    expect(eventToChord(ev({ key: "Meta", metaKey: true }))).toBeNull();
    expect(eventToChord(ev({ key: "Shift", shiftKey: true }))).toBeNull();
  });
});

describe("isValidBinding", () => {
  it("requires the Mod modifier", () => {
    expect(isValidBinding("Mod+K")).toBe(true);
    expect(isValidBinding("Mod+Shift+G")).toBe(true);
    expect(isValidBinding("K")).toBe(false);
    expect(isValidBinding("Shift+K")).toBe(false);
  });
});

describe("formatChord", () => {
  it("renders mac glyphs", () => {
    expect(formatChord("Mod+Shift+G", true)).toBe("⌘⇧G");
    expect(formatChord("Mod+Enter", true)).toBe("⌘↵");
    expect(formatChord("Mod+,", true)).toBe("⌘,");
  });
  it("renders non-mac text", () => {
    expect(formatChord("Mod+K", false)).toBe("Ctrl+K");
    expect(formatChord("Mod+Shift+G", false)).toBe("Ctrl+Shift+G");
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `pnpm test -- shortcuts/keybinding` — expect FAIL (module not found).

- [ ] **Step 3: Implement `keybinding.ts`**

Create `web/src/components/shortcuts/keybinding.ts`:

```ts
/** Canonical chord from a KeyboardEvent, e.g. "Mod+Shift+G". "Mod" = meta||ctrl.
 *  Returns null for a pure modifier press (no real key). */
export function eventToChord(e: KeyboardEvent): string | null {
  const key = e.key;
  if (key === "Meta" || key === "Control" || key === "Shift" || key === "Alt") {
    return null;
  }
  const parts: string[] = [];
  if (e.metaKey || e.ctrlKey) parts.push("Mod");
  if (e.shiftKey) parts.push("Shift");
  if (e.altKey) parts.push("Alt");
  parts.push(normalizeKey(key));
  return parts.join("+");
}

function normalizeKey(key: string): string {
  if (key === " ") return "Space";
  return key.length === 1 ? key.toUpperCase() : key; // "Enter","Tab","," stay as-is
}

/** A bindable chord must include the Mod modifier (reject bare keys / Shift-only). */
export function isValidBinding(chord: string): boolean {
  const parts = chord.split("+");
  return parts.includes("Mod") && parts[parts.length - 1] !== "Mod";
}

/** Display form: "Mod+Shift+G" → "⌘⇧G" (mac) / "Ctrl+Shift+G" (other). */
export function formatChord(chord: string, isMac: boolean): string {
  return chord
    .split("+")
    .map((p) => {
      if (p === "Mod") return isMac ? "⌘" : "Ctrl+";
      if (p === "Shift") return isMac ? "⇧" : "Shift+";
      if (p === "Alt") return isMac ? "⌥" : "Alt+";
      if (p === "Enter") return "↵";
      return p;
    })
    .join("");
}
```

- [ ] **Step 4: Run to verify pass**

Run: `pnpm test -- shortcuts/keybinding` — expect PASS.

- [ ] **Step 5: Per-task gate**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check` — all PASS.

- [ ] **Step 6: Commit**

```bash
git add web/src/components/shortcuts/keybinding.ts web/src/components/shortcuts/keybinding.test.ts
git commit -m "feat(shortcuts): pure chord parse/validate/format"
```

---

## Task 2: commands registry (pure)

**Files:**
- Create: `web/src/components/shortcuts/commands.ts`
- Create: `web/src/components/shortcuts/commands.test.ts`

- [ ] **Step 1: Write the failing tests**

Create `web/src/components/shortcuts/commands.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import {
  COMMAND_DEFS,
  effectiveBinding,
  chordToId,
  findConflict,
} from "./commands";

describe("COMMAND_DEFS", () => {
  it("includes the 7 commands with unique default chords", () => {
    expect(COMMAND_DEFS).toHaveLength(7);
    const chords = COMMAND_DEFS.map((c) => c.defaultBinding);
    expect(new Set(chords).size).toBe(chords.length); // unique
  });
});

describe("effectiveBinding", () => {
  it("returns the default with no override", () => {
    expect(effectiveBinding("new-note", {})).toBe("Mod+N");
  });
  it("an override beats the default; null unbinds", () => {
    expect(effectiveBinding("new-note", { "new-note": "Mod+Shift+N" })).toBe(
      "Mod+Shift+N",
    );
    expect(effectiveBinding("new-note", { "new-note": null })).toBeNull();
  });
});

describe("chordToId", () => {
  it("inverts effective bindings and skips unbound", () => {
    const m = chordToId({ "close-tab": null });
    expect(m["Mod+K"]).toBe("open-palette");
    expect(m["Mod+W"]).toBeUndefined();
  });
  it("reflects an override", () => {
    const m = chordToId({ "new-note": "Mod+J" });
    expect(m["Mod+J"]).toBe("new-note");
    expect(m["Mod+N"]).toBeUndefined();
  });
});

describe("findConflict", () => {
  it("finds the command holding a chord, ignoring exceptId", () => {
    expect(findConflict({}, "Mod+K", "new-note")).toBe("open-palette");
    expect(findConflict({}, "Mod+K", "open-palette")).toBeNull();
    expect(findConflict({}, "Mod+Shift+Z", "new-note")).toBeNull();
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `pnpm test -- shortcuts/commands` — expect FAIL (module not found).

- [ ] **Step 3: Implement `commands.ts`**

Create `web/src/components/shortcuts/commands.ts`:

```ts
export interface CommandDef {
  id: string;
  label: string;
  defaultBinding: string | null;
}

export type Overrides = Record<string, string | null>; // id → chord, or null = unbound

export const COMMAND_DEFS: CommandDef[] = [
  { id: "open-palette", label: "Command palette", defaultBinding: "Mod+K" },
  { id: "new-note", label: "New note", defaultBinding: "Mod+N" },
  { id: "commit", label: "Commit changes…", defaultBinding: "Mod+Enter" },
  {
    id: "toggle-view",
    label: "Toggle Graph / Editor",
    defaultBinding: "Mod+Shift+G",
  },
  {
    id: "toggle-editor-mode",
    label: "Toggle Source / Live preview",
    defaultBinding: "Mod+E",
  },
  { id: "open-settings", label: "Open Settings", defaultBinding: "Mod+," },
  { id: "close-tab", label: "Close tab", defaultBinding: "Mod+W" },
];

const DEFAULT_BY_ID: Record<string, string | null> = Object.fromEntries(
  COMMAND_DEFS.map((c) => [c.id, c.defaultBinding]),
);

/** Override if present (incl. null = unbound), else the default. */
export function effectiveBinding(
  id: string,
  overrides: Overrides,
): string | null {
  return id in overrides ? overrides[id] : (DEFAULT_BY_ID[id] ?? null);
}

/** Invert effective bindings → { chord: id }, skipping unbound commands. */
export function chordToId(overrides: Overrides): Record<string, string> {
  const map: Record<string, string> = {};
  for (const def of COMMAND_DEFS) {
    const chord = effectiveBinding(def.id, overrides);
    if (chord) map[chord] = def.id;
  }
  return map;
}

/** The command currently bound to `chord` (other than `exceptId`), or null. */
export function findConflict(
  overrides: Overrides,
  chord: string,
  exceptId: string,
): string | null {
  for (const def of COMMAND_DEFS) {
    if (def.id === exceptId) continue;
    if (effectiveBinding(def.id, overrides) === chord) return def.id;
  }
  return null;
}
```

- [ ] **Step 4: Run to verify pass**

Run: `pnpm test -- shortcuts/commands` — expect PASS.

- [ ] **Step 5: Per-task gate**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check` — all PASS.

- [ ] **Step 6: Commit**

```bash
git add web/src/components/shortcuts/commands.ts web/src/components/shortcuts/commands.test.ts
git commit -m "feat(shortcuts): command registry + effective/inverted maps + conflict"
```

---

## Task 3: keybindingPersistence (pure)

**Files:**
- Create: `web/src/components/shortcuts/keybindingPersistence.ts`
- Create: `web/src/components/shortcuts/keybindingPersistence.test.ts`

- [ ] **Step 1: Write the failing tests**

Create `web/src/components/shortcuts/keybindingPersistence.test.ts`:

```ts
import { describe, it, expect, beforeEach } from "vitest";
import { loadOverrides, saveOverrides } from "./keybindingPersistence";

beforeEach(() => localStorage.clear());

describe("keybindingPersistence", () => {
  it("round-trips overrides including explicit null (unbound)", () => {
    saveOverrides({ "new-note": "Mod+J", "close-tab": null });
    expect(loadOverrides()).toEqual({ "new-note": "Mod+J", "close-tab": null });
  });
  it("returns {} when nothing stored", () => {
    expect(loadOverrides()).toEqual({});
  });
  it("returns {} on malformed storage", () => {
    localStorage.setItem("cairn.keybindings", "{not json");
    expect(loadOverrides()).toEqual({});
  });
  it("drops non-string / non-null values", () => {
    localStorage.setItem(
      "cairn.keybindings",
      JSON.stringify({ a: "Mod+A", b: 5, c: null }),
    );
    expect(loadOverrides()).toEqual({ a: "Mod+A", c: null });
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `pnpm test -- shortcuts/keybindingPersistence` — expect FAIL (module not found).

- [ ] **Step 3: Implement `keybindingPersistence.ts`**

Create `web/src/components/shortcuts/keybindingPersistence.ts`:

```ts
import type { Overrides } from "./commands";

const STORAGE_KEY = "cairn.keybindings";

export function loadOverrides(): Overrides {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as unknown;
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
      return {};
    }
    const out: Overrides = {};
    for (const [k, v] of Object.entries(parsed as Record<string, unknown>)) {
      if (v === null || typeof v === "string") out[k] = v;
    }
    return out;
  } catch {
    return {};
  }
}

export function saveOverrides(o: Overrides): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(o));
  } catch {
    // ignore (private mode / quota)
  }
}
```

- [ ] **Step 4: Run to verify pass**

Run: `pnpm test -- shortcuts/keybindingPersistence` — expect PASS.

- [ ] **Step 5: Per-task gate**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check` — all PASS.

- [ ] **Step 6: Commit**

```bash
git add web/src/components/shortcuts/keybindingPersistence.ts web/src/components/shortcuts/keybindingPersistence.test.ts
git commit -m "feat(shortcuts): keybinding overrides localStorage persistence"
```

---

## Task 4: CommandPalette per-command hint

**Files:**
- Modify: `web/src/components/command-palette/CommandPalette.tsx`
- Modify: `web/src/components/command-palette/CommandPalette.test.tsx`

- [ ] **Step 1: Add a failing test**

Append this test inside the `describe("CommandPalette", …)` block in `web/src/components/command-palette/CommandPalette.test.tsx`:

```tsx
  it("renders a command's shortcut hint", () => {
    render(
      <CommandPalette
        open
        onClose={vi.fn()}
        commands={[{ id: "commit", label: "Commit changes", hint: "⌘↵" }]}
        notes={[]}
        onRunCommand={vi.fn()}
        onOpenNote={vi.fn()}
      />,
    );
    expect(screen.getByText("⌘↵")).toBeInTheDocument();
  });
```

- [ ] **Step 2: Run to verify failure**

Run: `pnpm test -- CommandPalette` — expect FAIL (hint not rendered; also a TS error that `hint` isn't on `PaletteCommand`).

- [ ] **Step 3: Implement**

In `web/src/components/command-palette/CommandPalette.tsx`:

(a) Extend the interface:
```tsx
export interface PaletteCommand {
  id: string;
  label: string;
  hint?: string;
}
```

(b) Extend the command `Item` variant:
```tsx
type Item =
  | { kind: "command"; id: string; label: string; hint?: string }
  | { kind: "note"; id: string; label: string; path: string };
```

(c) Carry the hint when building command items — change the `cmdItems` map:
```tsx
    const cmdItems: Item[] = props.commands.map((c) => ({
      kind: "command",
      id: c.id,
      label: c.label,
      hint: c.hint,
    }));
```

(d) Render the hint in the command `Row` — change the command list's `Row` body:
```tsx
                  <Row
                    key={item.id}
                    selected={results[index] === item}
                    onClick={() => run(item)}
                  >
                    {item.label}
                    <span className="ml-auto font-mono text-[11px] text-faint">
                      {item.kind === "command" ? (item.hint ?? "") : ""}
                    </span>
                  </Row>
```

- [ ] **Step 4: Run to verify pass**

Run: `pnpm test -- CommandPalette` — expect PASS.

- [ ] **Step 5: Per-task gate**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check` — all PASS.

- [ ] **Step 6: Commit**

```bash
git add web/src/components/command-palette/CommandPalette.tsx web/src/components/command-palette/CommandPalette.test.tsx
git commit -m "feat(shortcuts): render per-command shortcut hint in the palette"
```

---

## Task 5: KeyboardShortcuts settings section

**Files:**
- Create: `web/src/components/shortcuts/KeyboardShortcuts.tsx`
- Create: `web/src/components/shortcuts/KeyboardShortcuts.test.tsx`

- [ ] **Step 1: Write the failing tests**

Create `web/src/components/shortcuts/KeyboardShortcuts.test.tsx`:

```tsx
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { KeyboardShortcuts } from "./KeyboardShortcuts";

beforeEach(() => localStorage.clear());

function setup(overrides = {}) {
  const onChange = vi.fn();
  render(<KeyboardShortcuts overrides={overrides} onChange={onChange} />);
  return { onChange };
}

describe("KeyboardShortcuts", () => {
  it("renders each command's effective binding", () => {
    setup();
    // jsdom is non-mac → "Ctrl+N"
    expect(
      screen.getByRole("button", { name: "rebind New note" }),
    ).toHaveTextContent("Ctrl+N");
  });
  it("captures a new modifier-bearing chord", () => {
    const { onChange } = setup();
    fireEvent.click(screen.getByRole("button", { name: "rebind New note" }));
    const input = screen.getByLabelText("press keys for New note");
    fireEvent.keyDown(input, { key: "j", ctrlKey: true });
    expect(onChange).toHaveBeenCalledWith({ "new-note": "Mod+J" });
  });
  it("ignores a bare key during capture", () => {
    const { onChange } = setup();
    fireEvent.click(screen.getByRole("button", { name: "rebind New note" }));
    fireEvent.keyDown(screen.getByLabelText("press keys for New note"), {
      key: "j",
    });
    expect(onChange).not.toHaveBeenCalled();
  });
  it("warns on a conflict and Force unbinds the other command", () => {
    const { onChange } = setup();
    fireEvent.click(screen.getByRole("button", { name: "rebind New note" }));
    fireEvent.keyDown(screen.getByLabelText("press keys for New note"), {
      key: "w",
      ctrlKey: true,
    }); // Mod+W = Close tab's default
    expect(onChange).not.toHaveBeenCalled();
    expect(screen.getByText(/already bound to Close tab/i)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /force/i }));
    expect(onChange).toHaveBeenCalledWith({
      "close-tab": null,
      "new-note": "Mod+W",
    });
  });
  it("resets an overridden binding", () => {
    const { onChange } = setup({ "new-note": "Mod+J" });
    fireEvent.click(screen.getByRole("button", { name: "reset New note" }));
    expect(onChange).toHaveBeenCalledWith({});
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `pnpm test -- KeyboardShortcuts` — expect FAIL (module not found).

- [ ] **Step 3: Implement `KeyboardShortcuts.tsx`**

Create `web/src/components/shortcuts/KeyboardShortcuts.tsx`:

```tsx
import { useState } from "react";
import { SectionLabel } from "../ui/SectionLabel";
import {
  COMMAND_DEFS,
  effectiveBinding,
  findConflict,
  type Overrides,
} from "./commands";
import { eventToChord, isValidBinding, formatChord } from "./keybinding";

const IS_MAC =
  typeof navigator !== "undefined" &&
  /mac/i.test(navigator.platform || navigator.userAgent || "");

export function KeyboardShortcuts(props: {
  overrides: Overrides;
  onChange: (next: Overrides) => void;
}) {
  const [capturingId, setCapturingId] = useState<string | null>(null);
  const [conflict, setConflict] = useState<{
    chord: string;
    otherId: string;
  } | null>(null);

  const labelOf = (id: string) =>
    COMMAND_DEFS.find((c) => c.id === id)?.label ?? id;

  const assign = (id: string, chord: string, alsoUnbind?: string) => {
    const next: Overrides = { ...props.overrides };
    if (alsoUnbind) next[alsoUnbind] = null;
    next[id] = chord;
    props.onChange(next);
    setCapturingId(null);
    setConflict(null);
  };

  const reset = (id: string) => {
    const next = { ...props.overrides };
    delete next[id];
    props.onChange(next);
  };

  const onCaptureKeyDown = (id: string, e: React.KeyboardEvent) => {
    e.preventDefault();
    e.stopPropagation(); // don't let the captured chord also fire the global dispatch
    if (e.key === "Escape") {
      setCapturingId(null);
      setConflict(null);
      return;
    }
    const chord = eventToChord(e.nativeEvent);
    if (!chord || !isValidBinding(chord)) return; // wait for a valid modifier chord
    const other = findConflict(props.overrides, chord, id);
    if (other) {
      setConflict({ chord, otherId: other });
      return;
    }
    assign(id, chord);
  };

  return (
    <div className="flex flex-col gap-1 text-sm text-text">
      <span className="mb-1">
        <SectionLabel>Keyboard shortcuts</SectionLabel>
      </span>
      {COMMAND_DEFS.map((def) => {
        const binding = effectiveBinding(def.id, props.overrides);
        const overridden = def.id in props.overrides;
        const capturing = capturingId === def.id;
        return (
          <div key={def.id}>
            <div className="flex items-center justify-between gap-3 py-0.5">
              <span className="text-muted">{def.label}</span>
              <div className="flex items-center gap-2">
                {capturing ? (
                  <input
                    autoFocus
                    readOnly
                    aria-label={`press keys for ${def.label}`}
                    className="w-32 rounded border border-accent bg-surface-2 px-2 py-0.5 text-center text-[11px] text-text outline-none"
                    value="press keys…"
                    onKeyDown={(e) => onCaptureKeyDown(def.id, e)}
                    onBlur={() => {
                      setCapturingId(null);
                      setConflict(null);
                    }}
                  />
                ) : (
                  <button
                    type="button"
                    aria-label={`rebind ${def.label}`}
                    className="min-w-[3rem] rounded border border-border bg-surface-2 px-2 py-0.5 font-mono text-[11px] text-text hover:border-accent"
                    onClick={() => {
                      setConflict(null);
                      setCapturingId(def.id);
                    }}
                  >
                    {binding ? formatChord(binding, IS_MAC) : "—"}
                  </button>
                )}
                {overridden && (
                  <button
                    type="button"
                    aria-label={`reset ${def.label}`}
                    className="text-faint hover:text-text"
                    onClick={() => reset(def.id)}
                  >
                    ↺
                  </button>
                )}
              </div>
            </div>
            {capturing && conflict && (
              <div className="pb-1 text-right text-[11px] text-danger">
                already bound to {labelOf(conflict.otherId)} ·{" "}
                <button
                  type="button"
                  className="underline hover:text-text"
                  onClick={() => assign(def.id, conflict.chord, conflict.otherId)}
                >
                  Force
                </button>
              </div>
            )}
          </div>
        );
      })}
      <p className="mt-2 text-[11px] text-faint">
        Built-in: ⌃Tab / ⌃⇧Tab cycle tabs · ⌘1–9 jump to tab.
      </p>
    </div>
  );
}
```

- [ ] **Step 4: Run to verify pass**

Run: `pnpm test -- KeyboardShortcuts` — expect PASS (5 tests). If `IS_MAC` is true in this environment's jsdom (rare), the first test's `Ctrl+N` expectation would fail — confirm `navigator.platform`/`userAgent` has no "mac"; the repo's CI jsdom is non-mac, so this holds.

- [ ] **Step 5: Per-task gate**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check` — all PASS.

- [ ] **Step 6: Commit**

```bash
git add web/src/components/shortcuts/KeyboardShortcuts.tsx web/src/components/shortcuts/KeyboardShortcuts.test.tsx
git commit -m "feat(shortcuts): KeyboardShortcuts settings section (rebind + conflict + reset)"
```

---

## Task 6: Wire into App + SettingsDialog + e2e

**Files:**
- Modify: `web/src/components/SettingsDialog.tsx`
- Modify: `web/src/app/App.tsx`
- Modify: `web/e2e/skeleton.spec.ts`

- [ ] **Step 1: SettingsDialog — render KeyboardShortcuts**

Replace the ENTIRE contents of `web/src/components/SettingsDialog.tsx` with:

```tsx
import { Modal } from "./ui/Modal";
import { Button } from "./ui/Button";
import { Settings } from "./Settings";
import { KeyboardShortcuts } from "./shortcuts/KeyboardShortcuts";
import type { Overrides } from "./shortcuts/commands";
import type { Settings as SettingsType } from "../store/store";

export function SettingsDialog({
  open,
  onOpenChange,
  settings,
  onChange,
  keybindingOverrides,
  onKeybindingsChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  settings: SettingsType;
  onChange: (patch: Partial<SettingsType>) => void;
  keybindingOverrides: Overrides;
  onKeybindingsChange: (o: Overrides) => void;
}) {
  return (
    <Modal open={open} onClose={() => onOpenChange(false)} title="Settings">
      <Settings settings={settings} onChange={onChange} />
      <div className="my-3 border-t border-border" />
      <KeyboardShortcuts
        overrides={keybindingOverrides}
        onChange={onKeybindingsChange}
      />
      <div className="mt-3 flex justify-end">
        <Button variant="secondary" onClick={() => onOpenChange(false)}>
          Done
        </Button>
      </div>
    </Modal>
  );
}
```

- [ ] **Step 2: App — imports + overrides state + isMac**

In `web/src/app/App.tsx`:

(a) Add imports near the other imports. The first line of `App.tsx` is currently
`import { useEffect, useState } from "react";` — change it to include `useRef` and
`useMemo`, and add the shortcut-module imports:
```tsx
import { useEffect, useState, useRef, useMemo } from "react";
```
```tsx
import {
  COMMAND_DEFS,
  effectiveBinding,
  chordToId,
  type Overrides,
} from "../components/shortcuts/commands";
import { eventToChord, formatChord } from "../components/shortcuts/keybinding";
import {
  loadOverrides,
  saveOverrides,
} from "../components/shortcuts/keybindingPersistence";
```

(b) Add a module-level constant above `export default function App()`:
```tsx
const IS_MAC =
  typeof navigator !== "undefined" &&
  /mac/i.test(navigator.platform || navigator.userAgent || "");
```

(c) Add state + derived maps + a dispatch ref **immediately after the first `useEffect`
(the `init` one) and BEFORE the keydown `useEffect`** — this ordering matters: the
keydown effect's `[chordMap]` deps array references `chordMap`, so `chordMap` must be
declared above it (otherwise a temporal-dead-zone ReferenceError at render):
```tsx
  const [overrides, setOverrides] = useState<Overrides>(() => loadOverrides());
  const chordMap = useMemo(() => chordToId(overrides), [overrides]);
  const runCommandRef = useRef<(id: string) => void>(() => {});
```

- [ ] **Step 3: App — replace the global keydown effect (in place, now below the declarations from Step 2c)**

Replace the existing keydown `useEffect` (the ⌘K/⌘W/⌃Tab/⌘1-9 one) with:
```tsx
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const chord = eventToChord(e);
      const id = chord ? chordMap[chord] : undefined;
      if (id) {
        e.preventDefault();
        runCommandRef.current(id);
        return;
      }
      // Built-in tab navigation (parameterized; not rebindable).
      const st = cairnStore.getState();
      if (e.ctrlKey && e.key === "Tab") {
        e.preventDefault();
        st.cycleTab(e.shiftKey ? -1 : 1);
      } else if ((e.metaKey || e.ctrlKey) && /^[1-9]$/.test(e.key)) {
        e.preventDefault();
        st.jumpToTab(Number(e.key));
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [chordMap]);
```

- [ ] **Step 4: App — registry-driven COMMANDS + open-palette + ref**

(a) Replace the hard-coded `COMMANDS` array with one derived from the registry (drop `open-palette` — you don't open the palette from the palette — and attach hints):
```tsx
  const COMMANDS: PaletteCommand[] = COMMAND_DEFS.filter(
    (d) => d.id !== "open-palette",
  ).map((d) => {
    const eff = effectiveBinding(d.id, overrides);
    return { id: d.id, label: d.label, hint: eff ? formatChord(eff, IS_MAC) : undefined };
  });
```

(b) Add the `open-palette` case at the TOP of the `runCommand` switch (the early `return` skips the trailing `setPaletteOpen(false)` so ⌘K toggles):
```tsx
  const runCommand = (id: string) => {
    switch (id) {
      case "open-palette":
        setPaletteOpen((o) => !o);
        return;
      case "new-note":
```
(Leave the rest of the switch unchanged.)

(c) Immediately AFTER the `runCommand` definition, keep the ref current:
```tsx
  runCommandRef.current = runCommand;
```

- [ ] **Step 5: App — pass keybinding props to SettingsDialog**

Change the `<SettingsDialog … />` render to add the two new props:
```tsx
      <SettingsDialog
        open={settingsOpen}
        onOpenChange={setSettingsOpen}
        settings={settings}
        onChange={actions.setSettings}
        keybindingOverrides={overrides}
        onKeybindingsChange={(o) => {
          setOverrides(o);
          saveOverrides(o);
        }}
      />
```

- [ ] **Step 6: Unit gate + build**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check && pnpm build` — all PASS. Confirm no React hook sits after the `if (cairnPath === null)` early return (`useState`/`useMemo`/`useRef`/`useEffect` are all above it; `COMMANDS`/`runCommand`/`runCommandRef.current = …` are plain statements after it).

- [ ] **Step 7: Add the e2e**

Append to `web/e2e/skeleton.spec.ts`:
```ts
test("keyboard shortcuts: rebind Open Settings, use it, and persist", async ({
  page,
}) => {
  await page.goto("/");
  const sidebar = page.locator("aside").first();
  await expect(sidebar.getByText("index", { exact: true })).toBeVisible();

  // Open Settings via the gear, then rebind "Open Settings" to a free chord.
  await page.getByLabel("Settings").click();
  const dialog = page.getByRole("dialog");
  await expect(dialog.getByText("Keyboard shortcuts")).toBeVisible();
  await dialog.getByRole("button", { name: "rebind Open Settings" }).click();
  await dialog
    .getByLabel("press keys for Open Settings")
    .press("Control+Shift+S"); // Mod+Shift+S — unused by defaults
  // Close the dialog.
  await dialog.getByRole("button", { name: "Done" }).click();
  await expect(page.getByRole("dialog")).toHaveCount(0);

  // The new chord reopens Settings.
  await page.keyboard.press("Control+Shift+S");
  await expect(page.getByRole("dialog").getByText("Keyboard shortcuts")).toBeVisible();

  // Persisted across reload.
  await page.reload();
  await page.getByLabel("Settings").click();
  await expect(
    page.getByRole("dialog").getByRole("button", { name: "rebind Open Settings" }),
  ).toHaveText("Ctrl+⇧S");
});
```
Note on the final assertion: `formatChord("Mod+Shift+S", isMac)` renders `Ctrl+⇧S` when `navigator` is non-mac and `⌘⇧S` on macOS. If the e2e runs on macOS (the dev machine), change the expected text to `⌘⇧S`; on Linux CI it's `Ctrl+⇧S`. Pick the one matching the run environment, or assert with a regex `/⇧S$/` to cover both.

- [ ] **Step 8: Run e2e**

Run: `pnpm e2e` — expect 13 passed (12 existing + this one). If port 5273 busy: `lsof -ti :5273 | xargs kill 2>/dev/null` then retry once.
- If the rebind capture doesn't register the chord, verify the capture `<input>` is focused (autoFocus) and `press(...)` targets it.
- If `Control+Shift+S` triggers a conflict warning, a default already uses it — pick another free chord (e.g. `Control+Shift+J`) consistently in the test.
- If the reopen assertion fails, check the `open-palette`/`open-settings` dispatch and that `runCommandRef.current` is current. STOP and report if a core assertion fails.

- [ ] **Step 9: Final full gate + build**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check && pnpm build` — all PASS.

- [ ] **Step 10: Manual/visual check**

`lsof -ti :5273 | xargs kill 2>/dev/null`; start `pnpm dev --port 5273 --strictPort` (background); `curl -s -o /dev/null -w "%{http_code}" http://localhost:5273` (expect 200); confirm the dev log is error-free; stop it. (Human confirms: the palette shows shortcut hints; Settings has a Keyboard-shortcuts list; rebinding captures keys, blocks conflicts with a Force option, and Reset restores; the rebound shortcut works and persists across reload; ⌘K/⌘W/⌃Tab/⌘1-9 still work.)

- [ ] **Step 11: Commit**

```bash
git add web/src/components/SettingsDialog.tsx web/src/app/App.tsx web/e2e/skeleton.spec.ts
git commit -m "feat(shortcuts): registry-driven keydown + palette hints + Settings rebinding + e2e"
```

---

## Notes for the executor

- **Stale-closure guard:** the keydown effect dispatches via `runCommandRef.current(id)`, and `runCommandRef.current = runCommand` is reassigned every render. Do NOT call `runCommand` directly inside the effect — it would capture stale `editorMode`/`view`.
- **open-palette toggles:** its `runCommand` case `return`s before the trailing `setPaletteOpen(false)`, and it's filtered out of the palette's own command list.
- **Capture swallows the event** (`preventDefault` + `stopPropagation`) so rebinding to e.g. ⌘K doesn't also toggle the palette.
- **Force is atomic:** one `onChange` sets the other command to `null` and this command to the chord together, so the bindings map never transiently holds a duplicate.
- **Mod = meta OR ctrl** (as before) — one cross-platform binding string; `formatChord` renders ⌘ on macOS, Ctrl elsewhere.
- **No store/contract changes;** keybinding overrides persist under their own `cairn.keybindings` key (the unrelated auto-commit Settings remain in-memory — out of scope).
```
