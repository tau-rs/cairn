/** Join `relPath` onto `root`, confining the result to the root directory.
 *  Rejects absolute paths (POSIX, Windows drive, UNC) and any `..` sequence
 *  that would escape `root`. Returns the confined absolute path, or `null`
 *  if the path is absolute or escapes. Backslashes are treated as separators.
 *
 *  This is the guard that must run BEFORE a local image path reaches the
 *  Tauri asset protocol, where an unconfined path is local-file disclosure. */
export function confineToRoot(root: string, relPath: string): string | null {
  // NUL/control characters never belong in a legitimate vault path.
  // eslint-disable-next-line no-control-regex
  if (/[\x00-\x1f]/.test(relPath)) return null;
  // Absolute: leading slash/backslash, `X:\`/`X:/` drive, or `\\` UNC.
  if (/^([/\\]|[a-zA-Z]:[/\\]|\\\\)/.test(relPath)) return null;
  const stack: string[] = [];
  for (const seg of relPath.split(/[/\\]+/)) {
    if (seg === "" || seg === ".") continue;
    if (seg === "..") {
      if (stack.length === 0) return null; // would climb above the root
      stack.pop();
      continue;
    }
    stack.push(seg);
  }
  if (stack.length === 0) return null; // resolves to the root itself
  const base = root.replace(/[/\\]+$/, "");
  return `${base}/${stack.join("/")}`;
}
