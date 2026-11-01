import { isCapability, type PluginCapability } from "../client/pluginTier3";

export const GRANTS_KEY = "cairn.pluginGrants";

export type PluginGrant = {
  version: string;
  granted: readonly PluginCapability[];
};
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

/** Pure: return a new grants map with `pluginId` granted the requested set.
 *  Persistence is a separate step (`persistGrants`), matching the repo's
 *  pure-transform + explicit-write convention (cf. `saveStyles`). */
export function saveGrant(
  grants: PluginGrants,
  pluginId: string,
  version: string,
  granted: readonly PluginCapability[],
): PluginGrants {
  return {
    ...grants,
    [pluginId]: { version, granted: [...granted] },
  };
}

/** Pure: return a new grants map without `pluginId`. */
export function removeGrant(
  grants: PluginGrants,
  pluginId: string,
): PluginGrants {
  const next = { ...grants };
  delete next[pluginId];
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
      if (typeof rec.version !== "string" || !Array.isArray(rec.granted))
        continue;
      out[id] = {
        version: rec.version,
        granted: rec.granted.filter(isCapability),
      };
    }
    return out;
  } catch {
    return {};
  }
}

/** Write-only persistence helper (cf. `saveStyles`/`saveOverrides`). Swallows
 *  storage errors — grants then degrade to session-only. */
export function persistGrants(grants: PluginGrants): void {
  try {
    localStorage.setItem(GRANTS_KEY, JSON.stringify(grants));
  } catch {
    /* storage full / unavailable — grants degrade to session-only */
  }
}

// ── Store slice ────────────────────────────────────────────────────────────
export interface PluginGrantsState {
  pluginGrants: PluginGrants;
  grantPlugin(
    pluginId: string,
    version: string,
    granted: PluginCapability[],
  ): void;
  revokePlugin(pluginId: string): void;
}

/** Slice factory wired into the root store with one import + one spread. */
export function createPluginGrantsSlice(
  set: (
    fn: (s: {
      pluginGrants: PluginGrants;
    }) => Partial<{ pluginGrants: PluginGrants }>,
  ) => void,
): PluginGrantsState {
  return {
    pluginGrants: loadGrants(),
    grantPlugin(pluginId, version, granted) {
      set((s) => {
        const next = saveGrant(s.pluginGrants, pluginId, version, granted);
        persistGrants(next);
        return { pluginGrants: next };
      });
    },
    revokePlugin(pluginId) {
      set((s) => {
        const next = removeGrant(s.pluginGrants, pluginId);
        persistGrants(next);
        return { pluginGrants: next };
      });
    },
  };
}
