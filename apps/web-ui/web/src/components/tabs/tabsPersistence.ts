import type { TabsState } from "./tabsModel";
import type { PanesState } from "./paneModel";

export interface PersistedTabs {
  pinned: string[];
  activePath: string | null;
}

const STORAGE_KEY = "cairn.tabs";

/** Persist only the pinned tabs (paths) + the active path. */
export function saveTabs(state: TabsState): void {
  try {
    const data: PersistedTabs = {
      pinned: state.tabs.filter((t) => !t.preview).map((t) => t.path),
      activePath: state.activePath,
    };
    localStorage.setItem(STORAGE_KEY, JSON.stringify(data));
  } catch {
    // ignore (private mode / quota)
  }
}

/** Load pinned tabs, dropping any path not in `existingPaths`. The restored
 *  active path is always one of the surviving pinned tabs (else the last, else null). */
export function loadTabs(existingPaths: string[]): PersistedTabs {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return { pinned: [], activePath: null };
    const parsed = JSON.parse(raw) as Partial<PersistedTabs>;
    const exists = new Set(existingPaths);
    const pinned = (parsed.pinned ?? []).filter((p) => exists.has(p));
    const activePath =
      parsed.activePath && pinned.includes(parsed.activePath)
        ? parsed.activePath
        : pinned.length > 0
          ? pinned[pinned.length - 1]
          : null;
    return { pinned, activePath };
  } catch {
    return { pinned: [], activePath: null };
  }
}

export interface PersistedPane {
  pinned: string[];
  activePath: string | null;
}
export interface PersistedPanes {
  panes: PersistedPane[];
  activePane: number;
  ratio: number;
}

const DEFAULT_RATIO = 0.5;

/** Persist all panes (pinned tabs + active), the focused pane, and the ratio. */
export function savePanes(state: PanesState & { ratio: number }): void {
  try {
    const data: PersistedPanes = {
      panes: state.panes.map((p) => ({
        pinned: p.tabs.filter((t) => !t.preview).map((t) => t.path),
        activePath: p.activePath,
      })),
      activePane: state.activePane,
      ratio: state.ratio,
    };
    localStorage.setItem(STORAGE_KEY, JSON.stringify(data));
  } catch {
    // ignore (private mode / quota)
  }
}

function resolvePane(p: PersistedPane, exists: Set<string>): PersistedPane {
  const pinned = (p.pinned ?? []).filter((x) => exists.has(x));
  const activePath =
    p.activePath && pinned.includes(p.activePath)
      ? p.activePath
      : pinned.length > 0
        ? pinned[pinned.length - 1]
        : null;
  return { pinned, activePath };
}

/** Load panes, dropping vanished paths. Tolerates the legacy single-group shape
 *  (`{ pinned, activePath }`) by lifting it into one pane. */
export function loadPanes(existingPaths: string[]): PersistedPanes {
  const fallback: PersistedPanes = {
    panes: [{ pinned: [], activePath: null }],
    activePane: 0,
    ratio: DEFAULT_RATIO,
  };
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return fallback;
    const parsed = JSON.parse(raw) as Partial<PersistedPanes> &
      Partial<PersistedTabs>;
    const exists = new Set(existingPaths);

    // Legacy format: has `pinned`/`activePath` at top level, no `panes`.
    const rawPanes: PersistedPane[] = Array.isArray(parsed.panes)
      ? parsed.panes
      : [
          {
            pinned: parsed.pinned ?? [],
            activePath: parsed.activePath ?? null,
          },
        ];

    const panes = rawPanes.map((p) => resolvePane(p, exists));
    const safePanes = panes.length > 0 ? panes : fallback.panes;
    const activePane = Math.min(
      Math.max(0, parsed.activePane ?? 0),
      safePanes.length - 1,
    );
    const ratio =
      typeof parsed.ratio === "number" ? parsed.ratio : DEFAULT_RATIO;
    return { panes: safePanes, activePane, ratio };
  } catch {
    return fallback;
  }
}
