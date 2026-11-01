import { invoke, convertFileSrc, Channel } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  Command,
  Query,
  Event,
  CommandResponse,
  QueryResponse,
  AskRequest,
  AnswerEvent,
} from "../contract";
import type {
  CairnClient,
  RecoverySession,
  Unsubscribe,
  CollabHandlers,
  CollabSession,
} from "./types";
import type { CairnHost } from "./host";
import { confineToRoot } from "./vaultPath";
import {
  assertEvent,
  assertCommandResponse,
  assertQueryResponse,
  assertAnswerEvent,
} from "./contractGuards";

/** Talks to the Rust backend over Tauri IPC. Rejections are ContractError
 *  (the Err payload of the Rust command), matching MockClient. */
export class TauriClient implements CairnClient {
  async sendCommand(command: Command): Promise<CommandResponse> {
    return assertCommandResponse(
      await invoke<unknown>("send_command", { command }),
    );
  }
  async runQuery(query: Query): Promise<QueryResponse> {
    return assertQueryResponse(await invoke<unknown>("run_query", { query }));
  }
  subscribe(
    cb: (e: Event) => void,
    onError?: (err: unknown) => void,
  ): Unsubscribe {
    // Validate the payload's discriminant before dispatch (S5): a drifted /
    // malformed event routes to onError (the same degraded-state seam as a
    // failed attach) rather than silently mis-dispatching on a bad `type`.
    const pending = listen<unknown>("cairn://event", (e) => {
      try {
        cb(assertEvent(e.payload));
      } catch (err) {
        onError?.(err);
      }
    });
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    pending.then(
      (fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      },
      (err) => {
        // The channel never attached: the whole reactive-refresh model depends
        // on these push events, so report it rather than leave an unhandled
        // rejection and a silently-stale UI.
        if (!cancelled) onError?.(err);
      },
    );
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }
  async noteTags(): Promise<Record<string, string[]>> {
    const res = await this.runQuery({ type: "list_notes" });
    if (res.type !== "notes") return {};
    return Object.fromEntries(res.notes.map((n) => [n.path, n.tags]));
  }
  /** Stream a note-grounded answer in-process via the `ask` Tauri command. The
   *  Rust side gathers context, emits a leading `sources` frame, then forwards
   *  each agent increment as an `AnswerEvent` over the IPC `Channel`; the
   *  terminal frame is `completed`/`failed`. A pre-stream failure (no cairn
   *  open, internal error) rejects the invoke -> `onError` (the typed
   *  `ContractError`); an in-run failure arrives as a `failed` event. A
   *  malformed frame routes to `onError` rather than mis-dispatching (S5).
   *  Unsubscribe drops further events; the Rust run finishes harmlessly (no v1
   *  cancellation). `onmessage`/the returned `unsub` are assigned synchronously,
   *  before the invoke round-trips, so no frame can arrive un-guarded. */
  ask(
    req: AskRequest,
    onEvent: (e: AnswerEvent) => void,
    onError?: (err: unknown) => void,
  ): Unsubscribe {
    let cancelled = false;
    const channel = new Channel<AnswerEvent>();
    channel.onmessage = (raw) => {
      if (cancelled) return;
      let e: AnswerEvent;
      try {
        e = assertAnswerEvent(raw);
      } catch (err) {
        // A malformed frame is a fatal backend contract violation: stop the
        // stream (drop any later frames) so desktop and daemon transports
        // behave identically — DaemonClient.ask ends its read loop on the same
        // assertion throw.
        cancelled = true;
        onError?.(err);
        return;
      }
      onEvent(e);
    };
    void invoke("ask", { request: req, channel }).catch((err) => {
      if (!cancelled) onError?.(err);
    });
    return () => {
      cancelled = true;
    };
  }

  /** No in-process Tauri recovery transport exists: `/collab` recovery is
   *  daemon-only, so this always rejects. */
  openRecovery(note: string): Promise<RecoverySession> {
    void note;
    return Promise.reject(
      new Error("recovery is only available over a live collab daemon"),
    );
  }

  /** Desktop presence without a `/collab` transport: there is no in-process
   *  collab socket, but the presence layer treats a foreign op as an opaque
   *  "this note changed" signal, and on a single-user desktop an external disk
   *  edit *is* that signal. So bridge the engine watcher's `note_changed`
   *  events (the same `cairn://event` channel `subscribe` reads) into
   *  `onForeignOp`. There is no join frame, so `onSnapshot` fires synchronously:
   *  the session is live the moment the listener is wired. Mirrors `subscribe`'s
   *  cancel-safe unlisten lifecycle; does no store refresh of its own —
   *  `collabSlice.onForeignOp` already owns the targeted reload. */
  openCollab(note: string, handlers: CollabHandlers): CollabSession {
    handlers.onSnapshot?.(note);
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    const pending = listen<unknown>("cairn://event", (e) => {
      if (cancelled) return;
      let ev: Event;
      try {
        ev = assertEvent(e.payload);
      } catch (err) {
        // A drifted / malformed event routes to onError (non-fatal, presence
        // stays dark) — the same degraded seam as the daemon's error frame.
        handlers.onError?.(note, String(err));
        return;
      }
      if (ev.type === "note_changed" && ev.path === note) {
        handlers.onForeignOp?.(note);
      }
    });
    pending.then(
      (fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      },
      (err) => {
        if (!cancelled) handlers.onError?.(note, String(err));
      },
    );
    return {
      close: () => {
        cancelled = true;
        unlisten?.();
      },
    };
  }
}

/** App-level cairn lifecycle over Tauri commands. */
export class TauriHost implements CairnHost {
  private root: string | null = null;
  async currentCairn(): Promise<string | null> {
    this.root = await invoke<string | null>("current_cairn");
    return this.root;
  }
  async openCairn(): Promise<string | null> {
    this.root = await invoke<string | null>("pick_and_open_cairn");
    return this.root;
  }
  assetUrl(relPath: string): string {
    if (/^(https?:|data:)/i.test(relPath)) return relPath;
    if (!this.root) return relPath;
    const full = confineToRoot(this.root, relPath);
    if (full === null) return ""; // path escapes the vault — refuse to resolve
    return convertFileSrc(full);
  }
  async setPluginUiRoots(roots: Record<string, string>): Promise<void> {
    await invoke("set_plugin_ui_roots", { roots });
  }
}
