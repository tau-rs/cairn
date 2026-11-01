/** Minimal shape we need from react-router's Location (and easy to test). */
export interface RouteLocation {
  pathname: string;
}

/**
 * Build the URL for a note path. Each path segment is encoded individually and
 * the `/` separators are preserved, so the path hierarchy survives in the URL.
 * Invariant for a clean round-trip: a segment must not itself contain an encoded
 * slash (`%2F`) — `notePathFromLocation` splits on `/` before decoding, so a
 * `%2F` inside a segment would be read back as a separator.
 */
export function noteUrl(path: string): string {
  return "/note/" + path.split("/").map(encodeURIComponent).join("/");
}

/** Extract a note path from a `/note/*` location, or null if not one. */
export function notePathFromLocation(loc: RouteLocation): string | null {
  const m = loc.pathname.match(/^\/note\/(.+)$/);
  if (!m) return null;
  return m[1].split("/").map(decodeURIComponent).join("/");
}

/**
 * Build the URL for a tag filter. A tag is an opaque identifier, not a hierarchy,
 * so the whole string is encoded (slashes included) — unlike `noteUrl`.
 */
export function tagUrl(tag: string): string {
  return "/tags/" + encodeURIComponent(tag);
}

/** Extract a tag from a `/tags/:tag` location, or null if not one. */
export function tagFromLocation(loc: RouteLocation): string | null {
  const m = loc.pathname.match(/^\/tags\/([^/]+)$/);
  return m ? decodeURIComponent(m[1]) : null;
}

/** True only on the graph route. */
export function isGraph(loc: RouteLocation): boolean {
  return loc.pathname === "/graph";
}

/**
 * Target URL for the Graph/Editor toggle: from the graph, back to the active
 * note (root if none); otherwise into the graph.
 */
export function toggleViewTarget(
  loc: RouteLocation,
  activePath: string | null,
): string {
  return isGraph(loc) ? (activePath ? noteUrl(activePath) : "/") : "/graph";
}
