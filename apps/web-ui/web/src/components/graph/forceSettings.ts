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
