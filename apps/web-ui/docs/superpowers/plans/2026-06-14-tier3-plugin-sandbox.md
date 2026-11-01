# Tier-3 Sandboxed-Iframe Plugins Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the Phase-6a host side of Tier-3 plugins — untrusted plugin HTML/JS in a null-origin `srcdoc` iframe, talking to the host only through a capability-gated `postMessage` broker, behind a grouped user-consent permission model.

**Architecture:** Plugin code travels as an `html` string in a new `iframe` `PluginWidget` variant. `WidgetView` mounts it via `IframeHost`, which runs a sandboxed iframe and owns a per-frame `pluginBroker`. The broker authenticates by `event.source`, capability-gates every method against a persisted grant, clamps params, and dispatches to a `BrokerHost` port (a thin store/client adapter). A `pluginGrantsSlice` holds grants; `PermissionPrompt` gathers consent before first mount; `PluginsPanel` revokes.

**Tech Stack:** React 18 + TypeScript + Zustand (vanilla store) + Vite + Vitest + Testing Library + Playwright (e2e). Tauri webview host.

**Spec:** `docs/superpowers/specs/2026-06-14-tier3-plugin-sandbox-design.md`

---

## Working conventions

- **Per-task gate (run before every commit):** from `web/`:
  `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check`
  (`format:check` is easy to miss; eslint won't catch it — see memory `ci-local-gates`.)
- **TDD:** write the failing test, run it red, implement minimally, run it green, gate, commit.
- **Conventional commits**, imperative, scoped (e.g. `feat(plugins): …`).
- **Branch:** `docs/tier3-plugin-sandbox` already holds the spec+plan. Implementation
  is Wave-2; create `feat/tier3-plugin-sandbox` off `main` when starting.
- All paths below are relative to the repo root.

## Contract handling (read before Task 1)

The Tier-3 contract additions (`iframe` widget variant, `PluginSummary.capabilities`,
`PluginCapability` enum) live in the **engine repo** `tau-rs/cairn` and arrive here
only via the vendored-contract sync — **never hand-edit `web/src/contract/`**
(memory: `contract-sync-raw-format`). To build host-side ahead of the engine:

- The sanitizer already takes `raw: unknown` and the broker reads grants from a
  host-side store. So the host **tolerates** the new wire fields defensively
  without the vendored union being widened.
- We define the host's own Tier-3 types in `web/src/client/pluginTier3.ts`
  (Task 1). These are the host's internal representation, independent of the
  generated contract — exactly the posture `pluginContributions.ts` already takes
  with its local `WIDGET_KINDS` allow-list.
- When the engine lands the additions and the contract is re-synced, no host code
  changes: the generated `PluginWidget`/`PluginSummary` simply gain the fields the
  host already parses. A lockstep test (Task 2) asserts the host allow-list stays
  a superset of the contract values, mirroring the existing drift guard.

## File Structure

| File | Responsibility |
|------|----------------|
| `web/src/client/pluginTier3.ts` (new) | Tier-3 host types: `IframeWidget`, `PluginCapability`, capability↔method map, risk grouping for the prompt, clamp limits |
| `web/src/client/pluginContributions.ts` (modify) | Accept `kind:"iframe"` widgets + sanitize `capabilities`; clamp `html`/`height` |
| `web/src/store/pluginGrantsSlice.ts` (new) | Grants state + actions (load/grant/revoke/needsConsent); `cairn.pluginGrants` persistence |
| `web/src/store/store.ts` (modify) | One import + one spread to wire the grants slice |
| `web/src/client/pluginBrokerHost.ts` (new) | `BrokerHost` port + `createStoreBrokerHost(store, client)` adapter |
| `web/src/client/pluginBroker.ts` (new) | Per-frame broker: validate, gate, clamp, dispatch, timeout, rate cap |
| `web/src/components/plugins/PermissionPrompt.tsx` (new) | Grouped plain-language consent UI |
| `web/src/components/plugins/IframeHost.tsx` (new) | Sandboxed iframe + lifecycle state machine + broker ownership |
| `web/src/components/plugins/WidgetView.tsx` (modify) | `iframe` branch: gate (prompt vs IframeHost) |
| `web/src/components/plugins/PluginsPanel.tsx` (modify) | Per-plugin grant status + Revoke |
| `web/src/client/mock.ts` (modify) | Seed a demo word-count iframe plugin |
| `web/e2e/tier3-plugins.spec.ts` (new) | End-to-end consent → read → write → revoke |

---

## Task 1: Tier-3 host types, capability map, and risk grouping

**Files:**
- Create: `web/src/client/pluginTier3.ts`
- Test: `web/src/client/pluginTier3.test.ts`

- [ ] **Step 1: Write the failing test**

```ts
// web/src/client/pluginTier3.test.ts
import { describe, expect, it } from "vitest";
import {
  CAPABILITY_OF,
  PLUGIN_CAPABILITY_VALUES,
  groupCapabilities,
  isCapability,
} from "./pluginTier3";

describe("pluginTier3 capability model", () => {
  it("maps every method to a capability or null (silent)", () => {
    expect(CAPABILITY_OF["activeNote.write"]).toBe("activeNote.write");
    expect(CAPABILITY_OF["notes.search"]).toBe("notes.search");
    expect(CAPABILITY_OF["host.info"]).toBeNull();
    expect(CAPABILITY_OF["ui.notice"]).toBeNull();
  });

  it("recognizes only known capability strings", () => {
    expect(isCapability("activeNote.write")).toBe(true);
    expect(isCapability("filesystem.format")).toBe(false);
  });

  it("groups capabilities into plain-language risk rows, dropping silent ones", () => {
    const rows = groupCapabilities([
      "activeNote.write",
      "notes.read",
      "notes.search",
      "activeNote.read",
      "command.invoke",
    ]);
    const labels = rows.map((r) => r.label);
    expect(labels).toContain("Modify the current note");
    expect(labels).toContain("Read across your whole vault"); // notes.read+search collapse to one row
    expect(labels).toContain("Read the current note");
    expect(labels).not.toContain("command.invoke"); // silent → no row
    // notes.read + notes.search collapse into a single row (deduped)
    expect(rows.filter((r) => r.label === "Read across your whole vault")).toHaveLength(1);
  });

  it("orders rows HIGH severity first", () => {
    const rows = groupCapabilities(["activeNote.read", "activeNote.write"]);
    expect(rows[0].severity).toBe("high");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd web && pnpm test pluginTier3`
Expected: FAIL — cannot find module `./pluginTier3`.

- [ ] **Step 3: Write the implementation**

```ts
// web/src/client/pluginTier3.ts
// Host-side Tier-3 types and capability model. Independent of the generated
// contract (which gains these fields on the next engine sync) — same posture as
// pluginContributions.ts's local WIDGET_KINDS allow-list. See the plan's
// "Contract handling" note.

/** A method a sandboxed plugin may request through the broker. */
export type BrokerMethod =
  | "host.info"
  | "ui.notice"
  | "activeNote.read"
  | "activeNote.subscribe"
  | "activeNote.write"
  | "notes.read"
  | "notes.search"
  | "command.invoke";

/** A user-grantable capability declared by a plugin. `activeNote.subscribe` is
 *  folded under `activeNote.read` (subscribing is reading), so it is NOT a
 *  separate capability — see spec §6 review note. */
export type PluginCapability =
  | "activeNote.read"
  | "activeNote.write"
  | "notes.read"
  | "notes.search"
  | "command.invoke";

export const PLUGIN_CAPABILITY_VALUES = [
  "activeNote.read",
  "activeNote.write",
  "notes.read",
  "notes.search",
  "command.invoke",
] as const;

const CAP_SET: ReadonlySet<string> = new Set(PLUGIN_CAPABILITY_VALUES);

export function isCapability(x: unknown): x is PluginCapability {
  return typeof x === "string" && CAP_SET.has(x);
}

/** method → capability gate (null = no permission required, "silent"). */
export const CAPABILITY_OF: Record<BrokerMethod, PluginCapability | null> = {
  "host.info": null,
  "ui.notice": null,
  "activeNote.read": "activeNote.read",
  "activeNote.subscribe": "activeNote.read",
  "activeNote.write": "activeNote.write",
  "notes.read": "notes.read",
  "notes.search": "notes.search",
  "command.invoke": "command.invoke",
};

export type RiskSeverity = "high" | "normal";
export type RiskRow = { label: string; severity: RiskSeverity };

// Each capability maps to a plain-language row (or null = silent). Multiple
// capabilities can share a row label; we dedupe by label (Chrome-style grouping).
const RISK_OF: Record<PluginCapability, RiskRow | null> = {
  "activeNote.write": { label: "Modify the current note", severity: "high" },
  "notes.read": { label: "Read across your whole vault", severity: "high" },
  "notes.search": { label: "Read across your whole vault", severity: "high" },
  "activeNote.read": { label: "Read the current note", severity: "normal" },
  "command.invoke": null, // silent: plugin's own commands
};

/** Collapse a declared capability set into deduped, severity-sorted risk rows. */
export function groupCapabilities(caps: PluginCapability[]): RiskRow[] {
  const byLabel = new Map<string, RiskRow>();
  for (const cap of caps) {
    const row = RISK_OF[cap];
    if (row && !byLabel.has(row.label)) byLabel.set(row.label, row);
  }
  return [...byLabel.values()].sort((a, b) =>
    a.severity === b.severity ? 0 : a.severity === "high" ? -1 : 1,
  );
}

/** Host-side representation of an iframe widget (post-sanitize). */
export type IframeWidget = {
  kind: "iframe";
  html: string;
  height: number | null;
};

// Clamp limits for iframe widgets (sanitizer + broker share these).
export const MAX_IFRAME_HTML = 256 * 1024; // 256 KiB inlined HTML
export const MIN_IFRAME_HEIGHT = 80;
export const MAX_IFRAME_HEIGHT = 600;
export const DEFAULT_IFRAME_HEIGHT = 240;

// Broker runtime limits.
export const BROKER_REQUEST_TIMEOUT_MS = 5000;
export const BROKER_HANDSHAKE_TIMEOUT_MS = 3000;
export const BROKER_RATE_WINDOW_MS = 1000;
export const BROKER_RATE_MAX = 50; // inbound messages per window before dropping
export const MAX_BROKER_STR = 100_000; // clamp text params (e.g. write payloads)
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd web && pnpm test pluginTier3`
Expected: PASS (4 tests).

- [ ] **Step 5: Per-task gate**

Run: `cd web && pnpm test && pnpm typecheck && pnpm lint && pnpm format:check`
Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add web/src/client/pluginTier3.ts web/src/client/pluginTier3.test.ts
git commit -m "feat(plugins): add Tier-3 host capability model and limits"
```

---

## Task 2: Sanitize iframe widgets and capabilities

**Files:**
- Modify: `web/src/client/pluginContributions.ts`
- Test: `web/src/client/pluginContributions.test.ts` (extend)

- [ ] **Step 1: Write the failing tests**

Append to `web/src/client/pluginContributions.test.ts`:

```ts
import { sanitizeContributions, sanitizeCapabilities } from "./pluginContributions";
import { MAX_IFRAME_HTML, MAX_IFRAME_HEIGHT } from "./pluginTier3";

describe("Tier-3 iframe widget sanitization", () => {
  it("accepts an iframe widget and clamps height into range", () => {
    const out = sanitizeContributions([
      { id: "wc", slot: "sidebar.section", widget: { kind: "iframe", html: "<p>hi</p>", height: 9999 } },
    ]);
    expect(out).toHaveLength(1);
    const w = out[0].widget as { kind: string; html: string; height: number | null };
    expect(w.kind).toBe("iframe");
    expect(w.height).toBe(MAX_IFRAME_HEIGHT);
  });

  it("drops an iframe widget whose html exceeds the byte cap", () => {
    const report = { kept: 0, dropped: 0, reasons: [] as string[] };
    const out = sanitizeContributions(
      [{ id: "big", slot: "sidebar.section", widget: { kind: "iframe", html: "x".repeat(MAX_IFRAME_HTML + 1), height: null } }],
      report,
    );
    expect(out).toHaveLength(0);
    expect(report.reasons.join()).toMatch(/html too large/);
  });

  it("rejects an iframe widget outside sidebar.section", () => {
    const out = sanitizeContributions([
      { id: "x", slot: "command", widget: { kind: "iframe", html: "<p>x</p>", height: null } },
    ]);
    expect(out).toHaveLength(0); // command slot requires an action widget
  });

  it("sanitizeCapabilities keeps known values and drops unknown ones", () => {
    expect(sanitizeCapabilities(["activeNote.write", "filesystem.format", 7])).toEqual([
      "activeNote.write",
    ]);
    expect(sanitizeCapabilities(null)).toEqual([]);
    expect(sanitizeCapabilities("nope")).toEqual([]);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd web && pnpm test pluginContributions`
Expected: FAIL — `sanitizeCapabilities` not exported; iframe kind dropped as unknown.

- [ ] **Step 3: Implement**

In `web/src/client/pluginContributions.ts`:

Add to the imports:

```ts
import {
  MAX_IFRAME_HTML,
  MAX_IFRAME_HEIGHT,
  MIN_IFRAME_HEIGHT,
  DEFAULT_IFRAME_HEIGHT,
  PLUGIN_CAPABILITY_VALUES,
  isCapability,
  type PluginCapability,
} from "./pluginTier3";
```

Add `"iframe"` to the widget allow-list:

```ts
export const WIDGET_KINDS = ["text", "action", "list", "iframe"] as const;
```

In `sanitizeWidget`, before the trailing `// kind === "list"` branch, add:

```ts
  if (kind === "iframe") {
    if (typeof raw.html !== "string")
      return drop(report, "iframe widget: missing string html");
    if (raw.html.length > MAX_IFRAME_HTML)
      return drop(report, "iframe widget: html too large");
    let height: number | null = DEFAULT_IFRAME_HEIGHT;
    if (raw.height === null) height = null;
    else if (typeof raw.height === "number" && Number.isFinite(raw.height))
      height = Math.min(MAX_IFRAME_HEIGHT, Math.max(MIN_IFRAME_HEIGHT, raw.height));
    return { kind: "iframe", html: raw.html, height } as PluginWidget;
  }
```

Note the cast: the vendored `PluginWidget` union does not yet include `iframe`
(it arrives on the next engine sync). The cast is the single, contained
acknowledgement of that lag; remove it once the contract is re-synced.

In `sanitizeOne`, the existing `slot === "command"` guard already rejects an
iframe in the command slot (it requires `action`). No `topbar.action` guard
exists, so add one so iframes can only live in `sidebar.section`:

```ts
  if (widget.kind === "iframe" && slot !== "sidebar.section")
    return drop(
      report,
      `contribution ${raw.id}: iframe widget only allowed in sidebar.section`,
    );
```

Add the capability sanitizer at the end of the file:

```ts
/** Validate an untrusted `capabilities` array → known values only. Never throws. */
export function sanitizeCapabilities(raw: unknown): PluginCapability[] {
  if (!Array.isArray(raw)) return [];
  const out: PluginCapability[] = [];
  for (const v of raw.slice(0, PLUGIN_CAPABILITY_VALUES.length)) {
    if (isCapability(v) && !out.includes(v)) out.push(v);
  }
  return out;
}
```

- [ ] **Step 4: Add the lockstep drift guard test**

Append to the same test file:

```ts
import { WIDGET_KINDS } from "./pluginContributions";
import { PLUGIN_WIDGET_KIND_VALUES } from "../contract/pluginValues";

it("host WIDGET_KINDS stays a superset of contract widget kinds", () => {
  for (const k of PLUGIN_WIDGET_KIND_VALUES) {
    expect(WIDGET_KINDS).toContain(k);
  }
});
```

(Until the engine sync, `PLUGIN_WIDGET_KIND_VALUES` lacks `"iframe"`; the test
still passes because it only asserts the host is a *superset*. After the sync it
keeps them aligned.)

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd web && pnpm test pluginContributions`
Expected: PASS (existing + 5 new).

- [ ] **Step 6: Per-task gate + commit**

```bash
cd web && pnpm test && pnpm typecheck && pnpm lint && pnpm format:check
git add web/src/client/pluginContributions.ts web/src/client/pluginContributions.test.ts
git commit -m "feat(plugins): sanitize Tier-3 iframe widgets and capabilities"
```

---

## Task 3: Plugin grants slice

**Files:**
- Create: `web/src/store/pluginGrantsSlice.ts`
- Modify: `web/src/store/store.ts`
- Test: `web/src/store/pluginGrantsSlice.test.ts`

- [ ] **Step 1: Write the failing test**

```ts
// web/src/store/pluginGrantsSlice.test.ts
import { beforeEach, describe, expect, it } from "vitest";
import {
  GRANTS_KEY,
  loadGrants,
  needsConsent,
  saveGrant,
  removeGrant,
} from "./pluginGrantsSlice";

beforeEach(() => localStorage.clear());

describe("plugin grants", () => {
  it("needsConsent when no grant exists", () => {
    expect(needsConsent({}, "p", "1.0.0", ["activeNote.write"])).toBe(true);
  });

  it("no consent needed when grant covers requested set and version matches", () => {
    const grants = { p: { version: "1.0.0", granted: ["activeNote.write", "notes.read"] as const } };
    expect(needsConsent(grants, "p", "1.0.0", ["activeNote.write"])).toBe(false);
  });

  it("requires consent again when version changed", () => {
    const grants = { p: { version: "1.0.0", granted: ["activeNote.write"] as const } };
    expect(needsConsent(grants, "p", "2.0.0", ["activeNote.write"])).toBe(true);
  });

  it("requires consent when the requested set expands beyond granted", () => {
    const grants = { p: { version: "1.0.0", granted: ["activeNote.read"] as const } };
    expect(needsConsent(grants, "p", "1.0.0", ["activeNote.read", "activeNote.write"])).toBe(true);
  });

  it("persists and reloads a grant", () => {
    const next = saveGrant({}, "p", "1.0.0", ["activeNote.write"]);
    expect(localStorage.getItem(GRANTS_KEY)).toContain("activeNote.write");
    expect(loadGrants()).toEqual(next);
  });

  it("removeGrant drops a plugin and persists", () => {
    const seeded = saveGrant({}, "p", "1.0.0", ["activeNote.write"]);
    const after = removeGrant(seeded, "p");
    expect(after.p).toBeUndefined();
    expect(loadGrants()).toEqual({});
  });

  it("loadGrants tolerates corrupt storage", () => {
    localStorage.setItem(GRANTS_KEY, "{not json");
    expect(loadGrants()).toEqual({});
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd web && pnpm test pluginGrantsSlice`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement the slice helpers + slice factory**

```ts
// web/src/store/pluginGrantsSlice.ts
import { isCapability, type PluginCapability } from "../client/pluginTier3";

export const GRANTS_KEY = "cairn.pluginGrants";

export type PluginGrant = { version: string; granted: PluginCapability[] };
export type PluginGrants = Record<string, PluginGrant>;

/** Pure: does this plugin need a fresh consent prompt? True when there is no
 *  grant, the version differs, or the requested set exceeds the granted set. */
export function needsConsent(
  grants: PluginGrants,
  pluginId: string,
  version: string,
  requested: readonly PluginCapability[],
): boolean {
  const g = grants[pluginId];
  if (!g || g.version !== version) return true;
  const have = new Set(g.granted);
  return requested.some((c) => !have.has(c));
}

/** Pure: return a new grants map with `pluginId` granted the requested set. */
export function saveGrant(
  grants: PluginGrants,
  pluginId: string,
  version: string,
  granted: readonly PluginCapability[],
): PluginGrants {
  const next: PluginGrants = { ...grants, [pluginId]: { version, granted: [...granted] } };
  persist(next);
  return next;
}

/** Pure: return a new grants map without `pluginId`. */
export function removeGrant(grants: PluginGrants, pluginId: string): PluginGrants {
  const next = { ...grants };
  delete next[pluginId];
  persist(next);
  return next;
}

export function loadGrants(): PluginGrants {
  try {
    const raw = localStorage.getItem(GRANTS_KEY);
    if (!raw) return {};
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== "object" || parsed === null) return {};
    const out: PluginGrants = {};
    for (const [id, g] of Object.entries(parsed as Record<string, unknown>)) {
      if (typeof g !== "object" || g === null) continue;
      const rec = g as Record<string, unknown>;
      if (typeof rec.version !== "string" || !Array.isArray(rec.granted)) continue;
      out[id] = { version: rec.version, granted: rec.granted.filter(isCapability) };
    }
    return out;
  } catch {
    return {};
  }
}

function persist(grants: PluginGrants): void {
  try {
    localStorage.setItem(GRANTS_KEY, JSON.stringify(grants));
  } catch {
    /* storage full / unavailable — grants degrade to session-only */
  }
}

// ── Store slice ────────────────────────────────────────────────────────────
export interface PluginGrantsState {
  pluginGrants: PluginGrants;
  grantPlugin(pluginId: string, version: string, granted: PluginCapability[]): void;
  revokePlugin(pluginId: string): void;
}

/** Slice factory wired into the root store with one import + one spread. */
export function createPluginGrantsSlice(
  set: (fn: (s: { pluginGrants: PluginGrants }) => Partial<{ pluginGrants: PluginGrants }>) => void,
): PluginGrantsState {
  return {
    pluginGrants: loadGrants(),
    grantPlugin(pluginId, version, granted) {
      set((s) => ({ pluginGrants: saveGrant(s.pluginGrants, pluginId, version, granted) }));
    },
    revokePlugin(pluginId) {
      set((s) => ({ pluginGrants: removeGrant(s.pluginGrants, pluginId) }));
    },
  };
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd web && pnpm test pluginGrantsSlice`
Expected: PASS (7 tests).

- [ ] **Step 5: Wire the slice into the root store (the only `store.ts` edit)**

In `web/src/store/store.ts`, add the import near the other store imports:

```ts
import {
  createPluginGrantsSlice,
  type PluginGrantsState,
} from "./pluginGrantsSlice";
```

Extend the state interface (`interface CairnState` ~line 111) to include the slice:

```ts
export interface CairnState extends PluginGrantsState {
```

In the returned state object (inside `createStore(...)`), add the single spread
alongside the other initial state — place it right after the `notice: null,`
field for locality:

```ts
      ...createPluginGrantsSlice(set),
```

(`set` here is the zustand setter already in scope; the slice's narrowed setter
type is structurally compatible.)

- [ ] **Step 6: Per-task gate + commit**

```bash
cd web && pnpm test && pnpm typecheck && pnpm lint && pnpm format:check
git add web/src/store/pluginGrantsSlice.ts web/src/store/pluginGrantsSlice.test.ts web/src/store/store.ts
git commit -m "feat(plugins): add Tier-3 plugin grants slice"
```

---

## Task 4: BrokerHost port + store adapter

**Files:**
- Create: `web/src/client/pluginBrokerHost.ts`
- Test: `web/src/client/pluginBrokerHost.test.ts`

- [ ] **Step 1: Write the failing test**

```ts
// web/src/client/pluginBrokerHost.test.ts
import { describe, expect, it, vi } from "vitest";
import { createStoreBrokerHost } from "./pluginBrokerHost";
import type { CairnClient } from "./types";

function fakeStore(state: Record<string, unknown>) {
  return {
    getState: () => state,
    subscribe: vi.fn(() => () => {}),
  } as never;
}

const stubClient = (over: Partial<CairnClient> = {}): CairnClient =>
  ({
    runQuery: vi.fn(),
    runCommand: vi.fn(),
    subscribe: vi.fn(() => () => {}),
    ...over,
  }) as unknown as CairnClient;

describe("store broker host", () => {
  it("reads the active note from open buffers", () => {
    const store = fakeStore({
      activePath: "a.md",
      openNotes: { "a.md": { contents: "hello" } },
    });
    const host = createStoreBrokerHost(store, stubClient());
    expect(host.activeNote()).toEqual({ path: "a.md", title: "a", text: "hello" });
  });

  it("returns null active note when none open", () => {
    const host = createStoreBrokerHost(fakeStore({ activePath: null, openNotes: {} }), stubClient());
    expect(host.activeNote()).toBeNull();
  });

  it("writeActiveNote routes through editBuffer", () => {
    const editBuffer = vi.fn();
    const host = createStoreBrokerHost(fakeStore({ editBuffer }), stubClient());
    host.writeActiveNote("new text");
    expect(editBuffer).toHaveBeenCalledWith("new text");
  });

  it("readNote queries get_note", async () => {
    const runQuery = vi.fn().mockResolvedValue({ type: "note", contents: "X" });
    const host = createStoreBrokerHost(fakeStore({}), stubClient({ runQuery }));
    expect(await host.readNote("b.md")).toEqual({ path: "b.md", text: "X" });
    expect(runQuery).toHaveBeenCalledWith({ type: "get_note", path: "b.md" });
  });

  it("search queries search and returns paths", async () => {
    const runQuery = vi
      .fn()
      .mockResolvedValue({ type: "search_results", results: [{ path: "a.md" }, { path: "b.md" }] });
    const host = createStoreBrokerHost(fakeStore({}), stubClient({ runQuery }));
    expect(await host.search("q")).toEqual([{ path: "a.md" }, { path: "b.md" }]);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd web && pnpm test pluginBrokerHost`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement the port + adapter**

```ts
// web/src/client/pluginBrokerHost.ts
// The broker's port to the host. Decoupling the broker from store internals
// keeps it unit-testable against a fake host (hexagonal boundary).
import type { StoreApi } from "zustand";
import type { CairnState } from "../store/store";
import type { JsonValue } from "../contract/serde_json/JsonValue";
import type { CairnClient } from "./types";

export interface BrokerHost {
  info(): { appVersion: string; theme: string; activePath: string | null };
  notice(text: string): void;
  activeNote(): { path: string; title: string; text: string } | null;
  writeActiveNote(text: string): void;
  readNote(path: string): Promise<{ path: string; text: string } | null>;
  search(query: string): Promise<Array<{ path: string }>>;
  invokeOwnCommand(plugin: string, command: string, args: JsonValue | null): Promise<void>;
  /** Fire `cb` whenever the active note path or contents change. Returns unsub. */
  subscribeActiveNote(cb: () => void): () => void;
}

const APP_VERSION =
  (import.meta as { env?: Record<string, string> }).env?.VITE_APP_VERSION ?? "0.0.0";

function stem(path: string): string {
  const base = path.split("/").pop() ?? path;
  return base.replace(/\.md$/i, "");
}

export function createStoreBrokerHost(
  store: StoreApi<CairnState>,
  client: CairnClient,
): BrokerHost {
  return {
    info() {
      const s = store.getState();
      return {
        appVersion: APP_VERSION,
        theme: s.settings?.theme ?? "dark",
        activePath: s.activePath,
      };
    },
    notice(text) {
      // Reuse the existing notice surface via a transient set; the store exposes
      // dismissNotice but seeding a notice is done by writing the field.
      store.setState({ notice: text });
    },
    activeNote() {
      const s = store.getState();
      const path = s.activePath;
      if (!path) return null;
      const buf = s.openNotes[path];
      if (!buf) return null;
      return { path, title: stem(path), text: buf.contents };
    },
    writeActiveNote(text) {
      store.getState().editBuffer(text);
    },
    async readNote(path) {
      const res = await client.runQuery({ type: "get_note", path });
      if (res.type !== "note") return null;
      return { path, text: res.contents };
    },
    async search(query) {
      const res = await client.runQuery({ type: "search", query });
      if (res.type !== "search_results") return [];
      return res.results.map((r) => ({ path: r.path }));
    },
    async invokeOwnCommand(plugin, command, args) {
      await store.getState().invokePlugin(plugin, command, args ?? undefined);
    },
    subscribeActiveNote(cb) {
      let prevPath = store.getState().activePath;
      let prevText = prevPath ? store.getState().openNotes[prevPath]?.contents : undefined;
      return store.subscribe((s) => {
        const text = s.activePath ? s.openNotes[s.activePath]?.contents : undefined;
        if (s.activePath !== prevPath || text !== prevText) {
          prevPath = s.activePath;
          prevText = text;
          cb();
        }
      });
    },
  };
}
```

Note: `store.setState({ notice })` directly seeds the existing notice field used
by `dismissNotice`. If `settings.theme` differs in shape, adjust `info().theme`
to match the real `Settings` type (verify against `store.ts`).

- [ ] **Step 4: Run test to verify it passes**

Run: `cd web && pnpm test pluginBrokerHost`
Expected: PASS (5 tests).

- [ ] **Step 5: Per-task gate + commit**

```bash
cd web && pnpm test && pnpm typecheck && pnpm lint && pnpm format:check
git add web/src/client/pluginBrokerHost.ts web/src/client/pluginBrokerHost.test.ts
git commit -m "feat(plugins): add broker host port and store adapter"
```

---

## Task 5: The broker

**Files:**
- Create: `web/src/client/pluginBroker.ts`
- Test: `web/src/client/pluginBroker.test.ts`

- [ ] **Step 1: Write the failing test**

```ts
// web/src/client/pluginBroker.test.ts
import { afterEach, describe, expect, it, vi } from "vitest";
import { createBroker } from "./pluginBroker";
import type { BrokerHost } from "./pluginBrokerHost";

function host(over: Partial<BrokerHost> = {}): BrokerHost {
  return {
    info: () => ({ appVersion: "1", theme: "dark", activePath: "a.md" }),
    notice: vi.fn(),
    activeNote: () => ({ path: "a.md", title: "a", text: "hi" }),
    writeActiveNote: vi.fn(),
    readNote: vi.fn(async () => ({ path: "b.md", text: "B" })),
    search: vi.fn(async () => [{ path: "a.md" }]),
    invokeOwnCommand: vi.fn(async () => {}),
    subscribeActiveNote: () => () => {},
    ...over,
  };
}

// A fake iframe window: messages the broker posts back land in `sent`.
function fakeFrame() {
  const sent: unknown[] = [];
  const win = { postMessage: (m: unknown) => sent.push(m) } as unknown as Window;
  return { win, sent };
}

function send(source: Window, data: unknown) {
  window.dispatchEvent(new MessageEvent("message", { source, data }));
}

let teardown: (() => void) | null = null;
afterEach(() => {
  teardown?.();
  teardown = null;
});

describe("plugin broker", () => {
  it("ignores messages from a foreign source", async () => {
    const h = host();
    const { win } = fakeFrame();
    const b = createBroker({ frame: win, plugin: "p", granted: new Set(), pluginCommands: new Set(), host: h });
    teardown = b.dispose;
    const other = { postMessage: vi.fn() } as unknown as Window;
    send(other, { t: "req", id: "1", method: "host.info" });
    await Promise.resolve();
    expect(h.notice).not.toHaveBeenCalled();
  });

  it("answers a silent method (host.info) without a grant", async () => {
    const h = host();
    const { win, sent } = fakeFrame();
    const b = createBroker({ frame: win, plugin: "p", granted: new Set(), pluginCommands: new Set(), host: h });
    teardown = b.dispose;
    send(win, { t: "req", id: "1", method: "host.info" });
    await vi.waitFor(() => expect(sent).toContainEqual({ t: "res", id: "1", ok: true, result: { appVersion: "1", theme: "dark", activePath: "a.md" } }));
  });

  it("denies a permissioned method without the grant", async () => {
    const h = host();
    const { win, sent } = fakeFrame();
    const b = createBroker({ frame: win, plugin: "p", granted: new Set(), pluginCommands: new Set(), host: h });
    teardown = b.dispose;
    send(win, { t: "req", id: "1", method: "activeNote.write", params: { text: "x" } });
    await vi.waitFor(() => expect(sent).toContainEqual({ t: "res", id: "1", ok: false, error: "denied" }));
    expect(h.writeActiveNote).not.toHaveBeenCalled();
  });

  it("allows a granted method", async () => {
    const h = host();
    const { win, sent } = fakeFrame();
    const b = createBroker({ frame: win, plugin: "p", granted: new Set(["activeNote.write"]), pluginCommands: new Set(), host: h });
    teardown = b.dispose;
    send(win, { t: "req", id: "1", method: "activeNote.write", params: { text: "new" } });
    await vi.waitFor(() => expect(h.writeActiveNote).toHaveBeenCalledWith("new"));
    expect(sent).toContainEqual({ t: "res", id: "1", ok: true, result: null });
  });

  it("confines command.invoke to the plugin's own commands", async () => {
    const h = host();
    const { win, sent } = fakeFrame();
    const b = createBroker({ frame: win, plugin: "p", granted: new Set(["command.invoke"]), pluginCommands: new Set(["mine"]), host: h });
    teardown = b.dispose;
    send(win, { t: "req", id: "1", method: "command.invoke", params: { command: "notMine" } });
    await vi.waitFor(() => expect(sent).toContainEqual({ t: "res", id: "1", ok: false, error: "unknown command" }));
    expect(h.invokeOwnCommand).not.toHaveBeenCalled();
  });

  it("drops messages beyond the inbound rate cap", async () => {
    const h = host({ info: vi.fn(() => ({ appVersion: "1", theme: "dark", activePath: null })) });
    const { win, sent } = fakeFrame();
    const b = createBroker({ frame: win, plugin: "p", granted: new Set(), pluginCommands: new Set(), host: h, rateMax: 3 });
    teardown = b.dispose;
    for (let i = 0; i < 10; i++) send(win, { t: "req", id: String(i), method: "host.info" });
    await Promise.resolve();
    expect((sent as Array<{ ok: boolean }>).filter((m) => m.ok).length).toBeLessThanOrEqual(3);
  });

  it("rejects a hanging request with a timeout", async () => {
    const h = host({ readNote: () => new Promise(() => {}) }); // never resolves
    const { win, sent } = fakeFrame();
    const b = createBroker({
      frame: win,
      plugin: "p",
      granted: new Set(["notes.read"]),
      pluginCommands: new Set(),
      host: h,
      requestTimeoutMs: 5,
    });
    teardown = b.dispose;
    send(win, { t: "req", id: "1", method: "notes.read", params: { path: "x.md" } });
    await vi.waitFor(() =>
      expect(sent).toContainEqual({ t: "res", id: "1", ok: false, error: "timeout" }),
    );
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd web && pnpm test pluginBroker`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement the broker**

```ts
// web/src/client/pluginBroker.ts
// Per-frame postMessage broker. The single wall between an untrusted sandboxed
// iframe and the host: authenticate by event.source, rate-cap, capability-gate,
// clamp params, dispatch to the BrokerHost port, reply within a timeout.
import {
  BROKER_RATE_MAX,
  BROKER_RATE_WINDOW_MS,
  BROKER_REQUEST_TIMEOUT_MS,
  CAPABILITY_OF,
  MAX_BROKER_STR,
  type BrokerMethod,
  type PluginCapability,
} from "./pluginTier3";
import type { BrokerHost } from "./pluginBrokerHost";
import type { JsonValue } from "../contract/serde_json/JsonValue";

export type BrokerOptions = {
  frame: Window;
  plugin: string;
  granted: ReadonlySet<PluginCapability>;
  pluginCommands: ReadonlySet<string>;
  host: BrokerHost;
  rateMax?: number;
  requestTimeoutMs?: number;
};

type Req = { t: "req"; id: string; method: string; params?: unknown };

const METHODS = new Set<string>(Object.keys(CAPABILITY_OF));

function isReq(x: unknown): x is Req {
  if (typeof x !== "object" || x === null) return false;
  const r = x as Record<string, unknown>;
  return r.t === "req" && typeof r.id === "string" && typeof r.method === "string";
}

function clampText(x: unknown): string {
  if (typeof x !== "string") return "";
  return x.length > MAX_BROKER_STR ? x.slice(0, MAX_BROKER_STR) : x;
}

export function createBroker(opts: BrokerOptions): { dispose: () => void } {
  const { frame, plugin, granted, pluginCommands, host } = opts;
  const rateMax = opts.rateMax ?? BROKER_RATE_MAX;
  const requestTimeoutMs = opts.requestTimeoutMs ?? BROKER_REQUEST_TIMEOUT_MS;

  // Inbound rate cap: at most `rateMax` accepted messages per rolling window.
  // Window resets on a timer, so a synchronous flood only lets `rateMax` through.
  let inWindow = 0;
  let windowTimer: number | null = null;
  function withinRateCap(): boolean {
    if (windowTimer === null) {
      windowTimer = window.setTimeout(() => {
        inWindow = 0;
        windowTimer = null;
      }, BROKER_RATE_WINDOW_MS);
    }
    inWindow += 1;
    return inWindow <= rateMax;
  }

  const reply = (id: string, body: { ok: true; result: JsonValue } | { ok: false; error: string }) =>
    frame.postMessage({ t: "res", id, ...body }, "*");

  // Per-request timeout: a hanging host method rejects with "timeout" instead of
  // leaving the plugin (and its pending request) stuck forever (spec §8.2).
  const withTimeout = (p: Promise<JsonValue>): Promise<JsonValue> =>
    Promise.race([
      p,
      new Promise<JsonValue>((_, rej) =>
        window.setTimeout(() => rej(new Error("timeout")), requestTimeoutMs),
      ),
    ]);

  const dispatch = async (method: BrokerMethod, params: Record<string, unknown>): Promise<JsonValue> => {
    switch (method) {
      case "host.info":
        return host.info();
      case "ui.notice":
        host.notice(clampText(params.text));
        return null;
      case "activeNote.read":
        return host.activeNote();
      case "activeNote.subscribe":
        // Subscription wiring is owned by IframeHost (it forwards events); the
        // broker just acknowledges the subscribe request.
        return null;
      case "activeNote.write":
        host.writeActiveNote(clampText(params.text));
        return null;
      case "notes.read":
        return host.readNote(clampText(params.path));
      case "notes.search":
        return host.search(clampText(params.query));
      case "command.invoke": {
        const command = clampText(params.command);
        if (!pluginCommands.has(command)) throw new Error("unknown command");
        await host.invokeOwnCommand(plugin, command, (params.args as JsonValue) ?? null);
        return null;
      }
      default:
        throw new Error("unknown method");
    }
  };

  const onMessage = (e: MessageEvent) => {
    if (e.source !== frame) return; // 1. identity (origin is null → use source)
    const msg = e.data;
    if (!isReq(msg)) return; // 2. shape-validate, drop malformed
    if (!withinRateCap()) return; // 3. inbound rate cap (flood guard)

    if (!METHODS.has(msg.method)) {
      reply(msg.id, { ok: false, error: "unknown method" });
      return;
    }
    const method = msg.method as BrokerMethod;
    const cap = CAPABILITY_OF[method];
    if (cap && !granted.has(cap)) {
      reply(msg.id, { ok: false, error: "denied" }); // 4. broker is the wall
      return;
    }
    const params = (typeof msg.params === "object" && msg.params !== null ? msg.params : {}) as Record<string, unknown>;
    // 5. dispatch under a timeout; never let a rejection escape.
    void withTimeout(dispatch(method, params)).then(
      (result) => reply(msg.id, { ok: true, result }),
      (err: unknown) => reply(msg.id, { ok: false, error: err instanceof Error ? err.message : "error" }),
    );
  };

  window.addEventListener("message", onMessage);
  return {
    dispose() {
      window.removeEventListener("message", onMessage);
      if (windowTimer !== null) window.clearTimeout(windowTimer);
    },
  };
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd web && pnpm test pluginBroker`
Expected: PASS (6 tests). If the rate-window arithmetic is awkward, refactor to a
simple `if (++countInWindow > rateMax) return;` reset every `rateMax` accepted
messages — the test only asserts the cap holds.

- [ ] **Step 5: Per-task gate + commit**

```bash
cd web && pnpm test && pnpm typecheck && pnpm lint && pnpm format:check
git add web/src/client/pluginBroker.ts web/src/client/pluginBroker.test.ts
git commit -m "feat(plugins): add capability-gated postMessage broker"
```

---

## Task 6: PermissionPrompt component

**Files:**
- Create: `web/src/components/plugins/PermissionPrompt.tsx`
- Test: `web/src/components/plugins/PermissionPrompt.test.tsx`

- [ ] **Step 1: Write the failing test**

```tsx
// web/src/components/plugins/PermissionPrompt.test.tsx
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { PermissionPrompt } from "./PermissionPrompt";

describe("PermissionPrompt", () => {
  it("lists grouped risks (HIGH first) and hides silent capabilities", () => {
    render(
      <PermissionPrompt
        name="Word Linter"
        capabilities={["command.invoke", "activeNote.read", "activeNote.write", "notes.read"]}
        onAllow={() => {}}
        onDeny={() => {}}
      />,
    );
    const rows = screen.getAllByRole("listitem").map((li) => li.textContent);
    expect(rows[0]).toMatch(/Modify the current note/); // high first
    expect(rows.join()).toMatch(/Read across your whole vault/);
    expect(rows.join()).not.toMatch(/command\.invoke/); // silent
  });

  it("fires onAllow / onDeny", () => {
    const onAllow = vi.fn();
    const onDeny = vi.fn();
    render(<PermissionPrompt name="P" capabilities={["activeNote.write"]} onAllow={onAllow} onDeny={onDeny} />);
    fireEvent.click(screen.getByRole("button", { name: /allow/i }));
    fireEvent.click(screen.getByRole("button", { name: /don't run/i }));
    expect(onAllow).toHaveBeenCalled();
    expect(onDeny).toHaveBeenCalled();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd web && pnpm test PermissionPrompt`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement**

```tsx
// web/src/components/plugins/PermissionPrompt.tsx
import { groupCapabilities, type PluginCapability } from "../../client/pluginTier3";

/**
 * Grouped, plain-language consent gate shown before a Tier-3 iframe plugin is
 * mounted for the first time (or after it expands its capability set). All-or-
 * nothing: Allow grants the whole declared set; Don't run leaves it unmounted.
 */
export function PermissionPrompt({
  name,
  capabilities,
  onAllow,
  onDeny,
}: {
  name: string;
  capabilities: PluginCapability[];
  onAllow: () => void;
  onDeny: () => void;
}) {
  const rows = groupCapabilities(capabilities);
  return (
    <div className="rounded border border-border bg-surface-2 p-3 text-sm">
      <p className="mb-2 text-text">
        <strong>{name}</strong> wants to:
      </p>
      {rows.length > 0 ? (
        <ul className="mb-3 space-y-1">
          {rows.map((r) => (
            <li
              key={r.label}
              className={r.severity === "high" ? "text-text" : "text-muted"}
            >
              {r.severity === "high" ? "⚠️ " : "• "}
              {r.label}
            </li>
          ))}
        </ul>
      ) : (
        <p className="mb-3 text-muted">Run with no special access.</p>
      )}
      <div className="flex gap-2">
        <button
          type="button"
          onClick={onAllow}
          className="rounded border border-accent bg-accent px-3 py-1 text-xs text-on-accent"
        >
          Allow
        </button>
        <button
          type="button"
          onClick={onDeny}
          className="rounded border border-border px-3 py-1 text-xs text-muted hover:text-text"
        >
          Don't run
        </button>
      </div>
    </div>
  );
}
```

> Implementer note: match the exact Tailwind tokens used elsewhere
> (`text-on-accent` may be `text-bg` in this repo — grep an existing primary
> button, e.g. in `SettingsDialog`, and reuse its classes).

- [ ] **Step 4: Run test to verify it passes**

Run: `cd web && pnpm test PermissionPrompt`
Expected: PASS (2 tests).

- [ ] **Step 5: Per-task gate + commit**

```bash
cd web && pnpm test && pnpm typecheck && pnpm lint && pnpm format:check
git add web/src/components/plugins/PermissionPrompt.tsx web/src/components/plugins/PermissionPrompt.test.tsx
git commit -m "feat(plugins): add Tier-3 permission consent prompt"
```

---

## Task 7: IframeHost — sandboxed iframe + lifecycle

**Files:**
- Create: `web/src/components/plugins/IframeHost.tsx`
- Test: `web/src/components/plugins/IframeHost.test.tsx`

- [ ] **Step 1: Write the failing test**

```tsx
// web/src/components/plugins/IframeHost.test.tsx
import { render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { IframeHost } from "./IframeHost";
import type { BrokerHost } from "../../client/pluginBrokerHost";

const host: BrokerHost = {
  info: () => ({ appVersion: "1", theme: "dark", activePath: null }),
  notice: vi.fn(),
  activeNote: () => null,
  writeActiveNote: vi.fn(),
  readNote: async () => null,
  search: async () => [],
  invokeOwnCommand: async () => {},
  subscribeActiveNote: () => () => {},
};

describe("IframeHost", () => {
  it("renders a locked-down sandboxed iframe with the plugin html as srcdoc", () => {
    render(
      <IframeHost
        plugin="p"
        html="<p id='x'>hi</p>"
        height={200}
        granted={new Set()}
        pluginCommands={new Set()}
        host={host}
      />,
    );
    const frame = screen.getByTitle("plugin:p") as HTMLIFrameElement;
    expect(frame.getAttribute("sandbox")).toBe("allow-scripts");
    expect(frame.getAttribute("srcdoc")).toContain("hi");
    expect(frame.style.height).toBe("200px");
  });

  it("shows the WidgetError fallback if the handshake times out", async () => {
    render(
      <IframeHost
        plugin="p"
        html="<p>no handshake</p>"
        height={null}
        granted={new Set()}
        pluginCommands={new Set()}
        host={host}
        handshakeTimeoutMs={10}
      />,
    );
    await waitFor(() => expect(screen.getByText(/didn't start/i)).toBeInTheDocument());
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd web && pnpm test IframeHost`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement**

```tsx
// web/src/components/plugins/IframeHost.tsx
import { useEffect, useRef, useState } from "react";
import {
  BROKER_HANDSHAKE_TIMEOUT_MS,
  DEFAULT_IFRAME_HEIGHT,
  type PluginCapability,
} from "../../client/pluginTier3";
import { createBroker } from "../../client/pluginBroker";
import type { BrokerHost } from "../../client/pluginBrokerHost";

type Phase = "handshaking" | "ready" | "error";

/**
 * Mounts an untrusted plugin's HTML in a null-origin sandboxed iframe and owns
 * the per-frame broker for its whole lifetime. The sandbox attr is hard-coded
 * (never allow-same-origin) so the frame stays opaque-origin + network-blocked.
 */
export function IframeHost({
  plugin,
  html,
  height,
  granted,
  pluginCommands,
  host,
  handshakeTimeoutMs = BROKER_HANDSHAKE_TIMEOUT_MS,
}: {
  plugin: string;
  html: string;
  height: number | null;
  granted: ReadonlySet<PluginCapability>;
  pluginCommands: ReadonlySet<string>;
  host: BrokerHost;
  handshakeTimeoutMs?: number;
}) {
  const ref = useRef<HTMLIFrameElement>(null);
  const [phase, setPhase] = useState<Phase>("handshaking");

  useEffect(() => {
    const frame = ref.current?.contentWindow;
    if (!frame) return;

    const broker = createBroker({ frame, plugin, granted, pluginCommands, host });

    // Forward active-note change events to the frame (for activeNote.subscribe) —
    // ONLY if the plugin holds the read grant, so pushed events can't leak note
    // content past the capability gate.
    const unsubNote = granted.has("activeNote.read")
      ? host.subscribeActiveNote(() => {
          const note = host.activeNote();
          frame.postMessage({ t: "event", topic: "activeNote", payload: note }, "*");
        })
      : () => {};

    // Handshake: the frame posts {t:"req",method:"__handshake"}; we ack {t:"ready"}.
    const onHandshake = (e: MessageEvent) => {
      if (e.source !== frame) return;
      const d = e.data as { t?: string; method?: string } | null;
      if (d?.t === "req" && d.method === "__handshake") {
        setPhase("ready");
        frame.postMessage({ t: "ready", capabilities: [...granted] }, "*");
      }
    };
    window.addEventListener("message", onHandshake);

    const timer = window.setTimeout(() => {
      setPhase((p) => (p === "handshaking" ? "error" : p));
    }, handshakeTimeoutMs);

    return () => {
      window.clearTimeout(timer);
      window.removeEventListener("message", onHandshake);
      unsubNote();
      broker.dispose();
    };
    // Re-mount the frame (and broker) whenever identity/grant/html changes.
  }, [plugin, html, granted, pluginCommands, host, handshakeTimeoutMs]);

  if (phase === "error") {
    return (
      <button
        type="button"
        onClick={() => setPhase("handshaking")}
        className="text-xs text-faint italic hover:text-muted"
      >
        plugin didn't start — retry
      </button>
    );
  }

  return (
    <iframe
      ref={ref}
      title={`plugin:${plugin}`}
      sandbox="allow-scripts"
      srcDoc={html}
      style={{ width: "100%", height: `${height ?? DEFAULT_IFRAME_HEIGHT}px`, border: "none" }}
    />
  );
}
```

> Implementer note: in jsdom the iframe `contentWindow` exists but won't execute
> scripts; the handshake-timeout test relies on that (no handshake ever arrives).
> The retry button reuses the `WidgetError` visual; if you prefer, import and
> render `WidgetError` from `SlotRenderer` instead of duplicating the markup —
> extract it to its own file if sharing.

- [ ] **Step 4: Run test to verify it passes**

Run: `cd web && pnpm test IframeHost`
Expected: PASS (2 tests).

- [ ] **Step 5: Per-task gate + commit**

```bash
cd web && pnpm test && pnpm typecheck && pnpm lint && pnpm format:check
git add web/src/components/plugins/IframeHost.tsx web/src/components/plugins/IframeHost.test.tsx
git commit -m "feat(plugins): add sandboxed IframeHost with lifecycle + broker"
```

---

## Task 8: Wire the iframe branch into WidgetView (gate prompt vs host)

**Files:**
- Modify: `web/src/components/plugins/WidgetView.tsx`
- Test: `web/src/components/plugins/WidgetView.test.tsx` (create if absent)

This is the integration point: for an `iframe` widget, read the plugin's grant +
declared capabilities, and render either the `PermissionPrompt` (no grant yet) or
the `IframeHost` (granted). The plugin summary (for `name`, `version`,
`capabilities`, own command ids) is looked up from the store.

- [ ] **Step 1: Write the failing test**

```tsx
// web/src/components/plugins/WidgetView.test.tsx
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { WidgetView } from "./WidgetView";
import { cairnStore } from "../../app/cairnStore";

describe("WidgetView iframe branch", () => {
  it("shows the permission prompt when the plugin has no grant", () => {
    cairnStore.setState({
      plugins: [
        { id: "p", name: "Word Linter", version: "1", commands: [], contributions: [], capabilities: ["activeNote.write"] },
      ],
      pluginGrants: {},
    } as never);
    render(
      <WidgetView
        plugin="p"
        widget={{ kind: "iframe", html: "<p>x</p>", height: 200 } as never}
      />,
    );
    expect(screen.getByText(/wants to:/i)).toBeInTheDocument();
    expect(screen.getByText(/Modify the current note/i)).toBeInTheDocument();
  });

  it("mounts the iframe once the plugin is granted", () => {
    cairnStore.setState({
      plugins: [
        { id: "p", name: "Word Linter", version: "1", commands: [], contributions: [], capabilities: ["activeNote.write"] },
      ],
      pluginGrants: { p: { version: "1", granted: ["activeNote.write"] } },
    } as never);
    render(
      <WidgetView plugin="p" widget={{ kind: "iframe", html: "<p>x</p>", height: 200 } as never} />,
    );
    expect(screen.getByTitle("plugin:p")).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd web && pnpm test WidgetView`
Expected: FAIL — no `iframe` branch; falls through to `default → null`.

- [ ] **Step 3: Implement the branch**

In `web/src/components/plugins/WidgetView.tsx`, extend imports:

```ts
import { useCairn, useActions } from "../../app/cairnStore";
import { useShallow } from "zustand/react/shallow";
import { needsConsent } from "../../store/pluginGrantsSlice";
import { isCapability, type PluginCapability } from "../../client/pluginTier3";
import { IframeHost } from "./IframeHost";
import { PermissionPrompt } from "./PermissionPrompt";
import { createStoreBrokerHost } from "../../client/pluginBrokerHost";
import { cairnStore } from "../../app/cairnStore";
import { makeBackend } from "../../app/makeBackend";
```

Add the `iframe` case before `default:`:

```tsx
    case "iframe":
      return <IframeWidget plugin={plugin} widget={widget} />;
```

Then add the helper component at the bottom of the file:

```tsx
// Memoize the broker host once (it closes over the singleton store + client).
const brokerHost = createStoreBrokerHost(cairnStore, makeBackend().client);

function IframeWidget({
  plugin,
  widget,
}: {
  plugin: string;
  widget: { kind: "iframe"; html: string; height: number | null };
}) {
  const { grantPlugin } = useActions();
  const summary = useCairn(useShallow((s) => s.plugins.find((p) => p.id === plugin)));
  const grants = useCairn((s) => s.pluginGrants);

  const caps: PluginCapability[] = (summary?.capabilities ?? []).filter(isCapability);
  const version = summary?.version ?? "0";

  if (needsConsent(grants, plugin, version, caps)) {
    return (
      <PermissionPrompt
        name={summary?.name ?? plugin}
        capabilities={caps}
        onAllow={() => grantPlugin(plugin, version, caps)}
        onDeny={() => {
          /* leave unmounted; user can re-trigger by reloading plugins */
        }}
      />
    );
  }

  const commandIds = new Set((summary?.commands ?? []).map((c) => c.id));
  return (
    <IframeHost
      plugin={plugin}
      html={widget.html}
      height={widget.height}
      granted={new Set(caps)}
      pluginCommands={commandIds}
      host={brokerHost}
    />
  );
}
```

> Implementer note: `makeBackend()` returns a fresh `{client}`; to avoid a second
> client instance, export the bound `client` from `cairnStore.ts` (it already
> constructs one) and import it here instead of calling `makeBackend()` again.
> Small refactor: add `export const cairnClient = client;` in `cairnStore.ts`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd web && pnpm test WidgetView`
Expected: PASS (2 tests).

- [ ] **Step 5: Per-task gate + commit**

```bash
cd web && pnpm test && pnpm typecheck && pnpm lint && pnpm format:check
git add web/src/components/plugins/WidgetView.tsx web/src/components/plugins/WidgetView.test.tsx web/src/app/cairnStore.ts
git commit -m "feat(plugins): render Tier-3 iframe widgets behind consent gate"
```

---

## Task 9: PluginsPanel — grant status + revoke

**Files:**
- Modify: `web/src/components/plugins/PluginsPanel.tsx`
- Test: `web/src/components/plugins/PluginsPanel.test.tsx` (create if absent)

- [ ] **Step 1: Write the failing test**

```tsx
// web/src/components/plugins/PluginsPanel.test.tsx
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { PluginsPanel } from "./PluginsPanel";
import { cairnStore } from "../../app/cairnStore";

describe("PluginsPanel grants", () => {
  it("shows a Revoke action for a granted plugin and revokes it", () => {
    cairnStore.setState({ pluginGrants: { p: { version: "1", granted: ["activeNote.write"] } } } as never);
    render(
      <PluginsPanel
        plugins={[{ id: "p", name: "Word Linter", version: "1", commands: [], contributions: [], capabilities: ["activeNote.write"] }]}
      />,
    );
    const btn = screen.getByRole("button", { name: /revoke/i });
    fireEvent.click(btn);
    expect(cairnStore.getState().pluginGrants.p).toBeUndefined();
  });

  it("shows no Revoke for a plugin without granted permissions", () => {
    cairnStore.setState({ pluginGrants: {} } as never);
    render(
      <PluginsPanel
        plugins={[{ id: "q", name: "Static", version: "1", commands: [], contributions: [], capabilities: null }]}
      />,
    );
    expect(screen.queryByRole("button", { name: /revoke/i })).toBeNull();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd web && pnpm test PluginsPanel`
Expected: FAIL — no Revoke control.

- [ ] **Step 3: Implement**

In `web/src/components/plugins/PluginsPanel.tsx`, add grant-aware UI. Inside the
per-plugin row rendering, add:

```tsx
import { useActions, useCairn } from "../../app/cairnStore";
// ...inside the component:
  const grants = useCairn((s) => s.pluginGrants);
  const { revokePlugin } = useActions();
// ...inside each plugin row, after name + version:
  {grants[p.id] && grants[p.id].granted.length > 0 && (
    <div className="mt-1 flex items-center gap-2 text-xs text-muted">
      <span>Granted: {grants[p.id].granted.join(", ")}</span>
      <button
        type="button"
        onClick={() => revokePlugin(p.id)}
        className="rounded border border-border px-2 py-0.5 hover:border-accent"
      >
        Revoke
      </button>
    </div>
  )}
```

> Implementer note: read the current `PluginsPanel.tsx` structure first; insert
> the block into the existing per-plugin `<li>`/row without disturbing the
> read-only command list already rendered there.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd web && pnpm test PluginsPanel`
Expected: PASS (2 tests).

- [ ] **Step 5: Per-task gate + commit**

```bash
cd web && pnpm test && pnpm typecheck && pnpm lint && pnpm format:check
git add web/src/components/plugins/PluginsPanel.tsx web/src/components/plugins/PluginsPanel.test.tsx
git commit -m "feat(plugins): show grant status and revoke in PluginsPanel"
```

---

## Task 10: Demo plugin in MockClient + e2e

**Files:**
- Modify: `web/src/client/mock.ts`
- Create: `web/e2e/tier3-plugins.spec.ts`

- [ ] **Step 1: Seed a demo iframe plugin in the mock**

In `web/src/client/mock.ts`, find where `list_plugins` returns `PluginSummary[]`
(grep `list_plugins`), and add a word-count iframe plugin to the seeded list:

```ts
{
  id: "wordcount",
  name: "Word Count",
  version: "1.0.0",
  commands: [],
  contributions: [
    {
      id: "wc-panel",
      slot: "sidebar.section",
      title: "Word Count",
      icon: "info",
      order: 100,
      widget: {
        kind: "iframe",
        height: 120,
        html:
          "<body style='font:13px sans-serif;margin:6px'>" +
          "<div id='n'>…</div>" +
          "<script>" +
          "parent.postMessage({t:'req',id:'h',method:'__handshake'},'*');" +
          "function ask(){parent.postMessage({t:'req',id:'r',method:'activeNote.read'},'*')}" +
          "addEventListener('message',function(e){" +
          "var d=e.data;" +
          "if(d&&d.t==='ready'){ask()}" +
          "if(d&&d.t==='res'&&d.id==='r'&&d.ok){var t=d.result?d.result.text:'';" +
          "document.getElementById('n').textContent=(t.trim()?t.trim().split(/\\s+/).length:0)+' words'}" +
          "if(d&&d.t==='event'&&d.topic==='activeNote'){ask()}" +
          "});" +
          "parent.postMessage({t:'event-sub',topic:'activeNote'},'*');" +
          "</script></body>",
      },
    },
  ],
  capabilities: ["activeNote.read"],
} as never,
```

(The `as never` bridges the not-yet-synced contract type; remove after the engine
sync widens `PluginSummary`/`PluginWidget`.)

- [ ] **Step 2: Run the unit suite to confirm nothing regressed**

Run: `cd web && pnpm test`
Expected: PASS — the mock change is additive.

- [ ] **Step 3: Write the e2e spec**

```ts
// web/e2e/tier3-plugins.spec.ts
import { expect, test } from "@playwright/test";

test("Tier-3 word-count plugin: consent → read → live update", async ({ page }) => {
  await page.goto("/");
  // Word Count is an iframe plugin requesting activeNote.read → consent prompt.
  await expect(page.getByText("Word Count").first()).toBeVisible();
  await expect(page.getByText(/wants to:/i)).toBeVisible();
  await expect(page.getByText(/Read the current note/i)).toBeVisible();
  await page.getByRole("button", { name: /allow/i }).click();

  // Iframe mounts; open a note and assert the count reflects its content.
  const frame = page.frameLocator('iframe[title="plugin:wordcount"]');
  await page.getByText(/\.md$/).first().click(); // open any seeded note
  await expect(frame.getByText(/\d+ words/)).toBeVisible();

  // Revoke in Settings unmounts the iframe.
  await page.getByRole("button", { name: /settings/i }).click();
  await page.getByRole("button", { name: /revoke/i }).click();
  await page.getByRole("button", { name: /close/i }).click();
  await expect(page.locator('iframe[title="plugin:wordcount"]')).toHaveCount(0);
});
```

> Implementer note: selectors (Settings button name, note-list item, close
> button) must match the real app — open the running app and adjust. If the
> seeded mock has no notes, create one first or pick an existing seeded note
> name. Keep the assertions (consent shown → allow → count visible → revoke →
> gone); adapt only the selectors.

- [ ] **Step 4: Run the e2e**

Run: `cd web && pnpm test:e2e tier3-plugins` (or the repo's e2e command — check
`package.json`)
Expected: PASS.

- [ ] **Step 5: Full gate + commit**

```bash
cd web && pnpm test && pnpm typecheck && pnpm lint && pnpm format:check && pnpm test:e2e
git add web/src/client/mock.ts web/e2e/tier3-plugins.spec.ts
git commit -m "feat(plugins): seed Tier-3 demo plugin and add e2e"
```

---

## Final verification

- [ ] From `web/`: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check && pnpm test:e2e` — all green.
- [ ] Manually confirm in the running app: Word Count shows a consent prompt; Allow mounts it; opening/editing a note updates the count; Settings → Revoke removes it; reloading re-prompts.
- [ ] Grep for stray `as never` / `as PluginWidget` casts introduced for the contract lag and leave a `// TODO(contract-sync): drop cast after engine adds iframe variant` marker on each, so the post-sync cleanup is mechanical.

## Post-engine-sync follow-up (separate, tiny PR — NOT this plan)

Once `tau-rs/cairn` adds the `iframe` `PluginWidget` variant, `PluginSummary.capabilities`,
and the `PluginCapability` enum and the vendored contract is re-synced here:
- Remove the `as PluginWidget` / `as never` casts (Tasks 2, 8, 10).
- Repoint `IframeWidget`/`PluginCapability` host types to the generated contract
  if desired (or keep the host copies as the trust-boundary allow-list, matching
  `pluginContributions.ts`'s existing pattern — preferred).
- The lockstep test (Task 2) will then actively keep host + contract aligned.

## 6b backlog (documented in the spec, not built here)

Custom `plugin-sandbox://` protocol + multi-file bundles + persistent per-plugin
storage; `panel.main` dock slot for rich editors. Additive to everything above.
