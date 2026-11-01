import type { TabsState } from "./tabsModel";

/** One pane is exactly a tab group — reuse TabsState so tabsModel applies per pane. */
export type PaneState = TabsState;

export interface PanesState {
  /** 1 (single) or 2 (split). Modelled as an array so N-way is a later UI-only change. */
  panes: PaneState[];
  /** Index of the focused pane. */
  activePane: number;
}

/** Maximum panes the layout allows (v1: side-by-side only). */
const MAX_PANES = 2;

/** Append a second pane seeded with `seedPath` (pinned) and focus it. No-op if
 *  already at MAX_PANES or seedPath is null. */
export function splitPane(s: PanesState, seedPath: string | null): PanesState {
  if (s.panes.length >= MAX_PANES || seedPath === null) return s;
  const seeded: PaneState = {
    tabs: [{ path: seedPath, preview: false }],
    activePath: seedPath,
  };
  return { panes: [...s.panes, seeded], activePane: s.panes.length };
}

/** Remove pane `index`, clamping activePane to a surviving pane. Never drops the
 *  last pane. */
export function closePane(s: PanesState, index: number): PanesState {
  if (s.panes.length <= 1 || index < 0 || index >= s.panes.length) return s;
  const panes = s.panes.filter((_, i) => i !== index);
  const activePane = Math.min(
    s.activePane > index ? s.activePane - 1 : s.activePane,
    panes.length - 1,
  );
  return { panes, activePane };
}

/** Focus pane `index` (guarded). */
export function focusPane(s: PanesState, index: number): PanesState {
  if (index < 0 || index >= s.panes.length || index === s.activePane) return s;
  return { panes: s.panes, activePane: index };
}
