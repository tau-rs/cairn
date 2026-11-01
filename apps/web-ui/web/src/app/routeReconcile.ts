import {
  type RouteLocation,
  noteUrl,
  notePathFromLocation,
  tagFromLocation,
  isGraph,
} from "./routes";

export interface UrlToStoreInputs {
  location: RouteLocation;
  activePath: string | null;
  activeTag: string | null;
  searchActive: boolean;
}

export type UrlToStoreEffect =
  | { kind: "filterByTag"; tag: string }
  | { kind: "closeSearch" }
  | { kind: "loadGraph" }
  | { kind: "openNote"; path: string };

/** Lane A: what the store should do to match the current URL. */
export function urlToStore(i: UrlToStoreInputs): UrlToStoreEffect[] {
  const tag = tagFromLocation(i.location);
  if (tag !== null) {
    return i.activeTag === tag ? [] : [{ kind: "filterByTag", tag }];
  }

  const effects: UrlToStoreEffect[] = [];
  // Leaving a tag/search overlay: clear it before showing a note or the graph.
  if (i.searchActive || i.activeTag !== null) {
    effects.push({ kind: "closeSearch" });
  }
  if (isGraph(i.location)) {
    effects.push({ kind: "loadGraph" });
    return effects;
  }
  const notePath = notePathFromLocation(i.location);
  if (notePath !== null && notePath !== i.activePath) {
    effects.push({ kind: "openNote", path: notePath });
  }
  return effects;
}

export interface StoreToUrlInputs {
  location: RouteLocation;
  activePath: string | null;
  prevActivePath: string | null;
}

export type StoreToUrlEffect =
  { kind: "navigate"; to: string } | { kind: "none" };

/** Lane B: how the URL should follow a store-origin activePath change. */
export function storeToUrl(i: StoreToUrlInputs): StoreToUrlEffect {
  // Graph and tag routes are explicit, user-chosen foregrounds — never override.
  if (isGraph(i.location) || tagFromLocation(i.location) !== null) {
    return { kind: "none" };
  }

  const urlNote = notePathFromLocation(i.location);
  if (i.activePath === urlNote) return { kind: "none" };

  // A note is active but the URL is at root (or non-note) → reflect it.
  if (urlNote === null) {
    return { kind: "navigate", to: noteUrl(i.activePath as string) };
  }

  // The URL names a note that isn't active. If it matches the PREVIOUS active
  // note, the URL is trailing a store-origin change (closeTab/rename/delete) →
  // reconcile. Otherwise the URL is leading (deep link / user nav) and Lane A
  // will open it — stay out to avoid fighting it.
  if (urlNote === i.prevActivePath) {
    return {
      kind: "navigate",
      to: i.activePath !== null ? noteUrl(i.activePath) : "/",
    };
  }
  return { kind: "none" };
}
