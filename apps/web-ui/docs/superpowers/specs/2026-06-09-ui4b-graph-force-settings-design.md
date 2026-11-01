# Cairn Web UI — UI‑4b: Graph Force Settings Design Spec

**Date:** 2026-06-09
**Status:** approved, ready for implementation planning
**Sub-project:** UI‑4b — second of the UI‑4 graph cycles. Adds a live force-tuning
panel to the Obsidian-style graph from UI‑4a.
**Builds on:** UI‑4a (`react-force-graph-2d` canvas in `GraphView.tsx`, `fgRef`),
the graphite design system, and the UI‑1 primitives (`IconButton`, etc.).

---

## 1. Purpose

Give the graph an Obsidian-style **Forces** panel: a gear-toggled overlay with
sliders that live-tune the d3-force simulation, a freeze toggle, and a reset —
values persisted across sessions.

### Locked decisions (from brainstorming)

| Decision | Choice |
|---|---|
| Sliders | **Four:** Center force, Repel force, Link force, Link distance. |
| Freeze toggle | **Yes** — pins the layout static (hover still works); resume re-heats. |
| Reset | A **Reset** button restoring defaults. |
| Persistence | **localStorage** — restored on load (the app's current `Settings` are in-memory; this adds light graph-only persistence). |
| Mechanism | Imperative — set d3 force params via `fgRef` + `d3ReheatSimulation()`. |
| Panel | Gear `IconButton` (top-right of the graph) toggles an overlay panel. |
| Out of scope | Obsidian's "Animate" timeline (chronological reveal) — separate future cycle. Display settings (node size / line thickness / text-fade thresholds) — not requested. |

### Non-goals (deferred)

- The "Animate" timeline (needs per-note timestamps + a scrubber).
- Display sliders (node size, link width, label-fade thresholds) — UI‑4a's values stay hardcoded.
- Color groups (UI‑4c) and local-graph mode (UI‑4d).
- Per-graph (per-vault) profiles — a single global force setting.

---

## 2. Architecture

Contained to the graph view. **No store/host/contract changes.** Force settings
live in `GraphView` state + a small localStorage module; they are NOT added to the
global `Settings` store (graph-only, and the store isn't persisted).

```
web/src/components/graph/forceSettings.ts          NEW (pure-ish) — types + defaults + localStorage load/save.
web/src/components/graph/forceSettings.test.ts     NEW — unit tests (load/save/defaults/clamp).
web/src/components/graph/GraphForcesPanel.tsx       NEW (presentational) — sliders + freeze + reset.
web/src/components/graph/GraphForcesPanel.test.tsx  NEW — Testing Library (DOM sliders/buttons).
web/src/components/GraphView.tsx                    MODIFY — gear toggle + panel + apply forces via fgRef + freeze + persist.
```

### `forceSettings.ts` (data + persistence)

```ts
export interface ForceSettings {
  center: number;       // forceCenter strength      (0 .. 1),     default 0.05
  repel: number;        // forceManyBody strength     (-800 .. 0),  default -150
  linkForce: number;    // forceLink strength         (0 .. 1),     default 0.7
  linkDistance: number; // forceLink distance (px)    (10 .. 300),  default 80
  frozen: boolean;      // pin layout static          default false
}

export const DEFAULT_FORCE_SETTINGS: ForceSettings = {
  center: 0.05, repel: -150, linkForce: 0.7, linkDistance: 80, frozen: false,
};
export const FORCE_RANGES = {
  center: { min: 0, max: 1, step: 0.01 },
  repel: { min: -800, max: 0, step: 10 },
  linkForce: { min: 0, max: 1, step: 0.05 },
  linkDistance: { min: 10, max: 300, step: 5 },
};

clampForceSettings(s): ForceSettings        // clamp each numeric to its range
loadForceSettings(): ForceSettings          // localStorage["cairn.graph.forces"] → parsed+clamped, else defaults
saveForceSettings(s: ForceSettings): void   // JSON.stringify → localStorage (guarded)
```

- `loadForceSettings` tolerates missing/corrupt/partial JSON → returns defaults
  (merging known keys, clamped). `saveForceSettings` swallows storage errors
  (private mode / quota). Defaults/ranges are the tuning surface (refined at the
  visual check); the numbers above are sensible starting points.

### `GraphForcesPanel.tsx` (presentational)

`props: { settings: ForceSettings; onChange: (next: ForceSettings) => void; onReset: () => void }`.
Renders, graphite-styled, a titled "Forces" panel: four rows each with a label,
a value readout, and `<input type="range">` (min/max/step from `FORCE_RANGES`); a
**Freeze** checkbox/toggle bound to `settings.frozen`; a **Reset** button. Each
control calls `onChange({...settings, <field>: value})` (or `onReset`). No graph
or store coupling — pure props in/out.

### `GraphView.tsx` (wiring)

- `const [forces, setForces] = useState(loadForceSettings)`; `const [panelOpen, setPanelOpen] = useState(false)`.
- A gear `IconButton` (absolute, top-right of the graph container) toggles `panelOpen`; when open, render `<GraphForcesPanel settings={forces} onChange={apply} onReset={reset} />` (also top-right, below the gear).
- `apply(next)`: `setForces(next)`, `saveForceSettings(next)`, then push to the sim via `fgRef`:
  - `fgRef.current.d3Force("charge")?.strength(next.repel)`
  - `fgRef.current.d3Force("link")?.strength(next.linkForce).distance(next.linkDistance)`
  - center: ensure a center force exists; `fgRef.current.d3Force("center")?.strength(next.center)`
  - if `next.frozen` → pin all nodes (`node.fx = node.x; node.fy = node.y`); else clear pins (`node.fx = node.fy = undefined`).
  - then `fgRef.current.d3ReheatSimulation()` (skip reheat while frozen, or reheat then it settles with pins).
- On mount and whenever the simulation (re)initializes (e.g. data change / first engine tick), re-apply `forces` so a reload/rebuild reflects persisted values. `reset()` = `apply(DEFAULT_FORCE_SETTINGS)`.
- Freeze interaction with UI‑4a's `autoPauseRedraw={false}`: pinning nodes keeps the render loop + hover highlight working while the layout is static (preferred over pausing the whole animation, which would stop hover repaint).

---

## 3. Testing

- **Unit (Vitest):**
  - `forceSettings`: `clampForceSettings` clamps each field to its range;
    `loadForceSettings` returns defaults when localStorage is empty/corrupt/partial
    and a clamped round-trip when valid; `saveForceSettings` writes JSON and
    swallows errors (mock `localStorage` + a throwing setItem).
  - `GraphForcesPanel`: each slider fires `onChange` with the updated field;
    the freeze toggle flips `frozen`; **Reset** fires `onReset`; value readouts
    reflect `settings`.
- **e2e (Playwright):** extend the graph test — after toggling to Graph, click the
  gear and assert the Forces panel (e.g. a slider with an accessible name) is
  visible. (The force *effect* on the canvas is physics-driven and manual-visual,
  as in UI‑4a.)
- **Manual/visual check:** dragging each slider visibly changes the layout and
  re-settles; Freeze holds it static (hover still highlights); Reset restores;
  values survive a reload.
- All existing tests stay green; Tauri unaffected.

---

## 4. Files & dependencies

| File | Change |
|---|---|
| `web/src/components/graph/forceSettings.ts` (+ test) | **New.** Types, defaults, ranges, localStorage load/save/clamp. |
| `web/src/components/graph/GraphForcesPanel.tsx` (+ test) | **New.** Presentational sliders + freeze + reset. |
| `web/src/components/GraphView.tsx` | **Modify.** Gear toggle + panel + apply-via-fgRef + freeze + persist + re-apply on (re)init. |
| `web/e2e/skeleton.spec.ts` | **Modify.** Assert the gear opens the panel. |

No new dependencies. No store/host/contract changes.

---

## 5. Risks

- **`d3Force` availability/timing:** the forces (`charge`/`link`/`center`) must
  exist on the sim before tuning; apply after the graph has initialized, and
  guard with `?.` (react-force-graph creates `charge`/`link` by default; ensure
  `center` is present or add it). Re-apply on data (re)init so persisted values
  survive a rebuild.
- **Freeze vs hover:** freeze by pinning nodes (`fx/fy`), NOT by pausing the
  animation loop — pausing would also stop hover repaint (UI‑4a uses
  `autoPauseRedraw={false}`).
- **Link strength override:** setting a constant `forceLink.strength` overrides
  d3's default per-node auto-strength; acceptable (it's what the slider controls),
  just note the feel differs from UI‑4a's default — defaults chosen to approximate
  the current look.
- **localStorage in Tauri/private mode:** wrap read/write in try/catch → fall back
  to defaults / no-op; never throw.
- **Canvas testability:** the panel (DOM) is unit-tested; the force *effect* is
  manual-visual, consistent with UI‑4a's accepted limitation.
- **Tuning:** default values + ranges are starting points, refined at the visual
  check; not final.
