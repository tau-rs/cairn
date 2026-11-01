import { stem } from "../../client/wikilink";

/** Resolve a citation target (a stem, possibly path-like) to a real note path
 *  by stem match, or null if none of `notePaths` matches. */
export function resolveStem(
  notePaths: string[],
  target: string,
): string | null {
  const t = stem(target);
  return notePaths.find((p) => stem(p) === t) ?? null;
}
