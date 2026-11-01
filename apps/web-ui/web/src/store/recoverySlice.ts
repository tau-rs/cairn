import type { StoreApi } from "zustand/vanilla";
import type { CairnClient, RecoverySession } from "../client/types";
import type { WireRecoverableBlock, WireBlockId } from "../contract";
import type { CairnState } from "./store";
import { errMsg } from "./errMsg";
import { blockLabel } from "../components/recovery/recoveryModel";

export interface RecoveryState {
  recovery: {
    open: boolean;
    note: string | null;
    status: "idle" | "loading" | "ready" | "error";
    blocks: WireRecoverableBlock[];
    error: string | null;
    restoring: string | null;
  };
  openRecovery(note: string): void;
  restoreVersion(id: WireBlockId, versionIndex: number): void;
  closeRecovery(): void;
}

export const DEFAULT_RECOVERY: RecoveryState["recovery"] = {
  open: false,
  note: null,
  status: "idle",
  blocks: [],
  error: null,
  restoring: null,
};

type Set = StoreApi<CairnState>["setState"];
type Get = StoreApi<CairnState>["getState"];

/** Recovery session slice: owns a live `/collab` recovery session for one
 *  note in its closure (like askSlice's stream subscription), so a
 *  superseded open() can't race a stale session into state. A monotonic
 *  token drops resolutions from a session that's no longer current. */
export function createRecoverySlice(
  set: Set,
  get: Get,
  client: CairnClient,
): RecoveryState {
  let session: RecoverySession | null = null;
  let token = 0;

  const stop = () => {
    session?.close();
    session = null;
  };

  return {
    recovery: DEFAULT_RECOVERY,

    openRecovery(note) {
      stop();
      const t = ++token;
      set(() => ({
        recovery: { ...DEFAULT_RECOVERY, open: true, note, status: "loading" },
      }));
      client
        .openRecovery(note)
        .then((s) => {
          if (t !== token) {
            s.close();
            return;
          }
          session = s;
          set((st) => ({
            recovery: { ...st.recovery, status: "ready", blocks: s.blocks },
          }));
        })
        .catch((err) => {
          if (t !== token) return;
          set((st) => ({
            recovery: { ...st.recovery, status: "error", error: errMsg(err) },
          }));
        });
    },

    restoreVersion(id, versionIndex) {
      const s = session;
      const note = get().recovery.note;
      if (!s || !note) return;
      const t = token;
      set((st) => ({
        recovery: { ...st.recovery, restoring: blockLabel(id) },
      }));
      s.restore(id, versionIndex)
        .then(() => {
          if (t !== token) return;
          return get().reloadNoteBuffer(note);
        })
        .catch((err) => {
          if (t !== token) return;
          set((st) => ({ recovery: { ...st.recovery, error: errMsg(err) } }));
        })
        .finally(() => {
          if (t !== token) return;
          set((st) => ({ recovery: { ...st.recovery, restoring: null } }));
        });
    },

    closeRecovery() {
      stop();
      token++;
      set(() => ({ recovery: DEFAULT_RECOVERY }));
    },
  };
}
