import type { Rename } from "./treeMoves";

export type IconRef =
  | { kind: "emoji"; value: string }
  | { kind: "lucide"; name: string; color: string };

export interface TreeItemStyle {
  icon?: IconRef;
  folderColor?: string; // folders only; the left-bar accent
}

export type TreeStyleMap = Record<string, TreeItemStyle>;

const STORAGE_KEY = "cairn.treeIcons";

export function loadStyles(): TreeStyleMap {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return {};
    const val = JSON.parse(raw) as unknown;
    if (!val || typeof val !== "object" || Array.isArray(val)) return {};
    return val as TreeStyleMap;
  } catch {
    return {};
  }
}

export function saveStyles(map: TreeStyleMap): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(map));
  } catch {
    // ignore (private mode / quota)
  }
}

/** Remap style-map keys so icons follow notes/folders across rename & move.
 *  Note keys remap directly from each op; folder keys remap by the parent-dir
 *  change derived from the ops (a folder rename/move emits descendant-note ops). */
export function remapStyles(ops: Rename[], map: TreeStyleMap): TreeStyleMap {
  if (ops.length === 0) return map;

  const noteMap = new Map(ops.map((o) => [o.from, o.to] as const));

  // Distinct (oldDir -> newDir) prefix changes from each op's parent segments.
  const prefixPairs: Array<[string, string]> = [];
  const seen = new Set<string>();
  for (const { from, to } of ops) {
    const f = from.split("/");
    const t = to.split("/");
    for (let i = 1; i < f.length; i++) {
      const fp = f.slice(0, i).join("/");
      const tp = t.slice(0, i).join("/");
      if (fp !== tp && !seen.has(fp)) {
        seen.add(fp);
        prefixPairs.push([fp, tp]);
      }
    }
  }

  const remapKey = (key: string): string => {
    const direct = noteMap.get(key);
    if (direct) return direct;
    for (const [fp, tp] of prefixPairs) {
      if (key === fp) return tp;
      if (key.startsWith(fp + "/")) return tp + key.slice(fp.length);
    }
    return key;
  };

  const out: TreeStyleMap = {};
  for (const [key, style] of Object.entries(map)) out[remapKey(key)] = style;
  return out;
}

/** Remap a folder's own style key plus every descendant key (notes & subfolders)
 *  when the folder is renamed/moved. Driven by the explicit folder path change,
 *  so it works even for folders with no descendant notes (which emit no rename
 *  ops) — the gap `remapStyles` can't cover. Idempotent: a no-op when nothing
 *  sits under `from`. */
export function remapStylesByPrefix(
  from: string,
  to: string,
  map: TreeStyleMap,
): TreeStyleMap {
  if (from === to) return map;
  const out: TreeStyleMap = {};
  for (const [key, style] of Object.entries(map)) {
    if (key === from) out[to] = style;
    else if (key.startsWith(from + "/"))
      out[to + key.slice(from.length)] = style;
    else out[key] = style;
  }
  return out;
}
