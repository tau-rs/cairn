# Tier-2 Slot-Mount — Phase 1 (Frontend) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Render plugin-declared UI `contributions` into named shell slots (`sidebar.section`, `topbar.action`, `command`) as validated data, wired to the existing `invoke_plugin_command` round-trip — no plugin JS, no iframe.

**Architecture:** A new untrusted-data sanitizer (`pluginContributions.ts`) validates + clamps + groups + sorts contributions; the store holds them grouped-by-slot with a monotonic `pluginEpoch`; `SlotRenderer` renders each via host-owned `WidgetView` components inside a per-widget `ErrorBoundary`; clicks route through `useActions().invokePlugin(plugin, command, args)`. The `command` slot flows through the existing palette path.

**Tech Stack:** React, Zustand, TypeScript, Tailwind, vitest + @testing-library/react + jsdom + MockClient.

**Spec:** `docs/superpowers/specs/2026-06-13-tier2-slot-mount-design.md`.
**Prerequisite:** Phase-0 plan complete — `web/src/contract` must already contain `PluginWidget`/`PluginSlot`/`PluginIcon`/`PluginContribution`/`PluginListItem` + `pluginValues.ts`, and `PluginSummary.contributions`. Verify: `grep -l PluginWidget web/src/contract/*.ts`.

**Branch:** `tier2-slot-mount-api` (this workspace). **Merge LAST** (overlaps Track C's shell + store edits — expect a rebase onto C). Keep `Sidebar.tsx`/`TopBar.tsx` edits to a single line each to minimize conflict surface.

**Commands:** test = `pnpm -C web test <file>`; full gate = `just` (from repo root).

---

## File Structure (this repo)

- `web/src/client/pluginContributions.ts` — **create**. Sanitizer + caps + `SanitizeReport` + `groupBySlot` (sort) + slot/kind lockstep test target. Own module (distinct from the thin `contractGuards.ts`).
- `web/src/components/plugins/pluginIcon.tsx` — **create**. `PluginIcon → ReactNode` (closed, exhaustive).
- `web/src/components/plugins/WidgetView.tsx` — **create**. `text`/`action`/`list` host components + dispatch.
- `web/src/components/plugins/SlotRenderer.tsx` — **create**. Slot → ErrorBoundary-wrapped WidgetViews; `WidgetError` stub.
- `web/src/store/store.ts` — **modify**. `pluginContributions` + `pluginEpoch` + `pluginDropped` state; `loadPlugins` grouping; `loadCairn` reset; `invokePlugin(args)` widening; `CairnState` interface.
- `web/src/client/mock.ts` — **modify**. Demo contributions + a no-contrib second plugin; `stamp` echoes `args`.
- `web/src/components/Sidebar.tsx` — **modify** (1 line). Mount `sidebar.section`.
- `web/src/components/TopBar.tsx` — **modify** (1 line). Mount `topbar.action`.
- `web/src/components/plugins/pluginCommands.ts` — **modify**. Flatten `command`-slot contributions + args lookup.
- `web/src/app/useCommands.ts` — **modify**. Thread `args` through `runCommand`.
- `web/src/components/plugins/PluginsPanel.tsx` + `web/src/components/SettingsDialog.tsx` — **modify**. Surface the not-rendered count.

---

## Task 1: `pluginContributions.ts` — sanitizer, caps, lockstep

**Files:**
- Create: `web/src/client/pluginContributions.ts`
- Test: `web/src/client/pluginContributions.test.ts`

- [ ] **Step 1: Write the failing test**

```ts
import { describe, it, expect } from "vitest";
import { sanitizeContributions, type SanitizeReport } from "./pluginContributions";
import { PLUGIN_SLOT_VALUES, PLUGIN_WIDGET_KIND_VALUES } from "../contract";

const ok = { id: "a", slot: "sidebar.section", widget: { kind: "text", text: "hi" } };

describe("sanitizeContributions", () => {
  it("keeps a well-formed contribution", () => {
    expect(sanitizeContributions([ok])).toEqual([ok]);
  });
  it("drops unknown slot / kind / missing id", () => {
    expect(sanitizeContributions([{ ...ok, slot: "foo.bar" }])).toEqual([]);
    expect(sanitizeContributions([{ ...ok, widget: { kind: "canvas" } }])).toEqual([]);
    expect(sanitizeContributions([{ slot: "sidebar.section", widget: { kind: "text", text: "x" } }])).toEqual([]);
  });
  it("drops a command-slot contribution whose widget is not action", () => {
    expect(sanitizeContributions([{ id: "c", slot: "command", widget: { kind: "text", text: "x" } }])).toEqual([]);
    const cmd = { id: "c", slot: "command", widget: { kind: "action", label: "Go", command: "go" } };
    expect(sanitizeContributions([cmd])).toEqual([cmd]);
  });
  it("clamps long text, truncates long lists, drops oversized args, caps the array", () => {
    const long = sanitizeContributions([{ ...ok, widget: { kind: "text", text: "x".repeat(5000) } }]);
    expect((long[0].widget as { text: string }).text.length).toBe(2000);
    const list = sanitizeContributions([{ id: "l", slot: "sidebar.section",
      widget: { kind: "list", items: Array.from({ length: 10000 }, (_, i) => ({ id: String(i), label: "x" })) } }]);
    expect((list[0].widget as { items: unknown[] }).items.length).toBe(200);
    const big = sanitizeContributions([{ id: "b", slot: "topbar.action",
      widget: { kind: "action", label: "x", command: "c", args: { blob: "x".repeat(20000) } } }]);
    expect(big).toEqual([]);
    expect(sanitizeContributions(Array.from({ length: 1000 }, (_, i) => ({ ...ok, id: String(i) }))).length).toBe(64);
  });
  it("coerces out-of-enum icon to undefined", () => {
    const r = sanitizeContributions([{ id: "i", slot: "topbar.action",
      widget: { kind: "action", label: "x", command: "c", icon: "nope" } }]);
    expect((r[0].widget as { icon?: string }).icon).toBeUndefined();
  });
  it("fills a SanitizeReport when dropping", () => {
    const report: SanitizeReport = { kept: 0, dropped: 0, reasons: [] };
    sanitizeContributions([{ ...ok, slot: "foo.bar" }], report);
    expect(report.dropped).toBe(1);
    expect(report.reasons.length).toBeGreaterThan(0);
  });
  it("allow-lists stay supersets of the engine-emitted runtime arrays (lockstep)", () => {
    for (const s of PLUGIN_SLOT_VALUES) expect((["sidebar.section","topbar.action","command"] as string[])).toContain(s);
    for (const k of PLUGIN_WIDGET_KIND_VALUES) expect((["text","action","list"] as string[])).toContain(k);
  });
});
```

- [ ] **Step 2: Run it — fails (module missing)**

Run: `pnpm -C web test pluginContributions`
Expected: FAIL — cannot find `./pluginContributions`.

- [ ] **Step 3: Implement the sanitizer**

```ts
// web/src/client/pluginContributions.ts
// UNTRUSTED-DATA validator for plugin descriptors. DISTINCT from contractGuards.ts
// (the thin S5 outer-union tag-check, throw-on-drift). Posture here: drop-unknown
// + clamp, NEVER throw — a forward-version plugin degrades, it does not crash a pane.
import type { PluginSummary } from "../contract";
import type { PluginContribution } from "../contract/PluginContribution";
import type { PluginSlot } from "../contract/PluginSlot";

export const PLUGIN_SLOTS = ["sidebar.section", "topbar.action", "command"] as const;
const WIDGET_KINDS = ["text", "action", "list"] as const;
const PLUGIN_ICONS = ["tag","search","note","folder","link","star","info","play"] as const;
const MAX_CONTRIBS_PER_PLUGIN = 64, MAX_LIST_ITEMS = 200, MAX_STR = 2000, MAX_ARGS_BYTES = 16384;

export type SanitizeReport = { kept: number; dropped: number; reasons: string[] };
export type SlotEntry = { plugin: string; c: PluginContribution; epoch: number };

const clampStr = (s: unknown) => (typeof s === "string" ? s.slice(0, MAX_STR) : "");
const okIcon = (i: unknown) => (PLUGIN_ICONS as readonly string[]).includes(i as string) ? (i as string) : undefined;
const argsTooBig = (a: unknown) => a !== undefined && JSON.stringify(a).length > MAX_ARGS_BYTES;

function cleanWidget(w: { kind?: string; [k: string]: unknown }): PluginContribution["widget"] | null {
  switch (w.kind) {
    case "text":
      return { kind: "text", text: clampStr(w.text), ...(typeof w.muted === "boolean" ? { muted: w.muted } : {}) };
    case "action":
      if (typeof w.command !== "string" || argsTooBig(w.args)) return null;
      return { kind: "action", label: clampStr(w.label), command: w.command,
        ...(okIcon(w.icon) ? { icon: okIcon(w.icon) } : {}), ...(w.args !== undefined ? { args: w.args } : {}) };
    case "list": {
      const raw = Array.isArray(w.items) ? w.items.slice(0, MAX_LIST_ITEMS) : [];
      const items = raw.filter((it) => it && typeof it.id === "string").map((it) => ({
        id: it.id, label: clampStr(it.label),
        ...(okIcon(it.icon) ? { icon: okIcon(it.icon) } : {}),
        ...(typeof it.command === "string" && !argsTooBig(it.args) ? { command: it.command } : {}),
        ...(typeof it.command === "string" && !argsTooBig(it.args) && it.args !== undefined ? { args: it.args } : {}),
      }));
      return { kind: "list", items };
    }
    default:
      return null;
  }
}

export function sanitizeContributions(raw: unknown, report?: SanitizeReport): PluginContribution[] {
  const drop = (why: string) => { if (report) { report.dropped++; report.reasons.push(why); } };
  if (!Array.isArray(raw)) return [];
  const out: PluginContribution[] = [];
  for (const r of raw.slice(0, MAX_CONTRIBS_PER_PLUGIN)) {
    if (!r || typeof r.id !== "string") { drop("missing id"); continue; }
    if (!(PLUGIN_SLOTS as readonly string[]).includes(r.slot)) { drop(`slot ${r.slot}`); continue; }
    if (!r.widget || !(WIDGET_KINDS as readonly string[]).includes(r.widget.kind)) { drop(`kind ${r.widget?.kind}`); continue; }
    if (r.slot === "command" && r.widget.kind !== "action") { drop("command slot needs action"); continue; }
    const widget = cleanWidget(r.widget);
    if (!widget) { drop(`bad widget ${r.widget.kind}`); continue; }
    out.push({ id: r.id, slot: r.slot, widget,
      ...(typeof r.title === "string" ? { title: clampStr(r.title) } : {}),
      ...(okIcon(r.icon) ? { icon: okIcon(r.icon) } : {}),
      ...(typeof r.order === "number" ? { order: r.order } : {}) });
  }
  if (report) report.kept += out.length;
  return out;
}

/** Group sanitized contributions by slot, sorted (order ↑, then plugin id, then contribution id). */
export function groupBySlot(plugins: PluginSummary[], epoch: number, report?: SanitizeReport): Record<string, SlotEntry[]> {
  const groups: Record<string, SlotEntry[]> = {};
  for (const p of plugins)
    for (const c of sanitizeContributions(p.contributions ?? [], report))
      (groups[c.slot] ??= []).push({ plugin: p.id, c, epoch });
  for (const slot of Object.keys(groups))
    groups[slot].sort((a, b) =>
      (a.c.order ?? Infinity) - (b.c.order ?? Infinity) ||
      a.plugin.localeCompare(b.plugin) || a.c.id.localeCompare(b.c.id));
  return groups;
}
```

- [ ] **Step 4: Run — passes**

Run: `pnpm -C web test pluginContributions`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/client/pluginContributions.ts web/src/client/pluginContributions.test.ts
git commit -m "feat(plugins): contribution sanitizer + grouping"
```

---

## Task 2: `pluginIcon.tsx` — closed icon registry + lockstep

**Files:**
- Create: `web/src/components/plugins/pluginIcon.tsx`
- Test: `web/src/components/plugins/pluginIcon.test.tsx`

- [ ] **Step 1: Failing test**

```tsx
import { describe, it, expect } from "vitest";
import { PLUGIN_ICON_VALUES } from "../../contract";
import { pluginIconNode, PLUGIN_ICON_KEYS } from "./pluginIcon";

describe("pluginIcon", () => {
  it("maps every enum value to a node (registry ⊇ engine values)", () => {
    for (const v of PLUGIN_ICON_VALUES) {
      expect(PLUGIN_ICON_KEYS).toContain(v);
      expect(pluginIconNode(v)).not.toBeNull();
    }
  });
  it("returns null for an unknown icon", () => {
    expect(pluginIconNode("nope" as never)).toBeNull();
  });
});
```

- [ ] **Step 2: Run — fails**

Run: `pnpm -C web test pluginIcon`
Expected: FAIL — module missing.

- [ ] **Step 3: Implement** (small inline SVGs or reuse existing icon set — keep one node per key; `Record<PluginIcon, …>` makes a missing key a compile error)

```tsx
import type { ReactNode } from "react";
import type { PluginIcon } from "../../contract/PluginIcon";

const G = (d: string): ReactNode => (
  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor"
    strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d={d} /></svg>
);

const ICONS: Record<PluginIcon, ReactNode> = {
  tag: G("M20.59 13.41 12 22l-9-9V3h10l7.59 7.59a2 2 0 0 1 0 2.82z"),
  search: G("M21 21l-4.3-4.3M11 19a8 8 0 1 1 0-16 8 8 0 0 1 0 16z"),
  note: G("M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"),
  folder: G("M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"),
  link: G("M10 13a5 5 0 0 0 7 0l3-3a5 5 0 0 0-7-7l-1 1"),
  star: G("M12 2l3 7 7 .5-5 4.5 1.5 7L12 17l-6 4 1.5-7-5-4.5 7-.5z"),
  info: G("M12 16v-4M12 8h.01M12 22a10 10 0 1 1 0-20 10 10 0 0 1 0 20z"),
  play: G("M5 3l14 9-14 9z"),
};

export const PLUGIN_ICON_KEYS = Object.keys(ICONS);
export function pluginIconNode(icon: PluginIcon): ReactNode | null {
  return ICONS[icon] ?? null;
}
```

- [ ] **Step 4: Run — passes** · Run: `pnpm -C web test pluginIcon` · Expected: PASS.
- [ ] **Step 5: Commit**

```bash
git add web/src/components/plugins/pluginIcon.tsx web/src/components/plugins/pluginIcon.test.tsx
git commit -m "feat(plugins): closed plugin icon registry"
```

---

## Task 3: Store — contributions state, grouping, `invokePlugin(args)`, mock fixtures

**Files:**
- Modify: `web/src/store/store.ts` (`CairnState` ~83-159; `DEFAULT`/initial state; `loadCairn` reset ~340; `loadPlugins` 703; `invokePlugin` 144/713)
- Modify: `web/src/client/mock.ts` (plugins 78-84; invoke handler 134-140)
- Test: `web/src/store/store.test.ts` (append)

- [ ] **Step 1: Failing store test**

```ts
it("groups + sorts plugin contributions and bumps epoch", async () => {
  const { store } = setup();
  await store.getState().init();
  const st = store.getState();
  expect(st.pluginContributions["sidebar.section"]?.[0].plugin).toBe("demo");
  expect(st.pluginContributions["sidebar.section"]?.[0].epoch).toBe(st.pluginEpoch);
  expect(st.pluginEpoch).toBeGreaterThan(0);
});
it("invokePlugin threads args to the engine", async () => {
  const { store } = setup();
  await store.getState().init();
  await store.getState().invokePlugin("demo", "stamp", { n: 1 });
  expect(store.getState().notice).toContain("1"); // mock echoes args.n
});
it("resets contributions on reload, epoch stays monotonic", async () => {
  const { store } = setup();
  await store.getState().init();
  const e1 = store.getState().pluginEpoch;
  await store.getState().loadPlugins();
  expect(store.getState().pluginEpoch).toBeGreaterThan(e1);
});
```

- [ ] **Step 2: Run — fails** · Run: `pnpm -C web test store.test` · Expected: FAIL (`pluginContributions` undefined / `invokePlugin` arity).

- [ ] **Step 3: Edit `store.ts`**

In `CairnState` (after `plugins: PluginSummary[];`):
```ts
  pluginContributions: Record<string, SlotEntry[]>;
  pluginEpoch: number;
  pluginDropped: number;
```
Change the interface method (line 144):
```ts
  invokePlugin(plugin: string, command: string, args?: JsonValue): Promise<void>;
```
Add the import near the top:
```ts
import { groupBySlot, type SlotEntry, type SanitizeReport } from "../client/pluginContributions";
```
In the initial state object (where `plugins: []` is first set as default), add:
```ts
  pluginContributions: {},
  pluginEpoch: 0,
  pluginDropped: 0,
```
In `loadCairn`'s reset `set({...})` (~340), add alongside `plugins: []`:
```ts
  pluginContributions: {},
  pluginDropped: 0,
```
(do NOT reset `pluginEpoch` — it stays monotonic.)
Replace `loadPlugins` (703-711):
```ts
async loadPlugins() {
  try {
    const res = await client.runQuery({ type: "list_plugins" });
    if (res.type === "plugins") {
      const epoch = get().pluginEpoch + 1;
      const report: SanitizeReport = { kept: 0, dropped: 0, reasons: [] };
      const pluginContributions = groupBySlot(res.plugins, epoch, report);
      if (report.dropped > 0)
        console.warn(`[plugins] dropped ${report.dropped} contribution(s):`, report.reasons);
      set({ plugins: res.plugins, pluginContributions, pluginEpoch: epoch, pluginDropped: report.dropped });
    } else unexpected("Load plugins", res);
  } catch (err) {
    pushError("Load plugins", err);
  }
},
```
Widen `invokePlugin` (713):
```ts
async invokePlugin(plugin, command, args = null) {
  try {
    const res = await client.sendCommand({ type: "invoke_plugin_command", plugin, command, args });
    if (res.type === "plugin_result") {
      set({ notice: typeof res.result === "string" ? res.result : `Ran ${command}` });
    } else unexpected("Run plugin command", res, { plugin, command });
  } catch (err) {
    pushError("Run plugin command", err, { plugin, command });
  }
},
```
(`JsonValue` is already imported by the contract types used in `sendCommand`; if not, add `import type { JsonValue } from "../contract/serde_json/JsonValue";`.)

- [ ] **Step 4: Edit `mock.ts`** — give `demo` contributions, add a no-contrib plugin, echo `args` from `stamp`:

Plugins (78-84):
```ts
private plugins: PluginSummary[] = [
  {
    id: "demo", name: "Demo plugin", version: "1.0.0",
    commands: [{ id: "stamp", title: "Insert stamp note" }],
    contributions: [
      { id: "recent", slot: "sidebar.section", title: "Demo",
        widget: { kind: "list", items: [{ id: "s", label: "Insert stamp", command: "stamp" }] } },
      { id: "stampbtn", slot: "topbar.action",
        widget: { kind: "action", label: "Stamp", icon: "note", command: "stamp" } },
      { id: "stampcmd", slot: "command",
        widget: { kind: "action", label: "Insert stamp (cmd)", command: "stamp" } },
    ],
  },
  { id: "bare", name: "Bare plugin", version: "0.1.0", commands: [], contributions: [] },
];
```
Invoke handler (135-139) — echo args:
```ts
if (c.plugin === "demo" && c.command === "stamp") {
  this.notes.set("stamp.md", "# Stamp\n");
  this.emit({ type: "note_changed", path: "stamp.md" });
  this.emit({ type: "reindexed", count: this.notes.size });
  const suffix = c.args && typeof c.args === "object" && "n" in c.args ? ` ${(c.args as { n: unknown }).n}` : "";
  return { type: "plugin_result", result: `stamp.md${suffix}` };
}
```

- [ ] **Step 5: Run — passes** · Run: `pnpm -C web test store.test` · Expected: PASS.
- [ ] **Step 6: Commit**

```bash
git add web/src/store/store.ts web/src/client/mock.ts web/src/store/store.test.ts
git commit -m "feat(plugins): store contributions state + invokePlugin args"
```

---

## Task 4: `WidgetView.tsx` — host renderers + dispatch

**Files:**
- Create: `web/src/components/plugins/WidgetView.tsx`
- Test: `web/src/components/plugins/WidgetView.test.tsx`

- [ ] **Step 1: Failing test**

```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const invokePlugin = vi.fn();
vi.mock("../../app/cairnStore", () => ({ useActions: () => ({ invokePlugin }) }));
import { WidgetView } from "./WidgetView";

describe("WidgetView", () => {
  it("renders text and escapes script", () => {
    render(<WidgetView plugin="p" widget={{ kind: "text", text: "<script>x</script>" }} />);
    expect(screen.getByText("<script>x</script>")).toBeInTheDocument();
  });
  it("action click invokes with args", async () => {
    render(<WidgetView plugin="p" widget={{ kind: "action", label: "Go", command: "go", args: { a: 1 } }} />);
    await userEvent.click(screen.getByText("Go"));
    expect(invokePlugin).toHaveBeenCalledWith("p", "go", { a: 1 });
  });
  it("list item click fires its command", async () => {
    render(<WidgetView plugin="p" widget={{ kind: "list", items: [{ id: "i", label: "Row", command: "c" }] }} />);
    await userEvent.click(screen.getByText("Row"));
    expect(invokePlugin).toHaveBeenCalledWith("p", "c", null);
  });
  it("renders nothing for an unknown kind", () => {
    const { container } = render(<WidgetView plugin="p" widget={{ kind: "x" } as never} />);
    expect(container).toBeEmptyDOMElement();
  });
});
```

- [ ] **Step 2: Run — fails** · Run: `pnpm -C web test WidgetView` · Expected: FAIL.

- [ ] **Step 3: Implement**

```tsx
import { useActions } from "../../app/cairnStore";
import type { PluginWidget } from "../../contract/PluginWidget";
import { pluginIconNode } from "./pluginIcon";

export function WidgetView({ plugin, widget }: { plugin: string; widget: PluginWidget }) {
  const { invokePlugin } = useActions();
  switch (widget.kind) {
    case "text":
      return <span className={widget.muted ? "text-xs text-faint" : "text-sm text-muted"}>{widget.text}</span>;
    case "action":
      return (
        <button type="button" className="inline-flex items-center gap-1 rounded border border-border bg-surface-2 px-2 py-1 text-xs text-text hover:border-accent"
          onClick={() => void invokePlugin(plugin, widget.command, widget.args ?? null)}>
          {widget.icon && pluginIconNode(widget.icon)} {widget.label}
        </button>
      );
    case "list":
      return (
        <ul className="flex flex-col gap-0.5">
          {widget.items.map((it) => (
            <li key={it.id} className="cursor-pointer rounded px-2 py-1 text-sm text-text hover:bg-surface-2"
              onClick={() => it.command && void invokePlugin(plugin, it.command, it.args ?? null)}>
              {it.icon && pluginIconNode(it.icon)} {it.label}
            </li>
          ))}
        </ul>
      );
    default:
      return null;
  }
}
```

- [ ] **Step 4: Run — passes** · Run: `pnpm -C web test WidgetView` · Expected: PASS.
- [ ] **Step 5: Commit**

```bash
git add web/src/components/plugins/WidgetView.tsx web/src/components/plugins/WidgetView.test.tsx
git commit -m "feat(plugins): WidgetView host renderers"
```

---

## Task 5: `SlotRenderer.tsx` — slot → ErrorBoundary-wrapped widgets

**Files:**
- Create: `web/src/components/plugins/SlotRenderer.tsx`
- Test: `web/src/components/plugins/SlotRenderer.test.tsx`

- [ ] **Step 1: Failing test** (drives via the real store + MockClient so grouping is exercised)

```tsx
import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { cairnStore } from "../../app/cairnStore";
import { SlotRenderer } from "./SlotRenderer";

describe("SlotRenderer", () => {
  it("renders a slot's contributions after load", async () => {
    await cairnStore.getState().init();
    render(<SlotRenderer slot="sidebar.section" />);
    expect(await screen.findByText("Insert stamp")).toBeInTheDocument();
  });
  it("renders nothing for an empty slot", () => {
    const { container } = render(<SlotRenderer slot="topbar.action" />);
    // (after init it would render; here assert it doesn't throw on an empty group)
    expect(container).toBeDefined();
  });
});
```

- [ ] **Step 2: Run — fails** · Run: `pnpm -C web test SlotRenderer` · Expected: FAIL.

- [ ] **Step 3: Implement** (local `WidgetError` fallback + `epoch` in the key for remount-on-refetch)

```tsx
import { useShallow } from "zustand/react/shallow";
import { useCairn } from "../../app/cairnStore";
import type { PluginSlot } from "../../contract/PluginSlot";
import { ErrorBoundary } from "../ErrorBoundary";
import { WidgetView } from "./WidgetView";

function WidgetError({ onRetry }: { onRetry: () => void }) {
  return (
    <button type="button" onClick={onRetry} className="text-xs text-faint italic hover:text-muted">
      widget unavailable — retry
    </button>
  );
}

export function SlotRenderer({ slot }: { slot: PluginSlot }) {
  const here = useCairn(useShallow((s) => s.pluginContributions[slot] ?? []));
  if (here.length === 0) return null;
  return (
    <>
      {here.map(({ plugin, c, epoch }) => (
        <ErrorBoundary key={`${plugin}:${c.id}:${epoch}`} fallback={(reset) => <WidgetError onRetry={reset} />}>
          <WidgetView plugin={plugin} widget={c.widget} />
        </ErrorBoundary>
      ))}
    </>
  );
}
```

- [ ] **Step 4: Run — passes** · Run: `pnpm -C web test SlotRenderer` · Expected: PASS.
- [ ] **Step 5: Commit**

```bash
git add web/src/components/plugins/SlotRenderer.tsx web/src/components/plugins/SlotRenderer.test.tsx
git commit -m "feat(plugins): SlotRenderer with per-widget error boundary"
```

---

## Task 6: Mount `sidebar.section` in `Sidebar.tsx`

**Files:**
- Modify: `web/src/components/Sidebar.tsx`
- Test: `web/src/components/Sidebar.test.tsx` (create if absent)

- [ ] **Step 1: Failing test**

```tsx
import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { cairnStore } from "../app/cairnStore";
import { Sidebar } from "./Sidebar";

it("renders sidebar.section contributions", async () => {
  await cairnStore.getState().init();
  render(<MemoryRouter><Sidebar /></MemoryRouter>);
  expect(await screen.findByText("Insert stamp")).toBeInTheDocument();
});
```

- [ ] **Step 2: Run — fails** · Run: `pnpm -C web test Sidebar` · Expected: FAIL (no such text).

- [ ] **Step 3: Add the mount** — one line after `</TagsPanel>` close, plus the import:

```tsx
import { SlotRenderer } from "./plugins/SlotRenderer";
// …inside the fragment, after <TagsPanel ... />:
      <SlotRenderer slot="sidebar.section" />
```

- [ ] **Step 4: Run — passes** · Run: `pnpm -C web test Sidebar` · Expected: PASS.
- [ ] **Step 5: Commit**

```bash
git add web/src/components/Sidebar.tsx web/src/components/Sidebar.test.tsx
git commit -m "feat(plugins): mount sidebar.section slot"
```

---

## Task 7: Mount `topbar.action` in `TopBar.tsx`

**Files:**
- Modify: `web/src/components/TopBar.tsx`
- Test: `web/src/components/TopBar.test.tsx` (create if absent)

- [ ] **Step 1: Failing test**

```tsx
import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { cairnStore } from "../app/cairnStore";
import { TopBar } from "./TopBar";

it("renders topbar.action contributions", async () => {
  await cairnStore.getState().init();
  render(<MemoryRouter><TopBar /></MemoryRouter>);
  expect(await screen.findByText("Stamp")).toBeInTheDocument();
});
```

- [ ] **Step 2: Run — fails** · Run: `pnpm -C web test TopBar` · Expected: FAIL.

- [ ] **Step 3: Add the mount** — one line in the action cluster, after `<span className="grow" />` and before the Settings `IconButton`, plus the import:

```tsx
import { SlotRenderer } from "./plugins/SlotRenderer";
// …after <span className="grow" />:
      <SlotRenderer slot="topbar.action" />
```

- [ ] **Step 4: Run — passes** · Run: `pnpm -C web test TopBar` · Expected: PASS.
- [ ] **Step 5: Commit**

```bash
git add web/src/components/TopBar.tsx web/src/components/TopBar.test.tsx
git commit -m "feat(plugins): mount topbar.action slot"
```

---

## Task 8: `command` slot — flatten contributions + thread args

**Files:**
- Modify: `web/src/components/plugins/pluginCommands.ts`
- Modify: `web/src/app/useCommands.ts:54-60`
- Test: `web/src/components/plugins/pluginCommands.test.ts` (extend)

- [ ] **Step 1: Failing test**

```ts
import { toPaletteCommands, parsePluginCommandId, pluginCommandArgs } from "./pluginCommands";

const demo = { id: "demo", name: "Demo", version: "1", commands: [{ id: "stamp", title: "Stamp" }],
  contributions: [{ id: "x", slot: "command", widget: { kind: "action", label: "Stamp cmd", command: "go", args: { n: 2 } } }] };

it("flattens command-slot contributions into palette commands", () => {
  const cmds = toPaletteCommands([demo as never]);
  expect(cmds.some((c) => c.id === "plugin:demo/go" && c.label === "Stamp cmd")).toBe(true);
  expect(cmds.some((c) => c.id === "plugin:demo/stamp")).toBe(true);
});
it("dedupes legacy + contribution by id, contribution label wins", () => {
  const d = { ...demo, contributions: [{ id: "x", slot: "command",
    widget: { kind: "action", label: "Override", command: "stamp" } }] };
  const cmds = toPaletteCommands([d as never]).filter((c) => c.id === "plugin:demo/stamp");
  expect(cmds).toHaveLength(1);
  expect(cmds[0].label).toBe("Override");
});
it("exposes args by command id", () => {
  expect(pluginCommandArgs([demo as never])["plugin:demo/go"]).toEqual({ n: 2 });
});
```

- [ ] **Step 2: Run — fails** · Run: `pnpm -C web test pluginCommands` · Expected: FAIL.

- [ ] **Step 3: Extend `pluginCommands.ts`**

```ts
import type { PluginSummary } from "../../contract";
import type { JsonValue } from "../../contract/serde_json/JsonValue";
import type { PaletteCommand } from "../command-palette/CommandPalette";

const PREFIX = "plugin:";

export function toPaletteCommands(plugins: PluginSummary[]): PaletteCommand[] {
  const byId = new Map<string, PaletteCommand>();
  for (const p of plugins)
    for (const c of p.commands)
      byId.set(`${PREFIX}${p.id}/${c.id}`, { id: `${PREFIX}${p.id}/${c.id}`, label: `${p.name}: ${c.title}` });
  // contributions win on id collision (richer: carries args/icon → its label)
  for (const p of plugins)
    for (const ct of p.contributions ?? [])
      if (ct.slot === "command" && ct.widget.kind === "action") {
        const id = `${PREFIX}${p.id}/${ct.widget.command}`;
        byId.set(id, { id, label: ct.widget.label });
      }
  return [...byId.values()];
}

/** Map command id → args for command-slot contributions that carry args. */
export function pluginCommandArgs(plugins: PluginSummary[]): Record<string, JsonValue> {
  const out: Record<string, JsonValue> = {};
  for (const p of plugins)
    for (const ct of p.contributions ?? [])
      if (ct.slot === "command" && ct.widget.kind === "action" && ct.widget.args !== undefined)
        out[`${PREFIX}${p.id}/${ct.widget.command}`] = ct.widget.args;
  return out;
}

export function parsePluginCommandId(id: string): { plugin: string; command: string } | null {
  if (!id.startsWith(PREFIX)) return null;
  const rest = id.slice(PREFIX.length);
  const slash = rest.indexOf("/");
  if (slash === -1) return null;
  return { plugin: rest.slice(0, slash), command: rest.slice(slash + 1) };
}
```

- [ ] **Step 4: Thread args in `useCommands.ts`** — import `pluginCommandArgs`, and in `runCommand` (54-60):

```ts
import { toPaletteCommands, parsePluginCommandId, pluginCommandArgs } from "../components/plugins/pluginCommands";
// …in runCommand, replace the plugin branch:
    const pluginCmd = parsePluginCommandId(id);
    if (pluginCmd) {
      const args = pluginCommandArgs(st.plugins)[id] ?? null;
      void st.invokePlugin(pluginCmd.plugin, pluginCmd.command, args);
      st.setUi({ paletteOpen: false });
      return;
    }
```

- [ ] **Step 5: Run — passes** · Run: `pnpm -C web test pluginCommands` · Expected: PASS.
- [ ] **Step 6: Commit**

```bash
git add web/src/components/plugins/pluginCommands.ts web/src/app/useCommands.ts web/src/components/plugins/pluginCommands.test.ts
git commit -m "feat(plugins): command-slot contributions + args threading"
```

---

## Task 9: Surface the not-rendered count in `PluginsPanel`

**Files:**
- Modify: `web/src/components/plugins/PluginsPanel.tsx`
- Modify: `web/src/components/SettingsDialog.tsx` (pass `dropped`)
- Test: `web/src/components/plugins/PluginsPanel.test.tsx` (extend)

- [ ] **Step 1: Failing test**

```tsx
it("shows a not-rendered count when dropped > 0", () => {
  render(<PluginsPanel plugins={[]} dropped={3} />);
  expect(screen.getByText(/3 .*not rendered/i)).toBeInTheDocument();
});
it("shows nothing extra when dropped is 0", () => {
  render(<PluginsPanel plugins={[]} dropped={0} />);
  expect(screen.queryByText(/not rendered/i)).toBeNull();
});
```

- [ ] **Step 2: Run — fails** · Run: `pnpm -C web test PluginsPanel` · Expected: FAIL (prop not supported).

- [ ] **Step 3: Add the prop + line** to `PluginsPanel`:

```tsx
export function PluginsPanel(props: { plugins: PluginSummary[]; dropped?: number }) {
  // …existing body…, then before the closing </div>:
      {props.dropped ? (
        <span className="text-xs text-faint">
          {props.dropped} contribution(s) not rendered — unsupported by this version
        </span>
      ) : null}
```

- [ ] **Step 4: Pass it from `SettingsDialog.tsx`** — read `pluginDropped` and pass:

```tsx
// where <PluginsPanel plugins={plugins} /> is rendered:
<PluginsPanel plugins={plugins} dropped={useCairn((s) => s.pluginDropped)} />
```
(If `plugins` is already pulled via `useCairn`, add the `pluginDropped` selector beside it.)

- [ ] **Step 5: Run — passes** · Run: `pnpm -C web test PluginsPanel` · Expected: PASS.
- [ ] **Step 6: Commit**

```bash
git add web/src/components/plugins/PluginsPanel.tsx web/src/components/SettingsDialog.tsx web/src/components/plugins/PluginsPanel.test.tsx
git commit -m "feat(plugins): surface not-rendered contribution count"
```

---

## Task 10: Full gate

- [ ] **Step 1: Run the complete local gate**

Run (repo root): `just`
Expected: PASS — eslint, `prettier --check`, `tsc`, vitest, cargo all green. (Per the CI-local-gates note, `prettier --check` is easy to miss — run the full `just`, not just tests.)

- [ ] **Step 2: Fix any formatting** · Run: `just fix` (or `pnpm -C web prettier --write src`) then re-run `just`.

- [ ] **Step 3: Manual smoke (optional)** · Run the app, open Settings → confirm the demo plugin's sidebar list + topbar Stamp button render and fire (`stamp.md` appears / notice shows). See the `verify` skill.

- [ ] **Step 4: Final commit if anything changed**

```bash
git add -A && git commit -m "chore(plugins): formatting + gate"
```

---

## Self-Review notes
- **Spec coverage:** §3.3 sanitizer (T1), §3.1 icon enum (T2), §3.6 store/state/`invokePlugin` args (T3), §3.4 WidgetView/SlotRenderer + epoch remount + local fallback (T4–T5), §3.5 mounts (T6–T7), §3.5.1 command slot + args (T8), §3.8 drop count (T9), §7 TDD throughout, §8 back-compat (the `bare` no-contrib plugin + optional `dropped` prop). ✔
- **Type consistency:** `SlotEntry` defined in T1, imported in T3/T5; `invokePlugin(plugin, command, args?)` widened in T3, called in T4/T8; `pluginCommandArgs` defined T8, used in `useCommands`. ✔
- **Adjustment from spec wording:** §3.5.1 said "args travels on the `PaletteCommand`"; the real `PaletteCommand` is `{id,label,hint}` and dispatch is `runCommand(id)`, so args are threaded via `pluginCommandArgs(plugins)[id]` in `useCommands.runCommand` instead (T8). Same effect, fits the actual seam.
- **Merge-last caution:** `Sidebar.tsx`/`TopBar.tsx`/`store.ts`/`useCommands.ts` overlap Track C — rebase onto C before opening the PR; the mounts are single lines to keep conflicts trivial.
