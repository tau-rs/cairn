/** Daemon transport connection settings, persisted in localStorage.
 *
 *  This is NOT part of the store's `Settings`: the transport is chosen once at
 *  the composition root (`makeBackend`), which runs before the store exists, so
 *  the daemon URL/token must be readable independently of the store. Editing
 *  these takes effect on the next reload. */
export interface DaemonConfig {
  /** Base HTTP URL of the daemon, e.g. `http://localhost:7777`. Empty = unset. */
  url: string;
  /** Bearer token from `<cairn>/.cairn/token`. Empty = send no auth header. */
  token: string;
}

const KEY = "cairn.daemon";
const EMPTY: DaemonConfig = { url: "", token: "" };

/** Read the daemon config, tolerating absent/malformed storage (returns the
 *  empty config so the composition root falls back to the mock). */
export function loadDaemonConfig(): DaemonConfig {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return { ...EMPTY };
    const parsed: unknown = JSON.parse(raw);
    const obj = (parsed ?? {}) as Record<string, unknown>;
    return {
      url: typeof obj.url === "string" ? obj.url : "",
      token: typeof obj.token === "string" ? obj.token : "",
    };
  } catch {
    return { ...EMPTY };
  }
}

export function saveDaemonConfig(cfg: DaemonConfig): void {
  localStorage.setItem(KEY, JSON.stringify(cfg));
}
