import type { StoreApi } from "zustand/vanilla";
import type { Revision } from "../contract";
import type { CairnClient } from "../client/types";
import type { CairnState, NoteBuffer } from "./store";

export type RightTab = "backlinks" | "history";

export interface HistorySlice {
  history: Revision[] | null;
  historyPath: string | null;
  historyLoading: boolean;
  viewingRevision: { path: string; revision: string; contents: string } | null;
  rightTab: RightTab;

  setRightTab(tab: RightTab): void;
  showHistory(): void;
  loadHistory(): Promise<void>;
  viewRevision(revision: string): Promise<void>;
  exitRevisionView(): void;
  restoreRevision(revision: string): Promise<void>;
}

export interface HistorySliceDeps {
  set: StoreApi<CairnState>["setState"];
  get: StoreApi<CairnState>["getState"];
  client: CairnClient;
  pushError: (op: string, err: unknown, ctx?: Record<string, unknown>) => void;
  setBuffer: (path: string, patch: Partial<NoteBuffer>) => void;
}

export function createHistorySlice(deps: HistorySliceDeps): HistorySlice {
  const { set, get, client, pushError, setBuffer } = deps;
  // Monotonic token so a slow note_history can't clobber a newer note's load.
  let historySeq = 0;

  return {
    history: null,
    historyPath: null,
    historyLoading: false,
    viewingRevision: null,
    rightTab: "backlinks",

    setRightTab(tab) {
      set({ rightTab: tab });
    },

    showHistory() {
      set({ rightTab: "history" });
      get().setUi({ backlinksOpen: true }); // opens the drawer on tablet/mobile
      void get().loadHistory();
    },

    async loadHistory() {
      const path = get().activePath;
      if (!path) return;
      const token = ++historySeq;
      set({ historyLoading: true });
      try {
        const res = await client.runQuery({ type: "note_history", path });
        if (token !== historySeq) return; // superseded
        if (res.type !== "history") {
          pushError("Load history", new Error(`unexpected: ${res.type}`), {
            path,
          });
          return;
        }
        set({ history: res.revisions, historyPath: path });
      } catch (err) {
        if (token === historySeq) pushError("Load history", err, { path });
      } finally {
        if (token === historySeq) set({ historyLoading: false });
      }
    },

    async viewRevision(revision) {
      const path = get().activePath;
      if (!path) return;
      try {
        const res = await client.runQuery({ type: "note_at", path, revision });
        if (res.type !== "note") {
          pushError("View revision", new Error(`unexpected: ${res.type}`), {
            path,
            revision,
          });
          return;
        }
        set({ viewingRevision: { path, revision, contents: res.contents } });
      } catch (err) {
        pushError("View revision", err, { path, revision });
      }
    },

    exitRevisionView() {
      set({ viewingRevision: null });
    },

    async restoreRevision(revision) {
      const path = get().activePath;
      if (!path) return;
      try {
        await client.sendCommand({ type: "restore_note", path, revision });
        // restore overwrites the working copy; the store treats the resulting
        // note_changed as external (not a tracked self-write) and won't refresh
        // the active buffer — so reload it explicitly here.
        const res = await client.runQuery({ type: "get_note", path });
        if (res.type === "note") {
          setBuffer(path, { contents: res.contents, dirty: false });
        }
        set({ viewingRevision: null, uncommitted: true });
        await get().loadHistory();
      } catch (err) {
        pushError("Restore note", err, { path, revision });
      }
    },
  };
}
