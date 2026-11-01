/** App-level cairn lifecycle, separate from the engine contract (CairnClient). */
export interface CairnHost {
  /** The currently-open cairn's path, or null if none is open. */
  currentCairn(): Promise<string | null>;
  /** Pick + open a cairn; resolves the path, or null if cancelled. */
  openCairn(): Promise<string | null>;
  /** Resolve a local relative asset path to a displayable URL (sync). */
  assetUrl(relPath: string): string;
  /** Register the per-plugin ui/ dir allow-list with the native protocol
   *  handler. `{ pluginId: absoluteUiDir }`. No-op off Tauri. */
  setPluginUiRoots(roots: Record<string, string>): Promise<void>;
}

const FIXTURE = "(fixture)";

const BLANK_PNG =
  "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

/** A host where a cairn is always open — used under the mock so the UI never
 *  shows the empty state and existing tests are unaffected. */
export const alwaysOpenHost: CairnHost = {
  currentCairn: () => Promise.resolve(FIXTURE),
  openCairn: () => Promise.resolve(FIXTURE),
  assetUrl: () => BLANK_PNG,
  setPluginUiRoots: () => Promise.resolve(),
};

/** Class form for parity with MockClient construction. */
export class MockHost implements CairnHost {
  currentCairn() {
    return Promise.resolve<string | null>(FIXTURE);
  }
  openCairn() {
    return Promise.resolve<string | null>(FIXTURE);
  }
  assetUrl: (relPath: string) => string = () => BLANK_PNG;
  setPluginUiRoots(_roots: Record<string, string>): Promise<void> {
    void _roots;
    return Promise.resolve();
  }
}
