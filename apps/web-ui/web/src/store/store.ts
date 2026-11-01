import { createStore, type StoreApi } from "zustand/vanilla";
import { alwaysOpenHost, type CairnHost } from "../client/host";
import type { CairnClient, Unsubscribe } from "../client/types";
import type {
  TagCount,
  Event,
  GraphNode,
  Revision,
  SuggestedEdge,
  SuggestionScope,
} from "../contract";
import type { PluginSummary } from "../contract";
import type { JsonValue } from "../contract/serde_json/JsonValue";
import {
  groupBySlot,
  type SlotEntry,
  type SanitizeReport,
} from "../client/pluginContributions";
import { debounce, type Debounced } from "../util/timer";
import { errMsg } from "./errMsg";
import type { Overrides } from "../components/shortcuts/commands";
import {
  loadOverrides,
  saveOverrides,
} from "../components/shortcuts/keybindingPersistence";
import {
  loadStyles,
  saveStyles,
  remapStyles,
  remapStylesByPrefix,
  type TreeStyleMap,
  type TreeItemStyle,
} from "../components/tree/treeIcons";
import {
  createPluginGrantsSlice,
  type PluginGrantsState,
} from "./pluginGrantsSlice";
import {
  openOrPreview,
  pinTab as pinTabModel,
  closeTab as closeTabModel,
  cycle as cycleModel,
  jumpTo as jumpToModel,
  type Tab,
  type TabsState,
} from "../components/tabs/tabsModel";
import {
  splitPane as splitPaneModel,
  closePane as closePaneModel,
  type PaneState,
  type PanesState,
} from "../components/tabs/paneModel";
import { loadPanes, savePanes } from "../components/tabs/tabsPersistence";
import type { SearchSnippet } from "../components/searchHighlight";
import type { Rename } from "../components/tree/treeMoves";
import { type RefreshTrace, refreshTrace } from "./trace";
import { createHistorySlice, type HistorySlice } from "./historySlice";
import { createAskSlice, type AskState } from "./askSlice";
import { createCollabSlice, type CollabState } from "./collabSlice";
import { createRecoverySlice, type RecoveryState } from "./recoverySlice";

/** A queued, auto-dismissing error notification. */
export interface Toast {
  id: number;
  message: string;
}

/** How long a queued error toast stays before auto-dismissing (ms). */
export const ERROR_TOAST_MS = 6000;

export interface Settings {
  autosaveMs: number;
  editorMode: "livepreview" | "source";
  /** Auto-load remote (`http(s):`) and `data:` images in notes. Default off:
   *  a remote image is a tracking beacon that would otherwise fire on
   *  note-open. When off, such images render a click-to-load placeholder. */
  loadRemoteImages: boolean;
}

export const DEFAULT_SETTINGS: Settings = {
  autosaveMs: 1000,
  editorMode: "livepreview",
  loadRemoteImages: false,
};

export interface NoteBuffer {
  contents: string;
  dirty: boolean;
  saving: boolean;
}

export type MobileTab = "files" | "editor" | "search" | "graph" | "more";

export interface UiState {
  settingsOpen: boolean;
  newNoteOpen: boolean;
  newNoteInitial: string;
  paletteOpen: boolean;
  /** Blocking confirm before the live-collab Reload discards unsaved edits. */
  collabConflictOpen: boolean;
  /** Commit id currently targeted by the name-version dialog, or null when closed. */
  nameVersionFor: string | null;
  /** Active mobile bottom-nav tab (mobile shell only). */
  mobileTab: MobileTab;
  /** Backlinks drawer/sheet open (tablet + mobile shells). */
  backlinksOpen: boolean;
  /** Bottom plugin dock (panel.main slot): active tab + collapse state. */
  panelDock: { activeId: string | null; collapsed: boolean };
  /** Per-command keybinding overrides (chord, or null = unbound). Persisted. */
  keybindingOverrides: Overrides;
}

export const DEFAULT_UI: UiState = {
  settingsOpen: false,
  newNoteOpen: false,
  newNoteInitial: "",
  paletteOpen: false,
  collabConflictOpen: false,
  nameVersionFor: null,
  mobileTab: "editor",
  backlinksOpen: false,
  panelDock: { activeId: null, collapsed: false },
  keybindingOverrides: {},
};

export type GraphDiff = {
  nodes_added: GraphNode[];
  nodes_removed: GraphNode[];
  nodes_changed: GraphNode[];
  edges_added: { from: string; to: string }[];
  edges_removed: { from: string; to: string }[];
};

const EMPTY_TEMPORAL = {
  timeline: null,
  snapshot: null,
  diff: null,
} as const;

export interface CairnState
  extends
    PluginGrantsState,
    HistorySlice,
    AskState,
    RecoveryState,
    CollabState {
  cairnPath: string | null;
  // False until init()/openCairn() finishes restoring persisted tabs. RouteSync
  // waits for this so its URL<->store reconciliation can't race the restore.
  ready: boolean;
  notePaths: string[];
  openNotes: Record<string, NoteBuffer>;
  panes: PaneState[];
  activePane: number;
  splitRatio: number;
  activePath: string | null;
  activeContents: string;
  dirty: boolean;
  saving: boolean;
  uncommitted: boolean;
  lastCommit: string | null;
  lastVersion: Revision | null;
  committing: boolean;
  query: string;
  searchResults: string[] | null;
  searchSnippets: Record<string, SearchSnippet> | null;
  backlinks: string[];
  graph: { nodes: GraphNode[]; edges: { from: string; to: string }[] } | null;
  suggestions: SuggestedEdge[] | null;
  temporal: {
    timeline: Revision[] | null;
    snapshot: {
      nodes: GraphNode[];
      edges: { from: string; to: string }[];
    } | null;
    diff: GraphDiff | null;
  };
  noteTags: Record<string, string[]>;
  tags: TagCount[];
  activeTag: string | null;
  plugins: PluginSummary[];
  // Sanitized plugin UI contributions, grouped by slot for a future SlotRenderer.
  pluginContributions: Record<string, SlotEntry[]>;
  // Monotonic generation counter, bumped on each loadPlugins; lets renderers
  // key off the freshest contribution set.
  pluginEpoch: number;
  // How many contributions the last sanitize pass dropped (for diagnostics).
  pluginDropped: number;
  notice: string | null;
  settings: Settings;
  ui: UiState;
  treeStyles: TreeStyleMap;
  errors: Toast[];
  // Per-area pending flags so consumers can show a spinner/skeleton distinct
  // from an empty result. Set around each async call; a superseded request never
  // clears a newer one's flag (token-guarded).
  loading: {
    search: boolean;
    graph: boolean;
    backlinks: boolean;
    note: boolean;
    suggestions: boolean;
  };
  // "reconnecting" while the push channel is in silent backoff (calm pill);
  // "down" once it has clearly failed (hard banner). Both mean data may be
  // stale until reconnection or a manual refresh.
  liveUpdates: "ok" | "reconnecting" | "down";

  init(): Promise<void>;
  openCairn(): Promise<void>;
  refreshNotePaths(): Promise<void>;
  openNote(path: string, opts?: { pane?: number }): Promise<void>;
  reloadNoteBuffer(path: string): Promise<void>;
  editBuffer(contents: string): void;
  saveActive(): Promise<void>;
  saveNote(path: string): Promise<void>;
  createNote(path: string): Promise<void>;
  deleteNote(path: string): Promise<void>;
  applyRenames(ops: Rename[]): Promise<void>;
  selectTab(path: string, paneIndex?: number): void;
  closeTab(path: string, paneIndex?: number): void;
  closeActiveTab(): void;
  cycleTab(delta: 1 | -1): void;
  jumpToTab(n: number): void;
  pinTab(path: string, paneIndex?: number): void;
  splitPane(): void;
  openToSide(path: string): Promise<void>;
  closePane(index?: number): void;
  focusPane(index: number): void;
  setSplitRatio(ratio: number): void;
  runSearch(query: string): Promise<void>;
  loadTags(): Promise<void>;
  filterByTag(tag: string): Promise<void>;
  loadPlugins(): Promise<void>;
  invokePlugin(
    plugin: string,
    command: string,
    args?: JsonValue,
  ): Promise<void>;
  dismissNotice(): void;
  setQuery(query: string): void;
  closeSearch(): void;
  refreshBacklinks(): Promise<void>;
  loadGraph(): Promise<void>;
  loadSuggestions(scope: SuggestionScope): Promise<void>;
  loadTimeline(path: string): Promise<void>;
  loadVaultTimeline(structural?: boolean): Promise<void>;
  loadSnapshot(revision: string): Promise<void>;
  loadDiff(from: string, to: string): Promise<void>;
  clearTemporal(): void;
  sealNow(): Promise<void>;
  nameVersion(commit: string, name: string): Promise<void>;
  loadLastVersion(): Promise<void>;
  setSettings(patch: Partial<Settings>): void;
  setUi(patch: Partial<UiState>): void;
  setTreeStyle(path: string, style: TreeItemStyle): void;
  remapFolderStyles(from: string, to: string): void;
  setKeybindingOverrides(overrides: Overrides): void;
  dismissError(id: number): void;
  refreshAll(): Promise<void>;
  assetUrl(relPath: string): string;
}

export function createCairnStore(
  client: CairnClient,
  host: CairnHost = alwaysOpenHost,
  trace: RefreshTrace = refreshTrace,
): StoreApi<CairnState> {
  const autosaves = new Map<string, Debounced>();
  let started = false;
  let eventUnsub: Unsubscribe | null = null;

  // Monotonic request tokens: a slow response only applies if no newer request
  // of the same kind has started since. runSearch + filterByTag share `results`
  // because both write the search overlay, so the newest of either wins.
  // loadSnapshot + loadDiff share `temporalData` because both write
  // temporal.snapshot (and diff): without a shared token, a slow loadSnapshot
  // resolving after a loadDiff would still pass its own guard and clobber the
  // compare view with a stale historical graph + null diff.
  const seq = {
    backlinks: 0,
    results: 0,
    graph: 0,
    timeline: 0,
    temporalData: 0,
    suggestions: 0,
  };

  // Monotonic id for queued error toasts (auto-dismiss keys off it).
  let errorSeq = 0;

  // Paths the client itself just wrote, awaiting their note_changed echo. The
  // echo of our own write must not trigger the refresh cascade (autosave storm);
  // a per-path count tolerates several rapid self-writes to the same note.
  const pendingSelfWrites = new Map<string, number>();

  const store = createStore<CairnState>()((set, get) => {
    const tabsState = (paneIndex = get().activePane): TabsState =>
      get().panes[paneIndex];

    const panesState = (): PanesState => ({
      panes: get().panes,
      activePane: get().activePane,
    });

    const persist = () =>
      savePanes({ ...panesState(), ratio: get().splitRatio });

    // The focused-pane mirror fields, derived from a pane + the buffer pool.
    // Shared by syncMirror and any atomic set() that must recompute the mirror
    // alongside other changes (e.g. applyRenames).
    const mirrorOf = (pane: PaneState, openNotes: CairnState["openNotes"]) => {
      const buf = pane.activePath ? openNotes[pane.activePath] : undefined;
      return {
        activePath: pane.activePath,
        activeContents: buf?.contents ?? "",
        dirty: buf?.dirty ?? false,
        saving: buf?.saving ?? false,
      };
    };

    // Recompute the focused-pane mirror so existing consumers of the top-level
    // activePath/activeContents/dirty/saving stay unchanged.
    const syncMirror = () =>
      set((s) => mirrorOf(s.panes[s.activePane], s.openNotes));

    // Write a TabsState into one pane, then refresh the mirror from the focused pane.
    const applyTabs = (next: TabsState, paneIndex = get().activePane) => {
      set((s) => {
        const panes = s.panes.slice();
        panes[paneIndex] = next;
        return { panes };
      });
      syncMirror();
    };

    const setLoading = (key: keyof CairnState["loading"], value: boolean) =>
      set((s) => ({ loading: { ...s.loading, [key]: value } }));

    // Funnel for every caught command/query error. Logs a structured diagnostic
    // (operation + context + typed ContractError) for devs, surfaces an
    // operation-prefixed toast for users, and auto-dismisses it. Appends via a
    // functional set() so concurrent errors during a refresh storm queue up
    // instead of clobbering one another.
    const pushError = (
      operation: string,
      err: unknown,
      context: Record<string, unknown> = {},
    ) => {
      console.error(`[cairn] ${operation} failed`, {
        operation,
        ...context,
        error: err,
      });
      const id = ++errorSeq;
      const message = `${operation}: ${errMsg(err)}`;
      set((s) => ({ errors: [...s.errors, { id, message }] }));
      // Fire-and-forget: the store is a singleton for the page lifetime, so the
      // pending timer can't outlive a torn-down store. Manual dismiss is the
      // common case; this is just the auto-expiry backstop.
      setTimeout(() => {
        set((s) => ({ errors: s.errors.filter((t) => t.id !== id) }));
      }, ERROR_TOAST_MS);
    };

    // A query/command came back with a variant we don't handle. Narrowing on
    // `res.type` and silently no-opping would leave the view stale with no
    // diagnostic; route the surprise through the same error channel instead.
    const unexpected = (
      operation: string,
      res: { type: string },
      context: Record<string, unknown> = {},
    ) =>
      pushError(operation, new Error(`unexpected response: ${res.type}`), {
        ...context,
        response: res.type,
      });

    // Load a note's buffer if not already open. Returns false on failure.
    const ensureNote = async (path: string): Promise<boolean> => {
      if (get().openNotes[path]) return true;
      setLoading("note", true);
      try {
        const res = await client.runQuery({ type: "get_note", path });
        if (res.type !== "note") {
          unexpected("Open note", res, { path });
          return false;
        }
        set((s) => ({
          openNotes: {
            ...s.openNotes,
            [path]: { contents: res.contents, dirty: false, saving: false },
          },
        }));
        return true;
      } finally {
        setLoading("note", false);
      }
    };

    // Write a note's buffer; if it's the active note, keep the top-level mirror
    // (activeContents/dirty/saving) in sync so existing consumers are unchanged.
    const setBuffer = (path: string, patch: Partial<NoteBuffer>) => {
      set((s) => {
        const cur = s.openNotes[path] ?? {
          contents: "",
          dirty: false,
          saving: false,
        };
        const buf = { ...cur, ...patch };
        const mirror =
          s.activePath === path
            ? {
                activeContents: buf.contents,
                dirty: buf.dirty,
                saving: buf.saving,
              }
            : {};
        return { openNotes: { ...s.openNotes, [path]: buf }, ...mirror };
      });
    };

    const dropNote = (path: string) => {
      autosaves.get(path)?.cancel();
      autosaves.delete(path);
      set((s) => {
        const rest = { ...s.openNotes };
        delete rest[path];
        return { openNotes: rest };
      });
    };

    // The push-event handler. Extracted so `connectEvents` can (re)attach it.
    const onEvent = (e: Event) => {
      if (e.type === "note_changed" || e.type === "note_deleted") {
        // Detect the echo of our own just-written note. A debounced autosave
        // must not fan the full index/graph cascade out on every keystroke
        // (the refresh storm) — but the *targeted* views that reflect the
        // edit just made (backlinks, the active search/filter) should still
        // update. So for a self-write we skip the expensive index-wide work
        // (note list, tags, and the whole-graph rebuild) and refresh only
        // the active note's derived data. External changes refresh fully.
        let selfWrite = false;
        if (e.type === "note_changed") {
          const pending = pendingSelfWrites.get(e.path) ?? 0;
          if (pending > 0) {
            if (pending === 1) pendingSelfWrites.delete(e.path);
            else pendingSelfWrites.set(e.path, pending - 1);
            selfWrite = true;
          }
        }
        // Dispatch + observe the refresh fan-out. `fire` records each action
        // name (for the dev trace's event log) and times the backend call;
        // behavior is identical to the old fire-and-forget `void` dispatch.
        const dispatched: string[] = [];
        const fire = (name: string, run: () => Promise<void>) => {
          dispatched.push(name);
          void trace.time(name, run);
        };
        if (!selfWrite) {
          fire("refreshNotePaths", () => get().refreshNotePaths());
          fire("loadTags", () => get().loadTags());
          if (get().graph !== null) fire("loadGraph", () => get().loadGraph());
        }
        const tag = get().activeTag;
        if (tag) fire("filterByTag", () => get().filterByTag(tag));
        else if (get().searchResults !== null)
          fire("runSearch", () => get().runSearch(get().query));
        if (get().activePath)
          fire("refreshBacklinks", () => get().refreshBacklinks());
        trace.event(e.type, dispatched);
      } else if (e.type === "committed") {
        set({ lastCommit: e.commit, uncommitted: false });
        void get().loadLastVersion();
      }
    };

    // (Re)attach the event channel. A failed attach flips liveUpdates to "down"
    // so the UI can surface the degraded state and offer a manual refresh.
    const connectEvents = () => {
      eventUnsub?.();
      eventUnsub = client.subscribe(
        onEvent,
        () => set({ liveUpdates: "down" }),
        (s) => set({ liveUpdates: s === "live" ? "ok" : "reconnecting" }),
      );
    };

    // Load a cairn's derived state from scratch and arm autosave. Resets every
    // per-cairn view first (so nothing leaks from a previously-open cairn), then
    // reloads the note list, tags, plugins, and the persisted pinned tabs. Shared
    // by init() and openCairn() so the two load paths can't drift out of sync.
    const loadCairn = async () => {
      set({
        openNotes: {},
        panes: [{ tabs: [], activePath: null }],
        activePane: 0,
        splitRatio: 0.5,
        activePath: null,
        activeContents: "",
        dirty: false,
        saving: false,
        backlinks: [],
        graph: null,
        suggestions: null,
        temporal: { ...EMPTY_TEMPORAL },
        noteTags: {},
        searchResults: null,
        searchSnippets: null,
        activeTag: null,
        tags: [],
        plugins: [],
        pluginContributions: {},
        pluginDropped: 0,
        notice: null,
        loading: {
          search: false,
          graph: false,
          backlinks: false,
          note: false,
          suggestions: false,
        },
      });
      await get().refreshNotePaths();
      await get().loadTags();
      await get().loadPlugins();
      // Restore persisted panes; skip any pinned note that no longer loads, and
      // collapse a pane that restores empty.
      const persisted = loadPanes(get().notePaths);
      const restored: PaneState[] = [];
      for (const pp of persisted.panes) {
        const tabs: Tab[] = [];
        for (const p of pp.pinned) {
          if (await ensureNote(p)) tabs.push({ path: p, preview: false });
        }
        const activePath =
          pp.activePath && tabs.some((t) => t.path === pp.activePath)
            ? pp.activePath
            : tabs.length > 0
              ? tabs[tabs.length - 1].path
              : null;
        restored.push({ tabs, activePath });
      }
      const nonEmpty = restored.filter((p) => p.tabs.length > 0);
      const panes =
        nonEmpty.length > 0 ? nonEmpty : [{ tabs: [], activePath: null }];
      const activePane = Math.min(persisted.activePane, panes.length - 1);
      set({ panes, activePane, splitRatio: persisted.ratio });
      syncMirror();
      if (get().activePath) await get().refreshBacklinks();
      void get().loadLastVersion();
    };

    return {
      cairnPath: null,
      ready: false,
      notePaths: [],
      openNotes: {},
      panes: [{ tabs: [], activePath: null }],
      activePane: 0,
      splitRatio: 0.5,
      activePath: null,
      activeContents: "",
      dirty: false,
      saving: false,
      uncommitted: false,
      lastCommit: null,
      lastVersion: null,
      committing: false,
      query: "",
      searchResults: null,
      searchSnippets: null,
      backlinks: [],
      graph: null,
      suggestions: null,
      temporal: { ...EMPTY_TEMPORAL },
      noteTags: {},
      tags: [],
      activeTag: null,
      plugins: [],
      pluginContributions: {},
      pluginEpoch: 0,
      pluginDropped: 0,
      notice: null,
      ...createPluginGrantsSlice(set),
      settings: DEFAULT_SETTINGS,
      ui: DEFAULT_UI,
      treeStyles: loadStyles(),
      errors: [],
      loading: {
        search: false,
        graph: false,
        backlinks: false,
        note: false,
        suggestions: false,
      },
      liveUpdates: "ok",
      ...createAskSlice(set, get, client),
      ...createCollabSlice(set, get, client, setBuffer, pushError),
      ...createRecoverySlice(set, get, client),

      async init() {
        if (started) return;
        started = true;
        set((s) => ({
          ui: { ...s.ui, keybindingOverrides: loadOverrides() },
        }));
        const path = await host.currentCairn();
        set({ cairnPath: path });
        // Attach the push-event channel once, for the store's lifetime — NOT
        // inside the path gate.
        connectEvents();
        if (path !== null) {
          await loadCairn();
        }
        // Restore is complete: RouteSync may now reconcile URL <-> store. Setting
        // this last means a /note/* deep link in the URL is opened by RouteSync
        // *after* (and so wins over) the persisted-tab restore above, without the
        // two racing during startup.
        set({ ready: true });
      },

      async openCairn() {
        try {
          const path = await host.openCairn();
          if (path === null) return; // cancelled
          set({ cairnPath: path });
          await loadCairn();
          // The freshly loaded cairn is restored; RouteSync gates on this — flip
          // it so URL <-> store reconciliation can run.
          set({ ready: true });
        } catch (err) {
          pushError("Open vault", err);
        }
      },

      async refreshNotePaths() {
        try {
          const res = await client.runQuery({ type: "list_notes" });
          if (res.type === "notes")
            set({ notePaths: res.notes.map((n) => n.path) });
          else unexpected("List notes", res);
        } catch (err) {
          pushError("List notes", err);
        }
      },

      async openNote(path, opts) {
        const pane = opts?.pane ?? get().activePane;
        try {
          if (!(await ensureNote(path))) return;
          applyTabs(openOrPreview(tabsState(pane), path), pane);
          persist();
          await get().refreshBacklinks();
        } catch (err) {
          pushError("Open note", err, { path });
        }
      },

      // note_changed refreshes derived views (tree/backlinks/graph) but does
      // NOT reload an open note's buffer, so an external disk write (e.g. a
      // restore) wouldn't otherwise appear. Clean-only: never clobber unsaved
      // edits.
      async reloadNoteBuffer(path) {
        const buf = get().openNotes[path];
        if (!buf || buf.dirty) return;
        const res = await client.runQuery({ type: "get_note", path });
        if (res.type === "note") {
          setBuffer(path, { contents: res.contents, dirty: false });
        }
      },

      editBuffer(contents) {
        const path = get().activePath;
        if (!path) return;
        setBuffer(path, { contents, dirty: true });
        // Editing pins the (possibly preview) active tab.
        applyTabs(pinTabModel(tabsState(), path));
        persist();
        // One persistent debounce per open note, re-triggered on each keystroke
        // rather than reconstructed — the delay thunk re-reads autosaveMs so a
        // settings change still applies on the next edit.
        let d = autosaves.get(path);
        if (!d) {
          d = debounce(
            () => void get().saveNote(path),
            () => get().settings.autosaveMs,
          );
          autosaves.set(path, d);
        }
        d();
      },

      saveActive() {
        return get().saveNote(get().activePath ?? "");
      },

      async saveNote(path) {
        const buf = get().openNotes[path];
        if (!buf || !buf.dirty) return;
        const snapshot = buf.contents;
        setBuffer(path, { saving: true });
        // Mark before sending: the engine echoes note_changed for this write and
        // the subscribe handler must see the pending mark to suppress it.
        pendingSelfWrites.set(path, (pendingSelfWrites.get(path) ?? 0) + 1);
        try {
          await client.sendCommand({
            type: "write_note",
            path,
            contents: snapshot,
          });
          const cur = get().openNotes[path];
          // If the note was closed mid-write its buffer is gone — don't resurrect
          // it; the write still landed, so just mark the repo uncommitted.
          if (cur) {
            setBuffer(path, {
              saving: false,
              // Stay dirty if the note changed during the write (the pending
              // debounce will save it).
              dirty: cur.contents !== snapshot,
            });
          }
          set({ uncommitted: true });
          // Our write just became the on-disk version, so the pending "they
          // changed it" nudge has nothing left to offer: "See their version"
          // would fetch back our own bytes and show an empty diff. Clear the
          // conflict — the peer's edit is recoverable from the version history,
          // not from a chip that dead-ends.
          if (get().collab.note === path && get().collab.pendingCount > 0) {
            get().setUi({ collabConflictOpen: false });
            set((s) => ({
              collab: { ...s.collab, pendingCount: 0, theirs: null },
            }));
          }
        } catch (err) {
          // The write failed, so no echo is coming — release the pending mark
          // (else a later external change to this path would be wrongly skipped).
          pendingSelfWrites.set(
            path,
            Math.max(0, (pendingSelfWrites.get(path) ?? 1) - 1),
          );
          if (get().openNotes[path]) setBuffer(path, { saving: false });
          pushError("Save note", err, { path });
        }
      },

      async createNote(path) {
        try {
          await client.sendCommand({ type: "write_note", path, contents: "" });
          await get().openNote(path);
          get().pinTab(path); // new notes open pinned
        } catch (err) {
          pushError("Create note", err, { path });
        }
      },

      async deleteNote(path) {
        try {
          await client.sendCommand({ type: "delete_note", path });
          // Close the tab in every pane that holds it. Re-find after each close
          // because closeTab can collapse a pane and shift indices.
          let i = get().panes.findIndex((p) =>
            p.tabs.some((t) => t.path === path),
          );
          while (i !== -1) {
            get().closeTab(path, i);
            i = get().panes.findIndex((p) =>
              p.tabs.some((t) => t.path === path),
            );
          }
          // Drop the deleted note's tree icon/style.
          set((s) => {
            if (!(path in s.treeStyles)) return {};
            const next = { ...s.treeStyles };
            delete next[path];
            saveStyles(next);
            return { treeStyles: next };
          });
        } catch (err) {
          pushError("Delete note", err, { path });
        }
      },

      async applyRenames(ops) {
        for (const { from, to } of ops) {
          try {
            await client.sendCommand({ type: "rename_note", from, to });
          } catch (err) {
            pushError("Rename note", err, { from, to });
            break;
          }
          set((s) => {
            const openNotes = { ...s.openNotes };
            if (from in openNotes) {
              openNotes[to] = openNotes[from];
              delete openNotes[from];
            }
            const panes = s.panes.map((p) => ({
              tabs: p.tabs.map((t) =>
                t.path === from ? { ...t, path: to } : t,
              ),
              activePath: p.activePath === from ? to : p.activePath,
            }));
            return {
              openNotes,
              panes,
              ...mirrorOf(panes[s.activePane], openNotes),
            };
          });
        }
        set((s) => {
          const treeStyles = remapStyles(ops, s.treeStyles);
          saveStyles(treeStyles);
          return { treeStyles };
        });
        persist();
        if (get().activePath) void get().refreshBacklinks();
      },

      selectTab(path, paneIndex = get().activePane) {
        if (!get().openNotes[path]) return;
        set({ activePane: paneIndex });
        applyTabs({ ...tabsState(paneIndex), activePath: path }, paneIndex);
        persist();
        void get().refreshBacklinks();
      },

      closeTab(path, paneIndex = get().activePane) {
        void get().saveNote(path);
        applyTabs(closeTabModel(tabsState(paneIndex), path), paneIndex);
        const stillOpen = get().panes.some((p) =>
          p.tabs.some((t) => t.path === path),
        );
        if (!stillOpen) dropNote(path);
        if (
          get().panes.length > 1 &&
          get().panes[paneIndex].tabs.length === 0
        ) {
          const r = closePaneModel(panesState(), paneIndex);
          set({ panes: r.panes, activePane: r.activePane });
          syncMirror();
        }
        persist();
        void get().refreshBacklinks();
      },

      closeActiveTab() {
        const path = get().activePath;
        if (path) get().closeTab(path);
      },

      cycleTab(delta) {
        applyTabs(cycleModel(tabsState(), delta));
        persist();
        void get().refreshBacklinks();
      },

      jumpToTab(n) {
        applyTabs(jumpToModel(tabsState(), n));
        persist();
        void get().refreshBacklinks();
      },

      pinTab(path, paneIndex = get().activePane) {
        if (!get().openNotes[path]) return;
        set({ activePane: paneIndex });
        applyTabs(
          pinTabModel({ ...tabsState(paneIndex), activePath: path }, path),
          paneIndex,
        );
        persist();
        void get().refreshBacklinks();
      },

      splitPane() {
        const seed = get().activePath; // duplicate the focused note
        if (seed === null || get().panes.length >= 2) return;
        const r = splitPaneModel(panesState(), seed);
        set({ panes: r.panes, activePane: r.activePane });
        syncMirror();
        persist();
        void get().refreshBacklinks();
      },

      async openToSide(path) {
        if (!(await ensureNote(path))) return;
        if (get().panes.length < 2) {
          // Create the second pane seeded directly with `path` and focus it.
          const r = splitPaneModel(panesState(), path);
          set({ panes: r.panes, activePane: r.activePane });
        } else {
          const target = get().activePane === 0 ? 1 : 0;
          set({ activePane: target });
          applyTabs(openOrPreview(tabsState(target), path), target);
        }
        syncMirror();
        persist();
        void get().refreshBacklinks();
      },

      closePane(index = get().activePane) {
        if (get().panes.length < 2) return;
        const closing = get().panes[index];
        const survivorPaths = new Set(
          get()
            .panes.filter((_, i) => i !== index)
            .flatMap((p) => p.tabs.map((t) => t.path)),
        );
        for (const t of closing.tabs) {
          if (!survivorPaths.has(t.path)) {
            void get().saveNote(t.path);
            dropNote(t.path);
          }
        }
        const r = closePaneModel(panesState(), index);
        set({ panes: r.panes, activePane: r.activePane });
        syncMirror();
        persist();
        void get().refreshBacklinks();
      },

      focusPane(index) {
        if (
          index < 0 ||
          index >= get().panes.length ||
          index === get().activePane
        )
          return;
        set({ activePane: index });
        syncMirror();
        persist();
        void get().refreshBacklinks();
      },

      setSplitRatio(ratio) {
        set({ splitRatio: Math.min(0.8, Math.max(0.2, ratio)) });
        persist();
      },

      async runSearch(query) {
        const token = ++seq.results;
        setLoading("search", true);
        try {
          const res = await client.runQuery({ type: "search", query });
          if (token !== seq.results) return; // a newer search/filter superseded
          if (res.type === "search_results") {
            set({
              query,
              searchResults: res.results.map((r) => r.path),
              searchSnippets: Object.fromEntries(
                res.results.map((r) => [
                  r.path,
                  { snippet: r.snippet, highlights: r.highlights },
                ]),
              ),
              activeTag: null,
            });
          } else unexpected("Search", res, { query });
        } catch (err) {
          if (token !== seq.results) return;
          pushError("Search", err, { query });
        } finally {
          // Only the current request clears the flag; a superseded one leaves it
          // set because the newer request that bumped the token now owns it.
          if (token === seq.results) setLoading("search", false);
        }
      },

      async loadTags() {
        try {
          const res = await client.runQuery({ type: "list_tags" });
          if (res.type === "tags") set({ tags: res.tags });
          else unexpected("Load tags", res);
        } catch (err) {
          pushError("Load tags", err);
        }
      },

      async filterByTag(tag) {
        const token = ++seq.results;
        setLoading("search", true);
        try {
          const res = await client.runQuery({ type: "notes_by_tag", tag });
          if (token !== seq.results) return; // a newer search/filter superseded
          if (res.type === "paths")
            set({
              searchResults: res.paths,
              searchSnippets: null,
              activeTag: tag,
            });
          else unexpected("Filter notes by tag", res, { tag });
        } catch (err) {
          if (token !== seq.results) return;
          pushError("Filter notes by tag", err, { tag });
        } finally {
          if (token === seq.results) setLoading("search", false);
        }
      },

      async loadPlugins() {
        try {
          const res = await client.runQuery({ type: "list_plugins" });
          if (res.type === "plugins") {
            // Bump the epoch and re-group the (sanitized) contributions. The
            // epoch stays monotonic across reloads so renderers can detect a
            // refreshed contribution set.
            const epoch = get().pluginEpoch + 1;
            const report: SanitizeReport = { kept: 0, dropped: 0, reasons: [] };
            const pluginContributions = groupBySlot(res.plugins, epoch, report);
            if (report.dropped > 0) {
              console.warn(
                `[cairn] dropped ${report.dropped} plugin contribution(s)`,
                report.reasons,
              );
            }
            set({
              plugins: res.plugins,
              pluginContributions,
              pluginEpoch: epoch,
              pluginDropped: report.dropped,
            });
            // Register each plugin's ui/ dir with the native protocol handler so
            // its bundle can be served at plugin-sandbox://<id>/. No-op off Tauri.
            const roots: Record<string, string> = {};
            for (const p of res.plugins) {
              const r = (p as { uiRoot?: string | null }).uiRoot;
              if (r) roots[p.id] = r;
            }
            void host.setPluginUiRoots(roots).catch(() => {});
          } else unexpected("Load plugins", res);
        } catch (err) {
          pushError("Load plugins", err);
        }
      },

      async invokePlugin(plugin, command, args = null) {
        try {
          const res = await client.sendCommand({
            type: "invoke_plugin_command",
            plugin,
            command,
            args,
          });
          if (res.type === "plugin_result") {
            set({
              notice:
                typeof res.result === "string" ? res.result : `Ran ${command}`,
            });
          } else unexpected("Run plugin command", res, { plugin, command });
        } catch (err) {
          pushError("Run plugin command", err, { plugin, command });
        }
      },

      dismissNotice() {
        set({ notice: null });
      },

      setQuery(query) {
        set({ query });
      },

      closeSearch() {
        set({ searchResults: null, searchSnippets: null, activeTag: null });
      },

      async refreshBacklinks() {
        const path = get().activePath;
        if (!path) {
          setLoading("backlinks", false);
          return set({ backlinks: [] });
        }
        const token = ++seq.backlinks;
        setLoading("backlinks", true);
        try {
          const res = await client.runQuery({ type: "get_backlinks", path });
          if (token !== seq.backlinks) return; // superseded by a newer request
          if (res.type === "paths") set({ backlinks: res.paths });
          else unexpected("Load backlinks", res, { path });
        } catch (err) {
          if (token !== seq.backlinks) return;
          pushError("Load backlinks", err, { path });
        } finally {
          if (token === seq.backlinks) setLoading("backlinks", false);
        }
      },

      async loadGraph() {
        const token = ++seq.graph;
        setLoading("graph", true);
        try {
          try {
            // Whole-vault graph; local-neighborhood focusing is done
            // client-side in GraphView, so we always fetch the full scope.
            const res = await client.runQuery({
              type: "get_graph",
              scope: { type: "full" },
            });
            if (token !== seq.graph) return; // superseded by a newer reload
            if (res.type === "graph")
              set({ graph: { nodes: res.nodes, edges: res.edges } });
            else unexpected("Load graph", res);
          } catch (err) {
            if (token !== seq.graph) return;
            pushError("Load graph", err);
          }
          try {
            const tags = await client.noteTags();
            if (token !== seq.graph) return; // superseded by a newer reload
            set({ noteTags: tags });
          } catch {
            // leave the existing noteTags as-is — stale data beats clearing it
          }
        } finally {
          if (token === seq.graph) setLoading("graph", false);
        }
      },

      async loadSuggestions(scope) {
        const token = ++seq.suggestions;
        setLoading("suggestions", true);
        try {
          const res = await client.runQuery({ type: "get_suggestions", scope });
          if (token !== seq.suggestions) return; // superseded by a newer request
          if (res.type === "suggestions") set({ suggestions: res.suggestions });
          else unexpected("Load suggestions", res, { scope: scope.type });
        } catch (err) {
          if (token !== seq.suggestions) return;
          pushError("Load suggestions", err, { scope: scope.type });
        } finally {
          if (token === seq.suggestions) setLoading("suggestions", false);
        }
      },

      // Note-scoped timeline loader. Currently unused in production: the graph
      // scrubber switched to loadVaultTimeline (vault-wide) in the vault-history
      // work. Retained for the deferred note-scoped-filter follow-up (see
      // docs/superpowers/specs/2026-08-09-vault-history-timeline-design.md).
      async loadTimeline(path) {
        const token = ++seq.timeline;
        try {
          const res = await client.runQuery({ type: "note_history", path });
          if (token !== seq.timeline) return;
          if (res.type === "history")
            set((s) => ({
              temporal: { ...s.temporal, timeline: res.revisions },
            }));
          else unexpected("Load timeline", res);
        } catch (err) {
          if (token === seq.timeline) pushError("Load timeline", err, { path });
        }
      },

      async loadVaultTimeline(structural = false) {
        const token = ++seq.timeline;
        try {
          const res = await client.runQuery(
            structural
              ? // A real limit lets the engine's structural walk stop early
                // instead of graph-diffing every commit in history (the lazy
                // `limit` from engine PR #168). 500 graph-changing commits is
                // far more than any realistic vault scrub, so nothing is
                // truncated in practice. `vault_history` stays uncapped: it's a
                // cheap git walk with no per-commit graph diff to bound.
                { type: "structural_revisions", limit: 500 }
              : { type: "vault_history", limit: null },
          );
          if (token !== seq.timeline) return;
          if (res.type === "history")
            set((s) => ({
              temporal: { ...s.temporal, timeline: res.revisions },
            }));
          else unexpected("Load vault timeline", res);
        } catch (err) {
          if (token === seq.timeline) pushError("Load vault timeline", err);
        }
      },

      async loadSnapshot(revision) {
        const token = ++seq.temporalData;
        try {
          const res = await client.runQuery({
            type: "graph_at",
            revision,
            scope: { type: "full" },
          });
          if (token !== seq.temporalData) return; // superseded by a newer snapshot/diff/clear
          if (res.type === "graph")
            set((s) => ({
              temporal: {
                ...s.temporal,
                snapshot: { nodes: res.nodes, edges: res.edges },
                diff: null,
              },
            }));
          else unexpected("Load snapshot", res);
        } catch (err) {
          if (token === seq.temporalData) pushError("Load snapshot", err);
        }
      },

      async loadDiff(from, to) {
        const token = ++seq.temporalData;
        try {
          const [base, delta] = await Promise.all([
            client.runQuery({
              type: "graph_at",
              revision: to,
              scope: { type: "full" },
            }),
            client.runQuery({
              type: "graph_diff",
              from,
              to,
              scope: { type: "full" },
            }),
          ]);
          if (token !== seq.temporalData) return; // superseded by a newer snapshot/diff/clear
          if (base.type === "graph" && delta.type === "graph_diff")
            set((s) => ({
              temporal: {
                ...s.temporal,
                snapshot: { nodes: base.nodes, edges: base.edges },
                diff: {
                  nodes_added: delta.nodes_added,
                  nodes_removed: delta.nodes_removed,
                  nodes_changed: delta.nodes_changed,
                  edges_added: delta.edges_added,
                  edges_removed: delta.edges_removed,
                },
              },
            }));
          else unexpected("Load diff", base.type === "graph" ? delta : base);
        } catch (err) {
          if (token === seq.temporalData) pushError("Load diff", err);
        }
      },

      clearTemporal() {
        // Bump the shared token so any in-flight loadSnapshot/loadDiff is
        // superseded and can't repopulate snapshot/diff after the user left
        // compare/snapshot mode for live.
        ++seq.temporalData;
        set((s) => ({
          temporal: { ...s.temporal, snapshot: null, diff: null },
        }));
      },

      async sealNow() {
        // Claim the guard before the first await: the flush below is async, so
        // checking here and setting after would let a blur landing right on top
        // of a note switch run two overlapping seals.
        if (get().committing) return;
        set({ committing: true });
        try {
          // Flush pending autosaves first. The hints fire on note switch /
          // blur, both of which beat the ~1s debounce, so without this the last
          // keystrokes miss the version we're about to seal and land in
          // whatever the engine's idle timer seals next.
          const dirty = Object.entries(get().openNotes)
            .filter(([, b]) => b.dirty)
            .map(([path]) => path);
          for (const path of dirty) {
            autosaves.get(path)?.cancel();
            await get().saveNote(path);
          }
          if (!get().uncommitted) return;
          const res = await client.sendCommand({
            type: "commit",
            message: null, // engine generates the message and seals the session
          });
          if (res.type === "committed") {
            set({ lastCommit: res.commit, uncommitted: false });
            await get().loadLastVersion();
          } else if (res.type === "nothing_to_commit") {
            // Benign: the engine's idle auto-commit usually beats our hint to
            // the punch. Nothing to seal is success, and seal hints are silent.
            set({ uncommitted: false });
          } else unexpected("Seal version", res);
        } catch (err) {
          // Seal hints are background garnish — a failed hint must never toast.
          console.warn("sealNow failed", err);
        } finally {
          set({ committing: false });
        }
      },

      async nameVersion(commit, name) {
        try {
          const res = await client.sendCommand({
            type: "name_version",
            commit,
            name,
          });
          if (res.type === "done") {
            await get().loadHistory();
            void get().loadLastVersion();
          } else unexpected("Name version", res);
        } catch (err) {
          pushError("Name version", err);
        }
      },

      async loadLastVersion() {
        try {
          const res = await client.runQuery({
            type: "vault_history",
            limit: 1,
          });
          if (res.type === "history")
            set({
              lastVersion: res.revisions[0] ?? null,
            });
        } catch {
          // Status-bar garnish — leave the previous value, never toast.
        }
      },

      setSettings(patch) {
        set({ settings: { ...get().settings, ...patch } });
      },

      setUi(patch) {
        set((s) => ({ ui: { ...s.ui, ...patch } }));
      },

      setTreeStyle(path, style) {
        set((s) => {
          const next = { ...s.treeStyles };
          if (!style.icon && !style.folderColor) delete next[path];
          else next[path] = style;
          saveStyles(next);
          return { treeStyles: next };
        });
      },

      remapFolderStyles(from, to) {
        set((s) => {
          const treeStyles = remapStylesByPrefix(from, to, s.treeStyles);
          saveStyles(treeStyles);
          return { treeStyles };
        });
      },

      setKeybindingOverrides(overrides) {
        saveOverrides(overrides);
        set((s) => ({ ui: { ...s.ui, keybindingOverrides: overrides } }));
      },

      dismissError(id) {
        set((s) => ({ errors: s.errors.filter((t) => t.id !== id) }));
      },

      async refreshAll() {
        // The manual-refresh affordance: re-attach the channel (in case it
        // dropped) and re-pull everything the push events would have. Clearing
        // the flag is optimistic — if re-attach fails again, connectEvents'
        // onError flips it back to "down".
        connectEvents();
        set({ liveUpdates: "ok" });
        await get().refreshNotePaths();
        await get().loadTags();
        if (get().graph !== null) await get().loadGraph();
        // Reconcile the active overlay too, mirroring the push handler — else a
        // refresh while filtered/searching leaves a stale results list.
        const tag = get().activeTag;
        if (tag) await get().filterByTag(tag);
        else if (get().searchResults !== null)
          await get().runSearch(get().query);
        if (get().activePath) await get().refreshBacklinks();
      },

      assetUrl(relPath: string) {
        return host.assetUrl(relPath);
      },

      ...createHistorySlice({ set, get, client, pushError, setBuffer }),
    };
  });

  return store;
}
