// The broker's port to the host. Decoupling the broker from store internals
// keeps it unit-testable against a fake host (hexagonal boundary).
import type { StoreApi } from "zustand";
import type { CairnState } from "../store/store";
import type { JsonValue } from "../contract/serde_json/JsonValue";
import type { CairnClient } from "./types";

export interface BrokerHost {
  // NOTE: `info` is a SILENT method (no capability required), so it must not
  // expose anything sensitive. The active note's path is deliberately NOT here —
  // it would let a zero-grant plugin track which notes the user opens. A plugin
  // that needs the path requests the gated `activeNote.read` capability instead.
  info(): { appVersion: string; theme: string };
  notice(text: string): void;
  activeNote(): { path: string; title: string; text: string } | null;
  writeActiveNote(text: string): void;
  readNote(path: string): Promise<{ path: string; text: string } | null>;
  search(query: string): Promise<Array<{ path: string }>>;
  invokeOwnCommand(
    plugin: string,
    command: string,
    args: JsonValue | null,
  ): Promise<void>;
  /** Fire `cb` whenever the active note path or contents change. Returns unsub. */
  subscribeActiveNote(cb: () => void): () => void;
}

const APP_VERSION =
  (import.meta as { env?: Record<string, string> }).env?.VITE_APP_VERSION ??
  "0.0.0";

function stem(path: string): string {
  const base = path.split("/").pop() ?? path;
  return base.replace(/\.md$/i, "");
}

/** Current theme, read defensively from the DOM (Settings has no theme field on
 *  this branch); defaults to "dark". */
function currentTheme(): string {
  if (typeof document === "undefined") return "dark";
  return document.documentElement.dataset.theme ?? "dark";
}

export function createStoreBrokerHost(
  store: StoreApi<CairnState>,
  client: CairnClient,
): BrokerHost {
  return {
    info() {
      return {
        appVersion: APP_VERSION,
        theme: currentTheme(),
      };
    },
    notice(text) {
      // Set the host notice field directly (cleared later by `dismissNotice`);
      // there is no dedicated "push notice" action to route through.
      store.setState({ notice: text });
    },
    activeNote() {
      const s = store.getState();
      const path = s.activePath;
      if (!path) return null;
      const buf = s.openNotes[path];
      if (!buf) return null;
      return { path, title: stem(path), text: buf.contents };
    },
    writeActiveNote(text) {
      store.getState().editBuffer(text);
    },
    async readNote(path) {
      const res = await client.runQuery({ type: "get_note", path });
      if (res.type !== "note") return null;
      return { path, text: res.contents };
    },
    async search(query) {
      const res = await client.runQuery({ type: "search", query });
      if (res.type !== "search_results") return [];
      // Port returns paths only; score/snippet are intentionally dropped.
      return res.results.map((r) => ({ path: r.path }));
    },
    async invokeOwnCommand(plugin, command, args) {
      await store.getState().invokePlugin(plugin, command, args ?? undefined);
    },
    subscribeActiveNote(cb) {
      let prevPath = store.getState().activePath;
      let prevText = prevPath
        ? store.getState().openNotes[prevPath]?.contents
        : undefined;
      return store.subscribe((s) => {
        const text = s.activePath
          ? s.openNotes[s.activePath]?.contents
          : undefined;
        if (s.activePath !== prevPath || text !== prevText) {
          prevPath = s.activePath;
          prevText = text;
          cb();
        }
      });
    },
  };
}
