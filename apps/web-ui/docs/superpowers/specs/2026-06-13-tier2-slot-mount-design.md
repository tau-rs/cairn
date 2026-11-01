# Cairn UI Plugin Extensibility — Tier-2 Declarative Slot-Mount

**Status:** design-approved (brainstorm complete; awaiting spec review → implementation plan).
**Track:** Tier-2 of the 3-tier extensibility model (Tier-1 themes shipped).
**Repos:** `tau-rs/cairn-web-ui` (this repo, frontend) + `tau-rs/cairn` (engine; the author owns it, ADMIN).

> **Provenance note.** A first draft was produced by a multi-agent design workflow that ran against
> a faulty premise I supplied — *"plugins are compiled-in Rust, no runtime, no third-party developer
> story."* Probing the real engine (`tau-rs/cairn`) disproved that. This spec is the **corrected**
> design: it keeps the workflow's solid Tier-2 frontend mechanics, slot taxonomy, and TDD plan, and
> **rewrites the architecture sections** (north-star, composition, trust, capabilities, roadmap)
> against cairn's *actual* plugin runtime.

---

## 0. The real plugin architecture (verified, not assumed)

cairn **already has a mature plugin runtime**. Verified in `tau-rs/cairn`:

- **Out-of-process subprocess model.** The daemon spawns each plugin as a child process and speaks
  **JSON-RPC over NDJSON stdio** (`cairn-plugin-protocol`). This is the VS Code extension-host / LSP /
  Neovim-RPC family — **not** WASM, **not** compiled-in.
- **Authoring SDK** (`cairn-plugin-sdk`): `Plugin::new(id, version).command(id, title, handler).on_event(h).run()`.
  Host callbacks: `read_note` / `write_note` / `list_notes` / `search` / `delete_note`.
- **Initialize handshake** (`cairn-plugin-protocol::InitializeResult { name, version, commands: Vec<CommandDecl> }`).
- **Capability model** — three capabilities, host-gated in `required_cap()`:
  `fs:read` (read/search/list), `fs:write` (write/delete), `events` (pushed cairn events).
  Self-declared in the plugin manifest; gating narrows the host-callback RPC surface only.
- **Trust model** (`[plugins].trusted` in `cairn.toml`): full-trust subprocess, **no OS sandbox** — a
  deliberate, documented choice (`docs/.../2026-06-11-cairn-plugin-trust-design.md`). Approving a plugin
  == trusting its author to run as you.
- **A working example** (`cairn-plugin-example`) with host integration tests (`tests/host.rs`).
- **Contract pipeline.** Frontend contract types in `web/src/contract/*.ts` are **ts-rs-generated in the
  engine** (`crates/cairn-contract/bindings`), vendored byte-identically by `scripts/sync-contract.sh`,
  pinned by `CONTRACT_SOURCE_COMMIT` in `web/src/contract/source.ts`, and **drift-checked** by
  `scripts/check-contract-drift.sh` (clones the engine at the pinned commit, re-syncs, fails on ANY
  `git status --porcelain` diff incl. untracked). **You cannot hand-author contract `.ts` in this repo.**
- **`list_plugins` mapping.** `cairn-service` maps engine-internal `PluginInfo` → contract `PluginSummary`
  (the same place `CommandDecl` becomes `PluginCommandSummary`). `cairn-plugin-protocol`/`-sdk`/`-app` are
  **contract-independent** (no `cairn-contract` dep), so plugin-declared data is **mirrored** in the
  protocol crate and **mapped** into the contract — never shared by direct type reuse.

**What this changes versus the workflow draft:** the "where does logic live / who writes plugins / Tier-2
isn't DX" tension is **already resolved by the shipped runtime.** Tier-2 is *not* introducing a runtime; it
**extends the protocol these existing plugins already speak** so they can contribute UI. No WASM, no
producer-swap abstraction, no anti-spoofing layer.

---

## 1. Problem, goals, honest framing

### 1.1 Problem
Plugins can declare **commands** (surfaced into ⌘K), but cannot contribute **UI** into the shell. Tier-2 adds
a **declarative contribution layer**: a plugin declares "render this host-owned widget in this named slot;
clicking it fires that command" — as **contract-validated data the trusted React shell renders.** A plugin
never ships a component or markup.

### 1.2 Goals
- **G1.** Ship the declarative contribution layer over the **existing** subprocess protocol + contract.
- **G2.** Reuse the **existing** command round-trip (`invoke_plugin_command`) for interactivity — no new
  transport.
- **G3.** Preserve the webview trust boundary (S4/S5/U5): **no plugin JS, no plugin markup, no
  `dangerouslySetInnerHTML`/`href`/`src`/`eval`** in the main webview. Plugin output is always **data,
  validated + clamped at ingest**, rendered by host-owned components.
- **G4.** Fully testable today: frontend under vitest + MockClient; engine under its existing host
  integration-test harness.
- **G5.** A real day-one caller: extend `cairn-plugin-example` to contribute UI, proving the path
  end-to-end (not mock-only).

### 1.3 Honest framing
Tier-2 is the **contribution/manifest layer** (VS Code `contributes`, Zed `extension.toml`, Eclipse e4
fragments). The **logic+UI authoring DX already exists** via the subprocess SDK. Tier-2 lets those plugins
*place UI*; it does not invent the developer story — that shipped.

### 1.4 Decisions locked in brainstorm
| # | Decision |
|---|---|
| Q1 | (A) Author owns `tau-rs/cairn`; Phase 0 (engine) + Phase 1 (frontend) are one owned, ordered unit. |
| Q2 | (b) Day-one caller = extend `cairn-plugin-example` (a `sidebar.section`/`list` + a `topbar.action`). |
| Q3 | (A) North-star = the shipped **full-trust subprocess** runtime. `sanitizeContributions` is **defense-in-depth**, not a security boundary. |
| Q4 | Tier-2 adds **no** capability vocabulary; reuse existing `fs:read`/`fs:write`/`events`. |
| Q5 | (i) Forward-compat = **drop + surface a count**; reserve an additive `fallback?` for a future second widget-kind generation. |

---

## 2. North-star (the runtime exists; this is alignment, not new build)

- **Runtime:** out-of-process subprocess + JSON-RPC. Already shipped. Tier-2 extends its `initialize`
  handshake with `contributions`. No new runtime, no WASM.
- **Trust:** full-trust subprocess, user-approved via `[plugins].trusted`, no OS sandbox (Q3). Therefore
  descriptor data comes from **approved-but-arms-length** code — the sanitizer guards against *buggy* and
  *tainted-reflection* plugins (e.g. one echoing note text into a label), not adversaries.
- **Capabilities:** the existing `fs:read`/`fs:write`/`events` set (Q4). Tier-2 adds none: declaring a
  contribution needs no capability; a widget firing a command runs the plugin handler whose host callbacks
  are *already* gated.
- **Future tiers:** Tier-3 (custom pixels) remains a **sandboxed iframe / separate Tauri webview** with a
  narrowed `postMessage`→contract broker — the escape hatch for anything the widget vocabulary can't
  express. Unchanged by this correction.

**Deleted from the workflow draft:** the WASM/Wasmtime runtime, the Rust↔WIT mapping, the "swappable
producer" abstraction, anti-spoofing/provenance-binding, the invented capability vocabulary, the
`effect:read|write` discriminator as a *security* control (kept only as an optional future **UX** affordance —
§6).

---

## 3. Tier-2 deliverable

The whole layer is **data over the existing transports.** Two halves: **engine** (Phase 0) declares + carries
contributions; **frontend** (Phase 1) validates + renders them.

### 3.1 New types (defined in `cairn-contract`, ts-rs-generated → frontend)

```ts
// PluginSlot.ts (generated) — closed first-cut taxonomy (§4)
export type PluginSlot = "sidebar.section" | "topbar.action" | "command";

// PluginIcon.ts (generated) — CLOSED enum of bundled icons; never a string/URL/SVG
export type PluginIcon =
  | "tag" | "search" | "note" | "folder" | "link" | "star" | "info" | "play";

// PluginWidget.ts (generated) — closed, host-renderable vocabulary; first cut: text/action/list
import type { JsonValue } from "./serde_json/JsonValue";
import type { PluginIcon } from "./PluginIcon";
export type PluginWidget =
  | { kind: "text";   text: string; muted?: boolean }
  | { kind: "action"; label: string; icon?: PluginIcon; command: string; args?: JsonValue }
  | { kind: "list";   items: Array<PluginListItem> };
export type PluginListItem =
  { id: string; label: string; icon?: PluginIcon; command?: string; args?: JsonValue };

// PluginContribution.ts (generated)
import type { PluginSlot } from "./PluginSlot";
import type { PluginWidget } from "./PluginWidget";
import type { PluginIcon } from "./PluginIcon";
export type PluginContribution = {
  id: string;            // stable within a plugin (React key + dedup)
  slot: PluginSlot;
  widget: PluginWidget;
  title?: string;        // header where the slot owns chrome
  icon?: PluginIcon;
  order?: number;        // sort hint; HOST owns final order (§3.6.1)
  // fallback?: PluginWidget  // RESERVED (Q5) — added with the 2nd widget-kind generation
};
```

**Plus engine-emitted runtime value arrays** (ts-rs emits *types*, erased at runtime; the §7 lockstep test
needs iterable values):
```ts
export const PLUGIN_SLOT_VALUES = ["sidebar.section","topbar.action","command"] as const;
export const PLUGIN_WIDGET_KIND_VALUES = ["text","action","list"] as const;
export const PLUGIN_ICON_VALUES = ["tag","search","note","folder","link","star","info","play"] as const;
```

**`PluginSummary` gains one field** (the only edit to an existing contract type):
```ts
export type PluginSummary = {
  id: string; name: string; version: string;
  commands: Array<PluginCommandSummary>;        // RETAINED (§8)
  contributions: Array<PluginContribution>;      // NEW — #[serde(default)] => [] for old plugins
};
```

### 3.2 The three-layer engine flow (mirrors the existing `CommandDecl` path)

Because the protocol/app crates are contract-independent, contributions are **mirrored + mapped**, not shared:

```
cairn-plugin-example (subprocess)
  └─ InitializeResult.contributions: Vec<protocol::PluginContribution>   ← cairn-plugin-protocol mirror
        │  JSON-RPC initialize (NDJSON stdio)
        ▼
cairn-infra plugin_host  → engine-internal PluginInfo.contributions       ← cairn-app
        │  engine.list_plugins()
        ▼
cairn-service  maps PluginInfo → contract PluginSummary.contributions      ← cairn-contract (ts-rs)
        │  Query::ListPlugins → QueryResponse::Plugins
        ▼
web/src/contract  (vendored)  → frontend
```

**Phase-0 engine edits (`tau-rs/cairn`):**
- `cairn-contract`: add `PluginWidget`/`PluginSlot`/`PluginIcon`/`PluginContribution` + ts-rs derives +
  the runtime value arrays; add `contributions: Vec<PluginContribution>` (`#[serde(default)]`) to
  `PluginSummary`.
- `cairn-plugin-protocol`: mirror the contribution types (plain serde); add `contributions` (`#[serde(default)]`)
  to `InitializeResult`.
- `cairn-plugin-sdk`: a `.contribution(PluginContribution)` builder beside `.command(...)`, threaded into
  the `initialize` reply.
- engine-internal `PluginInfo` (`cairn-app`): carry `contributions`.
- `cairn-service`: extend the `PluginInfo → PluginSummary` map (and the protocol→PluginInfo map in
  `plugin_host`) to pass contributions through.
- `cairn-plugin-example`: declare the day-one contributions (Q2).
- Then in this repo: `scripts/sync-contract.sh ../cairn`, bump `CONTRACT_SOURCE_COMMIT`, commit regenerated TS.

> **Open plan detail (not a blocker):** whether to mirror the contribution types in the protocol crate or
> add a `cairn-contract` dependency to it. Recommend **mirror + map** (matches the existing decoupling;
> `CommandDecl`↔`PluginCommandSummary` already does this).

### 3.3 Frontend validator — `web/src/client/pluginContributions.ts` (NEW, own module)

Distinct from the thin, throw-on-drift `contractGuards.ts`. Posture: **drop-unknown + clamp, never throw**
(forward-compat). Header comment states it is the descriptor sanitizer, not the S5 outer guard.

```ts
import { PLUGIN_SLOT_VALUES, PLUGIN_WIDGET_KIND_VALUES, PLUGIN_ICON_VALUES } from "../contract";
const PLUGIN_SLOTS = ["sidebar.section","topbar.action","command"] as const;
const WIDGET_KINDS = ["text","action","list"] as const;
const MAX_CONTRIBS_PER_PLUGIN = 64, MAX_LIST_ITEMS = 200, MAX_STR = 2_000, MAX_ARGS_BYTES = 16_384;
export type SanitizeReport = { kept: number; dropped: number; reasons: string[] };
export function sanitizeContributions(raw: unknown, report?: SanitizeReport): PluginContribution[] { /* … */ }
```
- drops contribution if `slot ∉ PLUGIN_SLOTS` or `widget.kind ∉ WIDGET_KINDS` or missing `id` (records reason);
- a `slot:"command"` contribution **must** carry a `widget.kind:"action"` (else dropped — §3.5.1);
- clamps strings to `MAX_STR`, truncates `list.items` to `MAX_LIST_ITEMS`, drops a contribution whose `args`
  JSON exceeds `MAX_ARGS_BYTES`, caps the array to `MAX_CONTRIBS_PER_PLUGIN`;
- coerces out-of-enum `icon` to `undefined`.

**XSS barrier (stated + tested):** React text-child auto-escaping. No widget kind uses
`dangerouslySetInnerHTML`/`href`/`src`. Caps justified by Q3 framing: a *buggy* approved plugin must not
render-DoS the UI; a plugin reflecting note content is *tainted data regardless of author trust*.

### 3.4 `SlotRenderer` — the single trusted render path (`web/src/components/plugins/SlotRenderer.tsx`, NEW)

```tsx
export function SlotRenderer({ slot }: { slot: PluginSlot }) {
  const here = useCairn(useShallow((s) => s.pluginContributions[slot] ?? [])); // grouped+sorted (§3.6)
  return <>{here.map(({ plugin, c, epoch }) => (
    <ErrorBoundary key={`${plugin}:${c.id}:${epoch}`} fallback={(reset) => <WidgetError onRetry={reset} />}>
      <WidgetView plugin={plugin} contribution={c} />
    </ErrorBoundary>
  ))}</>;
}
```
- **Per-widget `ErrorBoundary` with a LOCAL fallback** (`<WidgetError/>`, a faint "unavailable" stub) — never
  the app-level reload card.
- **Re-fetch remounts a poisoned widget:** the boundary `key` includes a monotonic `epoch` bumped on every
  `loadPlugins`; a fresh fetch changes the key → remount → caught-error cleared. `<WidgetError onRetry>` wires
  the boundary `reset` for manual recovery.
- `WidgetView` switches on `widget.kind` → `TextWidget`/`ActionWidget`/`ListWidget`; routes every `command`
  through `useActions().invokePlugin(plugin, command, args)`; `default → null`.
- `pluginIcon.tsx` (NEW): `Record<PluginIcon, ReactNode>` (compile-time exhaustive) → bundled icons; runtime
  drift guarded by the icon lockstep test (§7).

### 3.5 Shell mount points (against real files; `Shell.tsx`/`App.tsx` untouched)

| Slot | File | Placement |
|---|---|---|
| `sidebar.section` | `web/src/components/Sidebar.tsx` | `<SlotRenderer slot="sidebar.section" />` after `<TagsPanel/>` |
| `topbar.action` | `web/src/components/TopBar.tsx` | `<SlotRenderer slot="topbar.action" />` in the action cluster; the `action` widget's `icon` resolves to a node passed to the existing `IconButton` |
| `command` | `web/src/components/plugins/pluginCommands.ts` | `toPaletteCommands` also flattens `slot:"command"` contributions (§3.5.1) |

#### 3.5.1 The `command` slot (fully specified)
- A `command`-slot contribution **must** carry `widget.kind:"action"` (enforced in the sanitizer).
- `toPaletteCommands` normalizes **two** sources into one `PaletteCommand[]`:
  - legacy `commands[]` `{id,title}` → `{ id:"plugin:{p}/{c}", title, run:()=>invokePlugin(p,c,null) }`;
  - `command` contribution `{label,command,args?}` → `{ id:"plugin:{p}/{command}", title:label, run:()=>invokePlugin(p,command,args ?? null) }`.
  - Same `plugin:{id}/{cmd}` id scheme (verified `pluginCommands.ts:4/29`); **dedupe by that id, contribution
    wins** (richer: carries `args`/`icon`). `args` travels as structured data on the `PaletteCommand`, **not**
    encoded in the id — the fix that makes the `invokePlugin(…, args)` widening reach the palette path.

### 3.6 Store / state (`web/src/store/store.ts`, `web/src/app/cairnStore.ts`)
- Add **derived** `pluginContributions: Record<PluginSlot, Array<{plugin:string; c:PluginContribution; epoch:number}>>`
  to `CairnState` (lives next to `plugins`, **not** in the `ui` slice — it's plugin data, not shell chrome).
- Add monotonic `pluginEpoch: number` (drives the `ErrorBoundary` remount).
- Extend `loadPlugins()` (store.ts:703): after `set({plugins})`, run each summary's `contributions` through
  `sanitizeContributions`, **group by slot**, **sort** (§3.6.1), stamp `epoch`, carry owning `plugin` id,
  `set({pluginContributions, pluginEpoch})`.
- `loadCairn` reset block: `pluginContributions` → empty; `pluginEpoch` stays monotonic (never reset).
- **Widen `invokePlugin`** (store.ts:144/713) `(plugin, command)` → `(plugin, command, args?: JsonValue = null)`.
  **Pure frontend** — the generated `invoke_plugin_command` already carries `args: JsonValue` (store.ts:719 sets
  `args:null` today); only the optional TS param widens. Back-compat (every existing caller compiles).
- No `ui`-slice change.

#### 3.6.1 Ordering (store-owned, total, testable)
Sort applied once during the `loadPlugins` grouping: (1) ascending `order` (missing = `+∞`); (2) tie → ascending
`plugin` id; (3) tie → ascending contribution `id`. Pure function; asserted in §7.

### 3.7 Visibility / dynamic content — deferred (seams named)
- **No `when` DSL** in the first cut; pane-level conditional render handles note-scoping (`BacklinksPane`
  already renders only with an active note). Future: a closed predicate enum
  (`always`/`note_open`/`tag_active`/`search_active`), host-evaluated over an allow-list of store fields,
  fail-closed — added with its first caller.
- **No live-pull / dynamic content.** When built: a **dedicated read-only Query** (never the side-effecting
  `invoke_plugin_command`), re-validated through `sanitizeContributions` on each refresh, with explicit new
  store wiring (not bolted onto the existing `onEvent` switch, which doesn't handle `reindexed` and has no
  component event seam). Not in the first cut.

### 3.8 Forward-compat (Q5)
Unknown `slot`/`widget.kind` → dropped by `sanitizeContributions` (`WidgetView default → null` backstop).
Drop is recorded in `SanitizeReport`; `loadPlugins` logs one structured line when `dropped > 0`; `PluginsPanel`
surfaces a **count** ("N contributions not rendered — unsupported by this version"). A dropped *contribution*
does **not** disable that plugin's *commands* (separate `commands` array). The additive `fallback?: PluginWidget`
field is **reserved**, added with the second widget-kind generation.

---

## 4. Slot taxonomy — first cut

| Slot | File | Justification | Ship? |
|---|---|---|---|
| **`command`** | `pluginCommands.ts` | Generalizes an already-half-built seam (commands → ⌘K). Lowest risk. | **Yes** |
| **`sidebar.section`** | `Sidebar.tsx` | Richest, most-wanted surface (word count, recent notes). Vertical stack → zero layout risk. | **Yes** |
| **`topbar.action`** | `TopBar.tsx` | Cheapest high-value (icon button → command). | **Yes** |
| `backlinks.section` | `BacklinksPane.tsx` | Note-scoped output. Low demand; trivial later. | Defer |
| `statusbar.item` | needs new footer in `Shell.tsx` | Lowest value, highest structural cost (no footer exists). | Defer |
| `editor.toolbar` | `EditorPane.tsx` | Highest-churn/risk surface. Add once `SlotRenderer` is proven. | Defer |
| `settings.section` | `SettingsDialog.tsx` | Needs a settings-schema vocabulary — separate design. | Defer |
| custom pixels | — | **Permanently Tier-3 (iframe)**, never a Tier-2 widget kind. | → Tier-3 |

**First cut = 3 slots, 3 widget kinds.** Exercises static text, the command round-trip, and list-as-data
without touching hard surfaces.

---

## 5. Composition — why Tier-2 isn't throwaway (simplified by reality)
The producer is **already well-defined**: a subprocess plugin declaring `contributions` in its `initialize`
reply. The frontend's only input is **validated JSON over the contract** — so the contract → sanitizer → store →
`SlotRenderer` arm is fixed and producer-agnostic. A Tier-3 iframe plugin later **also** declares a Tier-2
contribution (a `topbar.action` "open my panel") to place its launcher; only its pixels live in the sandboxed
iframe. **No producer-swap machinery, no WASM bridge** — those were artifacts of the wrong premise.

The durable nucleus: `PluginContribution` + closed `PluginWidget` + `PluginIcon` enum + `SlotRenderer`/`WidgetView`
+ the `invokePlugin(…, args)` widening + `sanitizeContributions`. Everything else (live-pull, `when`, deferred
slots, `fallback?`) is built with its first caller.

---

## 6. Trust / threat model (full-trust subprocess — Q3)
- **No new Tauri capability.** Contributions are inert descriptors; commands ride the existing
  `invoke_plugin_command`/`run_query` IPC. `src-tauri/capabilities/*.json` unchanged.
- **`send_command` now carries plugin-controlled `args: JsonValue`** — mitigated by ingest clamp
  (`MAX_ARGS_BYTES`), closed icon enum, and the engine routing/validating every `invoke_plugin_command` against
  the named plugin (handlers gated by `fs:*`).
- **`sanitizeContributions` = defense-in-depth, not a security boundary** (Q3). It guards *buggy* plugins
  (render-DoS caps), *tainted reflection* (note content echoed into a label is data regardless of author
  trust → always escaped/clamped), and *forward-compat* (drop-unknown).
- **Click→write nicety (deferred, UX not security):** a fired widget command may mutate the vault (same path as
  `stamp`). Under full trust this is acceptable (the user approved the plugin). Reserved future UX affordance: an
  engine-asserted `effect:"read"|"write"` hint on command-bearing widgets so the shell can style/confirm writes.
  **Not** a Tier-2 deliverable; **not** a security control.

---

## 7. TDD plan
Frontend — **author tests first** (vitest + @testing-library/react + jsdom + MockClient):

**`pluginContributions.test.ts`** — keep a well-formed contribution; drop unknown slot/kind, missing `id`;
drop a `command`-slot non-`action`; clamp over-long `text`, truncate huge `list`, drop oversized `args`, cap the
array; coerce out-of-enum `icon`; populate `SanitizeReport`; **slot/kind lockstep** asserts
`PLUGIN_SLOTS ⊇ PLUGIN_SLOT_VALUES` and `WIDGET_KINDS ⊇ PLUGIN_WIDGET_KIND_VALUES` (engine runtime arrays).
**`pluginIcon.test.ts`** — registry keys ⊇ `PLUGIN_ICON_VALUES`.
**XSS** — `text:"<script>"` renders inert.
**`SlotRenderer`/`WidgetView`** — text renders; action click calls `invokePlugin(p,c,args)`; list item click fires
its command/args; unknown kind → `null` no throw; a throwing widget → local `<WidgetError/>` (not the reload
card); `onRetry` calls `reset`; **re-fetch (epoch bump) remounts a poisoned widget**; ordered by
`order`/`(plugin,id)`.
**Shell mounts** — `sidebar.section` renders after `TagsPanel`; `topbar.action` renders an `IconButton` with the
resolved icon; no contributions → no extra DOM (existing tests green).
**Palette** (`pluginCommands.test.ts`) — a `command`-slot `action` → `PaletteCommand` whose `run` threads `args`;
legacy + contribution with same id dedupe (contribution wins).
**Store** (`store.test.ts`, MockClient) — `loadPlugins` populates grouped+sorted `pluginContributions` with
`plugin`+`epoch`; malformed dropped, siblings survive, no error toast; `invokePlugin('demo','stamp',{n:1})` sends
`args:{n:1}` (extend mock `stamp` handler to echo `args`); old `PluginSummary` w/o contributions → empty;
`loadCairn` resets contributions, `pluginEpoch` stays monotonic.
**Mock fixtures** (`mock.ts`): extend `demo` with a `sidebar.section`/`list`, a `topbar.action`/`action`, a
`command`/`action`; seed a second no-contrib plugin; `stamp` echoes `args`.

Engine — under the existing host integration-test harness (`tests/host.rs` style): a plugin declaring
`contributions` in `initialize` round-trips through `plugin_host` → `PluginInfo` → `list_plugins` →
`PluginSummary.contributions`; the `cairn-plugin-example` day-one contributions are asserted end-to-end.

---

## 8. Back-compat & migration
- `PluginSummary.contributions` and `InitializeResult.contributions` are **additive, `#[serde(default)] => []`** —
  an old plugin or old engine yields no slots, identical to today.
- `commands[]` untouched; `toPaletteCommands`/`parsePluginCommandId` keep working; `command`-slot reconciliation
  de-dupes by the existing two-part id (no flag day: commands-only, contributions-only, or both all work).
- `invokePlugin` widening is back-compat + pure frontend.
- `PluginsPanel.tsx` (in `SettingsDialog`, read-only inventory) gains a contributions count + the
  "N not rendered" count.
- No persisted-state migration (contributions recomputed from `list_plugins` each load).

---

## 9. Roadmap
- **Phase 0 — engine PR (`tau-rs/cairn`):** contract types + runtime arrays + `PluginSummary.contributions`;
  protocol mirror + `InitializeResult.contributions`; SDK `.contribution()`; `PluginInfo` + `cairn-service`
  mapping; `cairn-plugin-example` day-one contributions; engine host tests. Then sync + bump in this repo.
- **Phase 1 — Tier-2 frontend (this spec):** `pluginContributions.ts`; `SlotRenderer`/`WidgetView`/`pluginIcon.tsx`;
  mounts in `Sidebar`/`TopBar`/`pluginCommands.ts`; store `pluginContributions`+`pluginEpoch`+`invokePlugin(args)`;
  `mock.ts` fixtures; `PluginsPanel` counts; full test suite. **Scope cut:** 3 slots, 3 kinds, flat lists, no
  `when`, no live-pull, no `fallback?`, `Shell.tsx`/`App.tsx`/`contractGuards.ts`/Tauri ACL untouched.
- **Phase 2 — deferred slots + dynamic:** `backlinks.section`, `statusbar.item` (+ footer + its widget kind),
  `editor.toolbar`, `settings.section`; closed `when` enum; read-only live-pull Query + refresh wiring; `tree`
  widget (depth/breadth caps); `fallback?`.
- **Phase 3 — Tier-3 iframe:** sandboxed iframe / separate Tauri webview + narrowed broker for custom pixels.

> Phase 0 and Phase 1 are **one ordered unit** (the drift gate forbids hand-authored contract files), both owned
> by the author. No external dependency.

---

## 10. Risks & deferrals
**Risks / mitigations:** two validation philosophies → own module + header (sanitizer vs S5 guard); render-DoS →
hard caps; icon injection → closed enum; icon/slot/kind drift → runtime lockstep tests vs engine arrays;
app-reload-from-one-widget → local `ErrorBoundary` fallback; poisoned-widget-stuck → `pluginEpoch` remount +
`onRetry`; newer-plugin-blanks → drop + `PluginsPanel` count; closed-enum release treadmill (every new
slot/kind/icon = engine enum + array + sync + sanitizer list + lockstep test + `WidgetView` arm + fixture) —
acceptable while the surface is small.

**Explicit deferrals (built with first caller):** `when` DSL; live-pull + its read-only Query + refresh wiring;
`tree` + `statusbar.item` widget kind; `backlinks.section`/`statusbar.item`/`editor.toolbar`/`settings.section`;
`fallback?`; the `effect:read|write` UX hint; Tier-3 iframe + broker.

**No remaining open questions** — all five resolved (§1.4).
