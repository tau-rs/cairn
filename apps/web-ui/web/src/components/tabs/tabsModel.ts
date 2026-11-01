export interface Tab {
  path: string;
  preview: boolean;
}

export interface TabsState {
  tabs: Tab[];
  activePath: string | null;
}

/** Open `path`: focus it if already open; else replace the single preview tab
 *  in place; else append a new preview tab. */
export function openOrPreview(state: TabsState, path: string): TabsState {
  if (state.tabs.some((t) => t.path === path)) {
    return { tabs: state.tabs, activePath: path };
  }
  const previewIdx = state.tabs.findIndex((t) => t.preview);
  if (previewIdx !== -1) {
    const tabs = state.tabs.slice();
    tabs[previewIdx] = { path, preview: true };
    return { tabs, activePath: path };
  }
  return { tabs: [...state.tabs, { path, preview: true }], activePath: path };
}

/** Pin the tab for `path` (preview → false). No-op if absent. */
export function pinTab(state: TabsState, path: string): TabsState {
  return {
    tabs: state.tabs.map((t) =>
      t.path === path ? { ...t, preview: false } : t,
    ),
    activePath: state.activePath,
  };
}

/** Remove `path`. If it was active, focus the right neighbour, else the left,
 *  else null. */
export function closeTab(state: TabsState, path: string): TabsState {
  const idx = state.tabs.findIndex((t) => t.path === path);
  if (idx === -1) return state;
  const tabs = state.tabs.filter((t) => t.path !== path);
  let activePath = state.activePath;
  if (state.activePath === path) {
    if (tabs.length === 0) activePath = null;
    else activePath = (tabs[idx] ?? tabs[tabs.length - 1]).path;
  }
  return { tabs, activePath };
}

/** Focus the tab `delta` steps from the active one (wraps). */
export function cycle(state: TabsState, delta: 1 | -1): TabsState {
  if (state.tabs.length === 0) return state;
  const idx = state.tabs.findIndex((t) => t.path === state.activePath);
  const base = idx === -1 ? 0 : idx;
  const n = state.tabs.length;
  return {
    tabs: state.tabs,
    activePath: state.tabs[(base + delta + n) % n].path,
  };
}

/** Focus the Nth tab (1-based); no-op if out of range. */
export function jumpTo(state: TabsState, n: number): TabsState {
  const tab = state.tabs[n - 1];
  return tab ? { tabs: state.tabs, activePath: tab.path } : state;
}
