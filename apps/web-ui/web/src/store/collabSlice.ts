import type { StoreApi } from "zustand/vanilla";
import type { CairnClient, CollabSession } from "../client/types";
import type { CairnState, NoteBuffer } from "./store";

export interface CollabPeer {
  id: string;
  name?: string;
  editing?: boolean;
}

export interface CollabPresence {
  /** The note currently followed, or null when not following. */
  note: string | null;
  /** A foreign op arrived recently; decays to false after LIVE_DECAY_MS quiet. */
  live: boolean;
  /** Foreign changes seen while the buffer was dirty (feeds the reload nudge). */
  pendingCount: number;
  /** Roster from the engine's future awareness channel. Empty today (the wire
   *  is anonymous); the PresenceCluster degrades to "Someone is editing…". */
  peers: CollabPeer[];
  /** Non-destructive view of the incoming remote version (conflict flow). */
  theirs: { note: string; contents: string } | null;
}

export interface CollabState {
  collab: CollabPresence;
  /** Follow `path`'s live session (idempotent if already following it). */
  collabFollow(path: string): void;
  /** Leave the current session and reset presence. */
  collabStop(): void;
  /** User accepted the nudge: reload the buffer now and clear pendingCount. */
  collabReloadNow(): void;
  /** Fetch the remote version for viewing, without touching the buffer. */
  collabViewTheirs(): void;
  /** Close the conflict, keep my buffer, discard the pending nudge. */
  collabKeepMine(): void;
  /** Leave the non-destructive "their version" view, back to my buffer. */
  collabExitTheirs(): void;
}

export const DEFAULT_COLLAB: CollabPresence = {
  note: null,
  live: false,
  pendingCount: 0,
  peers: [],
  theirs: null,
};
export const LIVE_DECAY_MS = 6000;
export const COLLAB_RELOAD_DEBOUNCE_MS = 300;

type Set = StoreApi<CairnState>["setState"];
type Get = StoreApi<CairnState>["getState"];

/** Live-collab presence slice. Closure-owns the CollabSession (like askSlice
 *  owns its stream) plus a monotonic token so a superseded session's late
 *  callbacks can't race stale state in after a note switch. One-way: we receive
 *  peer ops as opaque "changed" signals and reload content from get_note. */
export function createCollabSlice(
  set: Set,
  get: Get,
  client: CairnClient,
  setBuffer: (path: string, patch: Partial<NoteBuffer>) => void,
  pushError: (op: string, err: unknown, ctx?: Record<string, unknown>) => void,
): CollabState {
  let session: CollabSession | null = null;
  let token = 0;
  let decayTimer: ReturnType<typeof setTimeout> | null = null;
  let reloadTimer: ReturnType<typeof setTimeout> | null = null;

  const clearTimers = () => {
    if (decayTimer) clearTimeout(decayTimer);
    if (reloadTimer) clearTimeout(reloadTimer);
    decayTimer = null;
    reloadTimer = null;
  };
  const teardown = () => {
    session?.close();
    session = null;
    clearTimers();
  };

  return {
    collab: DEFAULT_COLLAB,

    collabFollow(path) {
      if (get().collab.note === path && session) return; // already following
      teardown();
      // A pending reload-confirm targeted the note we're leaving; drop it so a
      // late confirm can't clobber the freshly-followed note's edits.
      get().setUi({ collabConflictOpen: false });
      const my = ++token;
      set({
        collab: {
          note: path,
          live: false,
          pendingCount: 0,
          peers: [],
          theirs: null,
        },
      });
      session = client.openCollab(path, {
        onForeignOp: (note) => {
          if (my !== token || get().collab.note !== note) return;
          set((s) => ({ collab: { ...s.collab, live: true } }));
          if (decayTimer) clearTimeout(decayTimer);
          decayTimer = setTimeout(() => {
            if (my !== token) return;
            set((s) => ({ collab: { ...s.collab, live: false } }));
          }, LIVE_DECAY_MS);

          const dirty = get().openNotes[note]?.dirty ?? false;
          if (dirty) {
            set((s) => ({
              collab: { ...s.collab, pendingCount: s.collab.pendingCount + 1 },
            }));
          } else {
            if (reloadTimer) clearTimeout(reloadTimer);
            reloadTimer = setTimeout(() => {
              if (my !== token) return;
              void get().reloadNoteBuffer(note);
            }, COLLAB_RELOAD_DEBOUNCE_MS);
          }
        },
        onError: () => {
          // Presence is non-critical: stay dark, never disrupt editing.
        },
      });
    },

    // Explicit user action (the Reload button): force-replace the buffer from
    // disk even if it's dirty. Unlike the silent auto-reload in onForeignOp
    // (clean-only), a deliberate click is the sanctioned escape hatch that may
    // clobber unsaved edits.
    collabReloadNow() {
      const note = get().collab.note;
      if (!note) return;
      const my = token;
      void (async () => {
        try {
          const res = await client.runQuery({ type: "get_note", path: note });
          if (my !== token) return; // superseded by a note switch / stop
          if (res.type === "note") {
            setBuffer(note, { contents: res.contents, dirty: false });
            set((s) => ({
              collab: { ...s.collab, pendingCount: 0, theirs: null },
            }));
          }
        } catch (err) {
          if (my !== token) return; // superseded: stay silent
          pushError("Reload note", err, { path: note });
        }
      })();
    },

    // Non-destructive: fetches the remote content for display only. The
    // local buffer is never touched, unlike collabReloadNow.
    collabViewTheirs() {
      const note = get().collab.note;
      if (!note) return;
      const my = token;
      void (async () => {
        try {
          const res = await client.runQuery({ type: "get_note", path: note });
          if (my !== token) return; // superseded by a note switch / stop
          if (res.type === "note") {
            set((s) => ({
              collab: {
                ...s.collab,
                theirs: { note, contents: res.contents },
              },
            }));
          }
        } catch (err) {
          if (my !== token) return; // superseded: stay silent
          pushError("View their version", err, { path: note });
        }
      })();
    },

    collabKeepMine() {
      get().setUi({ collabConflictOpen: false });
      set((s) => ({ collab: { ...s.collab, pendingCount: 0, theirs: null } }));
    },

    collabExitTheirs() {
      set((s) => ({ collab: { ...s.collab, theirs: null } }));
    },

    collabStop() {
      token++;
      teardown();
      get().setUi({ collabConflictOpen: false });
      set({ collab: DEFAULT_COLLAB });
    },
  };
}
