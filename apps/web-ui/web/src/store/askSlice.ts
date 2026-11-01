import type { StoreApi } from "zustand/vanilla";
import type { CairnClient, Unsubscribe } from "../client/types";
import type { AskRequest, AnswerEvent } from "../contract";
import type { CairnState } from "./store";
import {
  applyAnswerEvent,
  emptyAssistantTurn,
  type AskTurn,
} from "./askReducer";
import { errMsg } from "./errMsg";

export type AskMode = "closed" | "bar" | "panel";

export interface AskConversation {
  mode: AskMode;
  turns: AskTurn[];
  streaming: boolean;
  error: string | null;
}

export interface AskState {
  ask: AskConversation;
  /** Open the prompt bar (no-op if already open in some mode). */
  askOpen(): void;
  /** Submit a question: append turns + start the stream. Ignored while streaming. */
  askSubmit(question: string): void;
  /** Promote the bar into the docked panel (pure mode flip). */
  askPromote(): void;
  /** Close the surface and cancel any in-flight run. */
  askClose(): void;
}

export const DEFAULT_ASK: AskConversation = {
  mode: "closed",
  turns: [],
  streaming: false,
  error: null,
};

type Set = StoreApi<CairnState>["setState"];
type Get = StoreApi<CairnState>["getState"];

/** Ask conversation slice. Owns the agent-stream subscription in its closure
 *  (like store.ts's eventUnsub), so unmounting the bar on promote can't tear
 *  the stream down. A monotonic run token drops events from a superseded run. */
export function createAskSlice(
  set: Set,
  get: Get,
  client: CairnClient,
): AskState {
  let unsub: Unsubscribe | null = null;
  let runToken = 0;

  const stop = () => {
    unsub?.();
    unsub = null;
  };

  return {
    ask: DEFAULT_ASK,

    askOpen() {
      set((s) => ({
        ask: { ...s.ask, mode: s.ask.mode === "closed" ? "bar" : s.ask.mode },
      }));
    },

    askSubmit(question) {
      const q = question.trim();
      if (!q || get().ask.streaming) return;
      stop();
      const token = ++runToken;
      set((s) => ({
        ask: {
          ...s.ask,
          mode: s.ask.mode === "closed" ? "bar" : s.ask.mode,
          streaming: true,
          error: null,
          turns: [
            ...s.ask.turns,
            { role: "user", text: q, citations: [], tools: [] },
            emptyAssistantTurn(),
          ],
        },
      }));

      const onEvent = (e: AnswerEvent) => {
        if (token !== runToken) return;
        if (e.type === "failed") {
          stop();
          set((s) => ({
            ask: { ...s.ask, streaming: false, error: e.message },
          }));
          return;
        }
        if (e.type === "completed") {
          stop();
          set((s) => ({ ask: { ...s.ask, streaming: false } }));
          return;
        }
        if (e.type === "turn_completed") return;
        set((s) => {
          const turns = s.ask.turns.slice();
          const i = turns.length - 1;
          turns[i] = applyAnswerEvent(turns[i], e);
          return { ask: { ...s.ask, turns } };
        });
      };

      // Assumes client.ask delivers its first event asynchronously (the daemon's
      // fetch, the mock's microtask, and the Tauri stub all do), so unsub is
      // assigned before any onEvent/onError runs.
      const req: AskRequest = { query: q, top_k: null };
      unsub = client.ask(req, onEvent, (err) => {
        if (token !== runToken) return;
        stop();
        set((s) => ({
          ask: { ...s.ask, streaming: false, error: errMsg(err) },
        }));
      });
    },

    askPromote() {
      set((s) => ({
        ask: { ...s.ask, mode: s.ask.mode === "bar" ? "panel" : s.ask.mode },
      }));
    },

    askClose() {
      stop();
      runToken++;
      set((s) => ({
        ask: { ...s.ask, mode: "closed", streaming: false, error: null },
      }));
    },
  };
}
