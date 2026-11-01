import { stem } from "../../client/wikilink";

export type TreeNode =
  | { kind: "folder"; name: string; path: string; children: TreeNode[] }
  | { kind: "note"; name: string; path: string };

interface Acc {
  name: string;
  path: string;
  folders: Map<string, Acc>;
  notes: { name: string; path: string }[];
}

/** Group flat note paths into a tree (split on "/"), each level sorted
 *  folders-first then alphabetical (case-insensitive). Leaf `name` is the stem;
 *  folder `path` is the slash-joined prefix. */
export function buildTree(paths: string[]): TreeNode[] {
  const root: Acc = { name: "", path: "", folders: new Map(), notes: [] };
  for (const p of paths) {
    const segs = p.split("/");
    let cur = root;
    for (let i = 0; i < segs.length - 1; i++) {
      const seg = segs[i];
      const fpath = cur.path ? `${cur.path}/${seg}` : seg;
      let next = cur.folders.get(seg);
      if (!next) {
        next = { name: seg, path: fpath, folders: new Map(), notes: [] };
        cur.folders.set(seg, next);
      }
      cur = next;
    }
    cur.notes.push({ name: stem(segs[segs.length - 1]), path: p });
  }

  const byName = (a: { name: string }, b: { name: string }) =>
    a.name.localeCompare(b.name, undefined, { sensitivity: "base" });

  const toNodes = (acc: Acc): TreeNode[] => {
    const folders: TreeNode[] = [...acc.folders.values()]
      .map((f) => ({
        kind: "folder" as const,
        name: f.name,
        path: f.path,
        children: toNodes(f),
      }))
      .sort(byName);
    const notes: TreeNode[] = acc.notes
      .map((n) => ({ kind: "note" as const, name: n.name, path: n.path }))
      .sort(byName);
    return [...folders, ...notes];
  };
  return toNodes(root);
}

/** Folder paths enclosing a note path, outermost→innermost. "a/b/c.md" → ["a","a/b"]. */
export function ancestorFolders(path: string): string[] {
  const segs = path.split("/");
  segs.pop(); // drop the filename
  const out: string[] = [];
  let acc = "";
  for (const s of segs) {
    acc = acc ? `${acc}/${s}` : s;
    out.push(acc);
  }
  return out;
}
