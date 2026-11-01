export type AssetUrl = (relPath: string) => string;

/** A resolved image: safe to load now (`ready`), withheld pending opt-in
 *  (`blocked`), or refused outright (`invalid` — e.g. a local path the host
 *  rejected for escaping the vault). `invalid` never loads and offers no
 *  opt-in. */
export type ResolvedImage =
  | { kind: "ready"; url: string }
  | { kind: "blocked"; src: string }
  | { kind: "invalid"; src: string };

export type ImageResolver = (src: string) => ResolvedImage;

/** Map an image markdown `src` to a `ResolvedImage`. Remote (`http(s):`) and
 *  `data:` srcs are `blocked` unless `loadRemote` is set or `isApproved(src)`
 *  says the user clicked Load on that image — they are tracking / exfil
 *  beacons that must not fire on note-open without explicit opt-in. Local
 *  relative paths are resolved through the host's `assetUrl` (itself confined
 *  to the vault root); a host that returns `""` is refusing the path (it
 *  escapes the vault), which surfaces as `invalid` rather than a broken
 *  `<img src="">`. */
export function makeImageResolver(
  assetUrl: AssetUrl,
  opts?: { loadRemote?: boolean; isApproved?: (src: string) => boolean },
): ImageResolver {
  const loadRemote = opts?.loadRemote ?? false;
  const isApproved = opts?.isApproved ?? (() => false);
  return (src: string): ResolvedImage => {
    if (/^(https?:|data:)/i.test(src)) {
      return loadRemote || isApproved(src)
        ? { kind: "ready", url: src }
        : { kind: "blocked", src };
    }
    const url = assetUrl(src);
    if (url === "") return { kind: "invalid", src }; // host refused the path
    return { kind: "ready", url };
  };
}
