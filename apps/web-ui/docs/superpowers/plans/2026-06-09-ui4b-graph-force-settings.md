# UI‑4b Graph Force Settings Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an Obsidian-style Forces panel to the graph: a gear-toggled overlay with four live sliders (center / repel / link force / link distance), a freeze toggle, and a reset — values persisted to localStorage.

**Architecture:** A pure `forceSettings.ts` (types + defaults + ranges + localStorage load/save/clamp) and a presentational `GraphForcesPanel.tsx` (sliders/freeze/reset), wired into `GraphView` which applies the settings to the d3 simulation imperatively via `fgRef` (`d3Force(...).strength/.distance` + `d3ReheatSimulation()`), pins nodes to freeze, and persists changes.

**Tech Stack:** React 18 + TypeScript, `react-force-graph-2d`, Vite, Vitest + Testing Library, Playwright.

**Spec:** `docs/superpowers/specs/2026-06-09-ui4b-graph-force-settings-design.md`

**Working conventions (read before starting):**
- Run all `pnpm` from `web/`. Git from repo root or `git -C /Users/titouanlebocq/code/cairn-ui`.
- Per-task gate before commit: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check`. `pnpm build` + `pnpm e2e` where a task says so. Run `pnpm format` + re-stage if format fails.
- e2e on port 5273 (configured). Current: 131 unit, 8 e2e, all green.
- **Relevant existing code:**
  - `GraphView.tsx` (from UI-4a): `<ForceGraph2D ref={fgRef}>` inside a `<div ref={containerRef} className="h-full w-full">` (rendered only when `size.width/height > 0`). `fgRef` is `ForceGraphMethods<RFNode, GLink>`. `RFNode = { id; label; degree; x?; y? }`. `data` = `useMemo(() => buildGraphData(...), [nodes, edges])`. Uses `autoPauseRedraw={false}`.
  - `IconButton` (`web/src/components/ui/IconButton.tsx`): `{ label: string; children: ReactNode; ...buttonProps }` (so `onClick` is passed through).
  - jsdom (Vitest) provides `localStorage`; clear it between tests.
- `react-force-graph` ref methods used: `fgRef.current.d3Force(name)` returns the d3 force (or undefined) — `.strength()`/`.distance()` are chainable setters; `fgRef.current.d3ReheatSimulation()` re-runs the sim. Guard everything with `?.`.
- Canvas can't be unit-tested under jsdom — unit-test the pure module + the DOM panel; the force *effect* is manual-visual (UI-4a's accepted limitation).

---

## File Structure

| File | Responsibility |
|---|---|
| `web/src/components/graph/forceSettings.ts` | Types, `DEFAULT_FORCE_SETTINGS`, `FORCE_RANGES`, `clampForceSettings`, `loadForceSettings`, `saveForceSettings`. |
| `web/src/components/graph/forceSettings.test.ts` | Unit tests (defaults/round-trip/corrupt/clamp/save-error). |
| `web/src/components/graph/GraphForcesPanel.tsx` | Presentational panel — sliders + freeze + reset. |
| `web/src/components/graph/GraphForcesPanel.test.tsx` | Testing Library tests. |
| `web/src/components/GraphView.tsx` | Gear toggle + panel + apply-via-fgRef + freeze + persist. |
| `web/e2e/skeleton.spec.ts` | Assert the gear opens the panel. |

---

## Task 1: forceSettings (data + persistence)

**Files:**
- Create: `web/src/components/graph/forceSettings.ts`
- Create: `web/src/components/graph/forceSettings.test.ts`

- [ ] **Step 1: Write the failing tests**

Create `web/src/components/graph/forceSettings.test.ts`:

```ts
import { describe, it, expect, beforeEach, vi } from "vitest";
import {
  DEFAULT_FORCE_SETTINGS,
  clampForceSettings,
  loadForceSettings,
  saveForceSettings,
} from "./forceSettings";

beforeEach(() => localStorage.clear());

describe("clampForceSettings", () => {
  it("clamps each numeric field to its range", () => {
    const c = clampForceSettings({
      center: 9, repel: 9999, linkForce: -1, linkDistance: 5, frozen: true,
    });
    expect(c.center).toBe(1);
    expect(c.repel).toBe(0);
    expect(c.linkForce).toBe(0);
    expect(c.linkDistance).toBe(10);
    expect(c.frozen).toBe(true);
  });
});

describe("loadForceSettings", () => {
  it("returns defaults when localStorage is empty", () => {
    expect(loadForceSettings()).toEqual(DEFAULT_FORCE_SETTINGS);
  });
  it("returns defaults when the stored value is corrupt JSON", () => {
    localStorage.setItem("cairn.graph.forces", "{not json");
    expect(loadForceSettings()).toEqual(DEFAULT_FORCE_SETTINGS);
  });
  it("merges partial stored values over defaults and clamps", () => {
    localStorage.setItem(
      "cairn.graph.forces",
      JSON.stringify({ repel: -9999, linkDistance: 120 }),
    );
    const s = loadForceSettings();
    expect(s.repel).toBe(-800); // clamped
    expect(s.linkDistance).toBe(120);
    expect(s.center).toBe(DEFAULT_FORCE_SETTINGS.center); // default
  });
  it("round-trips a saved settings object", () => {
    const s = { ...DEFAULT_FORCE_SETTINGS, repel: -300, frozen: true };
    saveForceSettings(s);
    expect(loadForceSettings()).toEqual(s);
  });
});

describe("saveForceSettings", () => {
  it("swallows storage errors", () => {
    const spy = vi
      .spyOn(Storage.prototype, "setItem")
      .mockImplementation(() => {
        throw new Error("quota");
      });
    expect(() => saveForceSettings(DEFAULT_FORCE_SETTINGS)).not.toThrow();
    spy.mockRestore();
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `pnpm test -- forceSettings`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement `forceSettings.ts`**

Create `web/src/components/graph/forceSettings.ts`:

```ts
export interface ForceSettings {
  center: number;
  repel: number;
  linkForce: number;
  linkDistance: number;
  frozen: boolean;
}

export const DEFAULT_FORCE_SETTINGS: ForceSettings = {
  center: 0.05,
  repel: -150,
  linkForce: 0.7,
  linkDistance: 80,
  frozen: false,
};

export const FORCE_RANGES = {
  center: { min: 0, max: 1, step: 0.01 },
  repel: { min: -800, max: 0, step: 10 },
  linkForce: { min: 0, max: 1, step: 0.05 },
  linkDistance: { min: 10, max: 300, step: 5 },
} as const;

const STORAGE_KEY = "cairn.graph.forces";

const clamp = (v: number, min: number, max: number): number =>
  Math.min(max, Math.max(min, v));

export function clampForceSettings(s: ForceSettings): ForceSettings {
  return {
    center: clamp(s.center, FORCE_RANGES.center.min, FORCE_RANGES.center.max),
    repel: clamp(s.repel, FORCE_RANGES.repel.min, FORCE_RANGES.repel.max),
    linkForce: clamp(
      s.linkForce,
      FORCE_RANGES.linkForce.min,
      FORCE_RANGES.linkForce.max,
    ),
    linkDistance: clamp(
      s.linkDistance,
      FORCE_RANGES.linkDistance.min,
      FORCE_RANGES.linkDistance.max,
    ),
    frozen: !!s.frozen,
  };
}

export function loadForceSettings(): ForceSettings {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return DEFAULT_FORCE_SETTINGS;
    const parsed = JSON.parse(raw) as Partial<ForceSettings>;
    return clampForceSettings({ ...DEFAULT_FORCE_SETTINGS, ...parsed });
  } catch {
    return DEFAULT_FORCE_SETTINGS;
  }
}

export function saveForceSettings(s: ForceSettings): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(s));
  } catch {
    // ignore (private mode / quota)
  }
}
```

- [ ] **Step 4: Run to verify pass**

Run: `pnpm test -- forceSettings`
Expected: PASS.

- [ ] **Step 5: Per-task gate**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check`
Expected: all PASS (131 + new forceSettings tests). Fix format if needed.

- [ ] **Step 6: Commit**

```bash
git add web/src/components/graph/forceSettings.ts web/src/components/graph/forceSettings.test.ts
git commit -m "feat(graph): force-settings model + localStorage persistence"
```

---

## Task 2: GraphForcesPanel (presentational)

**Files:**
- Create: `web/src/components/graph/GraphForcesPanel.tsx`
- Create: `web/src/components/graph/GraphForcesPanel.test.tsx`

- [ ] **Step 1: Write the failing tests**

Create `web/src/components/graph/GraphForcesPanel.test.tsx`:

```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { GraphForcesPanel } from "./GraphForcesPanel";
import { DEFAULT_FORCE_SETTINGS } from "./forceSettings";

describe("GraphForcesPanel", () => {
  it("renders a slider per force with the current value", () => {
    render(
      <GraphForcesPanel
        settings={DEFAULT_FORCE_SETTINGS}
        onChange={vi.fn()}
        onReset={vi.fn()}
      />,
    );
    // Read the DOM string value directly (range inputs + jest-dom's numeric
    // coercion make toHaveValue ambiguous).
    const val = (label: string) =>
      (screen.getByLabelText(label) as HTMLInputElement).value;
    expect(val("Center force")).toBe("0.05");
    expect(val("Repel force")).toBe("-150");
    expect(val("Link force")).toBe("0.7");
    expect(val("Link distance")).toBe("80");
  });
  it("fires onChange with the updated field when a slider moves", () => {
    const onChange = vi.fn();
    render(
      <GraphForcesPanel
        settings={DEFAULT_FORCE_SETTINGS}
        onChange={onChange}
        onReset={vi.fn()}
      />,
    );
    fireEvent.change(screen.getByLabelText("Link distance"), {
      target: { value: "120" },
    });
    expect(onChange).toHaveBeenCalledWith({
      ...DEFAULT_FORCE_SETTINGS,
      linkDistance: 120,
    });
  });
  it("toggles frozen via the freeze control", () => {
    const onChange = vi.fn();
    render(
      <GraphForcesPanel
        settings={DEFAULT_FORCE_SETTINGS}
        onChange={onChange}
        onReset={vi.fn()}
      />,
    );
    fireEvent.click(screen.getByLabelText("Freeze layout"));
    expect(onChange).toHaveBeenCalledWith({
      ...DEFAULT_FORCE_SETTINGS,
      frozen: true,
    });
  });
  it("fires onReset when Reset is clicked", () => {
    const onReset = vi.fn();
    render(
      <GraphForcesPanel
        settings={DEFAULT_FORCE_SETTINGS}
        onChange={vi.fn()}
        onReset={onReset}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /reset/i }));
    expect(onReset).toHaveBeenCalled();
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `pnpm test -- GraphForcesPanel`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement `GraphForcesPanel.tsx`**

Create `web/src/components/graph/GraphForcesPanel.tsx`:

```tsx
import { type ForceSettings, FORCE_RANGES } from "./forceSettings";

const SLIDERS: { key: keyof typeof FORCE_RANGES; label: string }[] = [
  { key: "center", label: "Center force" },
  { key: "repel", label: "Repel force" },
  { key: "linkForce", label: "Link force" },
  { key: "linkDistance", label: "Link distance" },
];

export function GraphForcesPanel(props: {
  settings: ForceSettings;
  onChange: (next: ForceSettings) => void;
  onReset: () => void;
}) {
  const { settings, onChange, onReset } = props;
  return (
    <div className="w-52 rounded-lg border border-border bg-surface p-3 shadow-2xl">
      <div className="mb-2 flex items-center justify-between">
        <span className="text-[10px] uppercase tracking-wide text-faint">
          Forces
        </span>
        <button
          type="button"
          className="text-[11px] text-muted hover:text-text"
          onClick={onReset}
        >
          Reset
        </button>
      </div>
      {SLIDERS.map(({ key, label }) => {
        const range = FORCE_RANGES[key];
        return (
          <div key={key} className="mb-2.5">
            <div className="mb-1 flex justify-between text-[11px] text-text">
              <span>{label}</span>
              <span className="text-faint">{settings[key]}</span>
            </div>
            <input
              type="range"
              aria-label={label}
              className="w-full accent-accent"
              min={range.min}
              max={range.max}
              step={range.step}
              value={settings[key]}
              onChange={(e) =>
                onChange({ ...settings, [key]: Number(e.target.value) })
              }
            />
          </div>
        );
      })}
      <label className="mt-1 flex items-center gap-2 text-[11px] text-text">
        <input
          type="checkbox"
          aria-label="Freeze layout"
          className="accent-accent"
          checked={settings.frozen}
          onChange={(e) => onChange({ ...settings, frozen: e.target.checked })}
        />
        Freeze layout
      </label>
    </div>
  );
}
```

- [ ] **Step 4: Run to verify pass**

Run: `pnpm test -- GraphForcesPanel`
Expected: PASS (4 tests).

- [ ] **Step 5: Per-task gate**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check`
Expected: all PASS. Fix format if needed.

- [ ] **Step 6: Commit**

```bash
git add web/src/components/graph/GraphForcesPanel.tsx web/src/components/graph/GraphForcesPanel.test.tsx
git commit -m "feat(graph): presentational force-settings panel (sliders/freeze/reset)"
```

---

## Task 3: Wire the panel into GraphView + e2e

**Files:**
- Modify: `web/src/components/GraphView.tsx`
- Modify: `web/e2e/skeleton.spec.ts`

- [ ] **Step 1: Wire force settings into `GraphView`**

In `web/src/components/GraphView.tsx`:

First, extend the `RFNode` interface (top of the file) with the d3 pin fields so the freeze code typechecks:

```tsx
interface RFNode {
  id: string;
  label: string;
  degree: number;
  x?: number;
  y?: number;
  fx?: number; // d3 pin (set to freeze, undefined to release)
  fy?: number;
}
```

Add imports:

```tsx
import { IconButton } from "./ui/IconButton";
import { GraphForcesPanel } from "./graph/GraphForcesPanel";
import {
  type ForceSettings,
  DEFAULT_FORCE_SETTINGS,
  loadForceSettings,
  saveForceSettings,
} from "./graph/forceSettings";
```

Add state near the other hooks (after `const [size, setSize] = useState(...)`):

```tsx
  const [forces, setForces] = useState<ForceSettings>(loadForceSettings);
  const [panelOpen, setPanelOpen] = useState(false);

  const changeForces = (next: ForceSettings) => {
    setForces(next);
    saveForceSettings(next);
  };
```

Add an effect that applies the settings to the live simulation (place it after the existing effects). It re-runs whenever `forces`, `data`, or `size` change — so persisted values apply on first mount and survive a data rebuild:

```tsx
  // Apply force settings to the d3 simulation (imperative; forces created by
  // react-force-graph). Re-applies on settings/data/size change.
  useEffect(() => {
    const fg = fgRef.current;
    if (!fg) return;
    fg.d3Force("charge")?.strength(forces.repel);
    const link = fg.d3Force("link") as
      | { strength: (n: number) => unknown; distance: (n: number) => unknown }
      | undefined;
    link?.strength(forces.linkForce);
    link?.distance(forces.linkDistance);
    fg.d3Force("center")?.strength(forces.center);

    // Freeze = pin every node so the layout holds static (hover still repaints);
    // unfreeze clears the pins.
    for (const n of data.nodes as RFNode[]) {
      if (forces.frozen) {
        n.fx = n.x;
        n.fy = n.y;
      } else {
        n.fx = undefined;
        n.fy = undefined;
      }
    }
    if (!forces.frozen) fg.d3ReheatSimulation();
  }, [forces, data, size.width, size.height]);
```

Make the container `relative` and add the gear + panel overlay. Change the container `<div>` and add the overlay as the FIRST children (before the `{size... && <ForceGraph2D/>}`):

```tsx
  return (
    <div ref={containerRef} className="relative h-full w-full">
      <div className="absolute right-2 top-2 z-10 flex flex-col items-end gap-2">
        <IconButton
          label="Graph forces"
          className="border border-border bg-surface"
          onClick={() => setPanelOpen((o) => !o)}
        >
          <svg
            width="16"
            height="16"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <circle cx="12" cy="12" r="3" />
            <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
          </svg>
        </IconButton>
        {panelOpen && (
          <GraphForcesPanel
            settings={forces}
            onChange={changeForces}
            onReset={() => changeForces(DEFAULT_FORCE_SETTINGS)}
          />
        )}
      </div>
      {size.width > 0 && size.height > 0 && (
        <ForceGraph2D
          /* …all existing props unchanged… */
        />
      )}
    </div>
  );
```

(Keep the existing `<ForceGraph2D>` element and its props exactly as they are — only the wrapping `<div>` className gains `relative` and the overlay block is added above it.)

- [ ] **Step 2: Per-task gate (component is canvas — no new unit test here)**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check && pnpm build`
Expected: all PASS. (`pnpm build` confirms the wiring compiles.) The `as RFNode[]` and the `link` cast bridge react-force-graph's loose d3 types; keep them minimal.

- [ ] **Step 3: Add the e2e (gear opens the panel)**

In `web/e2e/skeleton.spec.ts`, extend the graph test (the one asserting the canvas). After the canvas assertion, add:

```ts
  // The forces gear opens the settings panel.
  await page.getByRole("button", { name: "Graph forces" }).click();
  await expect(page.getByLabelText("Center force")).toBeVisible();
```

(Place these lines inside the existing `test("graph view: toggle shows the force-graph canvas", …)` block, after the existing assertions. Keep everything else.)

- [ ] **Step 4: Run e2e**

Run: `pnpm e2e`
Expected: all 8 tests pass (the graph test now also opens the panel). If the gear or a slider isn't found, check the overlay isn't behind the canvas (it has `z-10`) and the labels match. If the panel genuinely doesn't open, STOP and report.

- [ ] **Step 5: Final full gate + build**

Run: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check && pnpm build`
Expected: all PASS.

- [ ] **Step 6: Manual/visual check (agent can't view a browser)**

`lsof -ti :5273 | xargs kill 2>/dev/null`; start `pnpm dev --port 5273 --strictPort` (background); `curl -s -o /dev/null -w "%{http_code}" http://localhost:5273` (expect 200); check the dev log is error-free; stop it. Report the app loads. (The human confirms: dragging each slider re-shapes the graph and it re-settles; Freeze holds it static while hover still highlights; Reset restores; values survive a reload.)

- [ ] **Step 7: Commit**

```bash
git add web/src/components/GraphView.tsx web/e2e/skeleton.spec.ts
git commit -m "feat(graph): force-settings panel wired into the graph (live tuning + freeze + persist)"
```

---

## Notes for the executor

- **Apply forces in the effect, persist in the handler.** `changeForces` only does `setForces` + `saveForceSettings`; the single `useEffect` (keyed on `[forces, data, size…]`) is the one place that touches the simulation — keeps state/persist separate from sim-application and ensures persisted values apply on first mount (after `<ForceGraph2D>` mounts, `fgRef.current` exists) and re-apply after a data rebuild.
- **Guard every `d3Force` call with `?.`** — the forces may not exist on the very first render before the engine initializes; the effect re-runs on `size`/`data` changes once it does.
- **Freeze by pinning (`fx/fy`), not pausing** — pausing the animation loop would also stop the UI-4a hover repaint (`autoPauseRedraw={false}`). Pinning holds the layout while keeping rendering/hover alive. Don't `d3ReheatSimulation()` while frozen (it'd fight the pins; pinned nodes stay put anyway).
- **Don't touch the existing `<ForceGraph2D>` props or the UI-4a paint callbacks** — this task only adds state, an effect, and the overlay; the canvas rendering is unchanged.
- **Canvas force effect is manual-visual** — the pure module + DOM panel are unit-tested; the e2e only asserts the panel opens (not the physics). Consistent with UI-4a.
- **react-force-graph type looseness:** `d3Force` returns a loosely-typed force; the small `link` cast + `as RFNode[]` are expected. Keep them minimal; don't change `forceSettings`/`RFNode` shapes.
