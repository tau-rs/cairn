export interface RecencySettings {
  enabled: boolean;
  windowDays: number;
}
export const DEFAULT_RECENCY: RecencySettings = {
  enabled: false,
  windowDays: 30,
};
export const RECENCY_WINDOW_RANGE = { min: 1, max: 365, step: 1 } as const;

const RING_MAX_WIDTH = 4;
const RING_MIN_WIDTH = 1.5;
const STORAGE_KEY = "cairn.graph.recency";

/** Ring alpha+width from a note's mtime. `t` = fraction of newness in [0,1]
 *  (1 = edited now, 0 = at the window edge). null when older than the window. */
export function recencyRing(
  mtimeSecs: number,
  nowSecs: number,
  windowDays: number,
): { alpha: number; width: number } | null {
  const ageDays = (nowSecs - mtimeSecs) / 86_400;
  if (ageDays > windowDays) return null;
  const t = Math.min(1, Math.max(0, 1 - ageDays / windowDays));
  return {
    alpha: t,
    width: RING_MIN_WIDTH + (RING_MAX_WIDTH - RING_MIN_WIDTH) * t,
  };
}

export function loadRecency(): RecencySettings {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return DEFAULT_RECENCY;
    const p = JSON.parse(raw) as Partial<RecencySettings>;
    return {
      enabled: !!p.enabled,
      windowDays:
        typeof p.windowDays === "number"
          ? Math.min(
              RECENCY_WINDOW_RANGE.max,
              Math.max(RECENCY_WINDOW_RANGE.min, Math.round(p.windowDays)),
            )
          : DEFAULT_RECENCY.windowDays,
    };
  } catch {
    return DEFAULT_RECENCY;
  }
}

export function saveRecency(s: RecencySettings): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(s));
  } catch {
    // ignore (private mode / quota)
  }
}
