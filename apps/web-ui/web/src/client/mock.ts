import type {
  Command,
  Query,
  Event,
  CommandResponse,
  QueryResponse,
  ContractError,
  NoteSummary,
  GraphEdge,
  GraphNode,
  GraphScope,
  SuggestedEdge,
  SearchResult,
  PluginSummary,
  Revision,
  AskRequest,
  AnswerEvent,
  WireRecoverableBlock,
  WireBlockId,
} from "../contract";
import type {
  CairnClient,
  RecoverySession,
  Unsubscribe,
  CollabHandlers,
  CollabSession,
} from "./types";
import { extractLinks, stem } from "./wikilink";
import { extractTags } from "../components/graph/tags";

/** Split a leading `---\n...\n---\n` frontmatter block. Mirrors cairn-domain
 *  Note::parse (frontmatter is the YAML between fences; body is the rest). */
function splitFrontmatter(raw: string): {
  frontmatter: string | null;
  body: string;
} {
  if (!raw.startsWith("---\n")) return { frontmatter: null, body: raw };
  const rest = raw.slice(4);
  if (rest.startsWith("---\n")) return { frontmatter: "", body: rest.slice(4) };
  const end = rest.indexOf("\n---\n");
  if (end === -1) return { frontmatter: null, body: raw };
  return { frontmatter: rest.slice(0, end), body: rest.slice(end + 5) };
}

/** display_title: frontmatter `title:`, else first `# ` heading, else stem.
 *  Mirrors cairn-domain Note::display_title. */
function displayTitle(path: string, raw: string): string {
  const { frontmatter, body } = splitFrontmatter(raw);
  if (frontmatter !== null) {
    for (const line of frontmatter.split("\n")) {
      const t = line.trimStart();
      if (t.startsWith("title:")) {
        // Two-pass quote strip mirrors Rust's trim_matches('"').trim_matches('\'').
        const v = t
          .slice("title:".length)
          .trim()
          .replace(/^"+|"+$/g, "")
          .replace(/^'+|'+$/g, "")
          .trim();
        if (v) return v;
      }
    }
  }
  for (const line of body.split("\n")) {
    const t = line.trimStart();
    if (t.startsWith("# ")) {
      const v = t.slice(2).trim();
      if (v) return v;
    }
  }
  return stem(path);
}

/** Rewrite `[[oldStem]]` / `[[oldStem|alias]]` → newStem (link target only). */
function rewriteWikilinks(
  raw: string,
  oldStem: string,
  newStem: string,
): string {
  const esc = oldStem.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return raw.replace(
    new RegExp(`\\[\\[${esc}(\\]\\]|\\|)`, "g"),
    `[[${newStem}$1`,
  );
}

type SealChange = {
  op: "add" | "edit" | "delete";
  path: string;
  /** display_title of the note, as the engine's DiffSummary carries it —
   *  frontmatter `title:` → first `# ` heading → stem, NOT the bare stem. */
  title: string;
  added: number;
  removed: number;
};

function countWords(s: string): number {
  return (s.match(/\S+/g) ?? []).length;
}

/** Counts suffix, zero sides elided — mirrors the engine's `counts()`. */
function counts(added: number, removed: number): string {
  if (added === 0 && removed === 0) return "";
  if (removed === 0) return ` (+${added} words)`;
  if (added === 0) return ` (−${removed} words)`;
  return ` (+${added}/−${removed} words)`;
}

/** Mirror of the engine's deterministic message template
 *  (cairn-app `commit_msg::commit_message`). The mock must render byte-identical
 *  labels: the UI's regex fallback is exercised against these. */
function sealMessage(changes: SealChange[]): string {
  if (changes.length === 0) return "Checkpoint";
  if (changes.length > 1) {
    const titles = changes
      .slice(0, 3)
      .map((x) => `"${x.title}"`)
      .join(", ");
    return `Update ${changes.length} notes: ${titles}${
      changes.length > 3 ? "…" : ""
    }`;
  }
  const [ch] = changes;
  if (ch.op === "add") return `Add "${ch.title}"${counts(ch.added, 0)}`;
  if (ch.op === "delete") return `Delete "${ch.title}"`;
  return `Edit "${ch.title}"${counts(ch.added, ch.removed)}`;
}

/** Detach revisions from the mock's internal arrays before handing them out.
 *  `name_version` mutates the shared revision objects in place, so a caller
 *  holding a live reference would see history it never re-queried. */
function snapshot(revs: Revision[]): Revision[] {
  return revs.map((r) => ({ ...r }));
}

/** Optional seeded git history per note: the revision list (newest first) and
 *  the note's contents at each revision id. */
export interface HistoryFixture {
  revisions: Revision[];
  contents: Record<string, string>;
}

/** A full note-set at one revspec — the mock's synthetic vault history, used by
 *  graph_at / graph_diff so the temporal surface is testable offline. */
export interface VaultSnapshot {
  notes: Record<string, string>;
}

/** In-memory faithful mock of the cairn engine + cairn-service dispatch. */
export class MockClient implements CairnClient {
  private notes: Map<string, string>;
  private history: Map<string, HistoryFixture>;
  private vaultSnapshots: Map<string, VaultSnapshot>;
  private subscribers = new Set<(e: Event) => void>();
  private commitSeq = 0;
  // C0 seal baseline: the note contents as of the last sealed version.
  private sealedSnapshot: Map<string, string>;
  private plugins: PluginSummary[] = [
    {
      id: "demo",
      name: "Demo plugin",
      version: "1.0.0",
      commands: [{ id: "stamp", title: "Insert stamp note" }],
      contributions: [
        {
          id: "stamp-list",
          slot: "sidebar.section",
          widget: {
            kind: "list",
            items: [
              {
                id: "s",
                label: "Insert stamp",
                icon: null,
                command: "stamp",
                args: null,
              },
            ],
          },
          title: null,
          icon: null,
          order: null,
        },
        {
          id: "stamp-action",
          slot: "topbar.action",
          widget: {
            kind: "action",
            label: "Stamp",
            icon: "note",
            command: "stamp",
            args: null,
          },
          title: null,
          icon: null,
          order: null,
        },
        {
          id: "stamp-cmd",
          slot: "command",
          widget: {
            kind: "action",
            label: "Insert stamp (cmd)",
            icon: null,
            command: "stamp",
            args: null,
          },
          title: null,
          icon: null,
          order: null,
        },
      ],
    },
    {
      id: "bare",
      name: "Bare plugin",
      version: "0.1.0",
      commands: [],
      contributions: [],
    },
    {
      id: "wordcount",
      name: "Word Count",
      version: "1.0.0",
      commands: [],
      uiRoot: "(mock)/wordcount/ui",
      contributions: [
        {
          id: "wc-panel",
          slot: "panel.main",
          title: "Word Count",
          icon: "info",
          order: 100,
          widget: {
            kind: "iframe",
            height: 120,
            entry: "index.html",
          },
        },
      ],
      capabilities: ["activeNote.read"],
    },
  ];

  // Vault-wide commit history (newest first), returned by the `vault_history`
  // query. Seeded like the per-note fixtures so the temporal surface is
  // testable offline.
  private vaultHistory: Revision[];

  // Structural subset (revisions that changed the link graph), returned by the
  // `structural_revisions` query. The engine pre-filters this server-side, so
  // it's a separate fixture — the UI never derives it from `vaultHistory`.
  private structuralHistory: Revision[];

  constructor(
    seed: Record<string, string> = {},
    history: Record<string, HistoryFixture> = {},
    vaultSnapshots: Record<string, VaultSnapshot> = {},
    vaultHistory: Revision[] = [],
    structuralHistory: Revision[] = [],
  ) {
    this.notes = new Map(Object.entries(seed));
    this.sealedSnapshot = new Map(this.notes);
    this.history = new Map(Object.entries(history));
    this.vaultSnapshots = new Map(Object.entries(vaultSnapshots));
    this.vaultHistory = vaultHistory;
    this.structuralHistory = structuralHistory;
  }

  // The mock channel never fails to attach, so it ignores the contract's
  // optional `onError` (a narrower impl still satisfies the wider interface).
  subscribe(cb: (e: Event) => void): Unsubscribe {
    this.subscribers.add(cb);
    return () => this.subscribers.delete(cb);
  }

  private emit(e: Event): void {
    // Asynchronous so subscribers see push-after-the-fact timing.
    queueMicrotask(() => this.subscribers.forEach((cb) => cb(e)));
  }

  private stemIndex(): Map<string, string> {
    const byStem = new Map<string, string>();
    for (const path of this.notes.keys()) byStem.set(stem(path), path);
    return byStem;
  }

  /** Diff notes vs the last sealed snapshot; advances the snapshot. */
  private sealChanges(): SealChange[] {
    const changes: SealChange[] = [];
    for (const [path, now] of this.notes) {
      const then = this.sealedSnapshot.get(path);
      if (then === now) continue;
      const d = countWords(now) - countWords(then ?? "");
      changes.push({
        op: then === undefined ? "add" : "edit",
        path,
        title: displayTitle(path, now),
        added: Math.max(d, 0),
        removed: Math.max(-d, 0),
      });
    }
    for (const [path, then] of this.sealedSnapshot) {
      if (!this.notes.has(path))
        changes.push({
          op: "delete",
          path,
          // The note is gone; the engine titles it from the pre-image.
          title: displayTitle(path, then),
          added: 0,
          removed: countWords(then),
        });
    }
    this.sealedSnapshot = new Map(this.notes);
    return changes;
  }

  async sendCommand(c: Command): Promise<CommandResponse> {
    switch (c.type) {
      case "write_note":
        this.notes.set(c.path, c.contents);
        this.emit({ type: "note_changed", path: c.path });
        this.emit({ type: "reindexed", count: this.notes.size });
        return { type: "done" };
      case "delete_note": {
        // The real store errors when deleting a missing note (fs NotFound ->
        // PortError::NotFound -> ContractError::NotFound).
        if (!this.notes.has(c.path)) {
          const err: ContractError = { type: "not_found", what: c.path };
          throw err;
        }
        this.notes.delete(c.path);
        this.emit({ type: "note_deleted", path: c.path });
        this.emit({ type: "reindexed", count: this.notes.size });
        return { type: "done" };
      }
      case "commit": {
        // An explicit message only overrides the *subject*: the daemon still
        // computes the summary, attaches the revision to per-note history, and
        // reports `nothing_to_commit` when nothing is dirty. Verified against a
        // live cairn-daemon — the two branches differ in the message alone.
        const changes = this.sealChanges();
        // Nothing dirty: the engine reports `nothing_to_commit` rather than
        // creating an empty version. Callers must treat this as success.
        if (changes.length === 0) return { type: "nothing_to_commit" };
        // `message: null` ⇒ engine-side policy generates it (mirrored here).
        const message = c.message ?? sealMessage(changes);
        this.commitSeq += 1;
        const commit = `c${String(this.commitSeq).padStart(4, "0")}`;
        const rev: Revision = {
          id: commit,
          message,
          author: "mock",
          timestamp_secs: Math.floor(Date.now() / 1000),
          summary: {
            files_changed: changes.length,
            words_added: changes.reduce((n, s) => n + s.added, 0),
            words_removed: changes.reduce((n, s) => n + s.removed, 0),
          },
          name: null,
        };
        this.vaultHistory.unshift(rev);
        for (const s of changes) {
          const fix = this.history.get(s.path) ?? {
            revisions: [],
            contents: {},
          };
          fix.revisions.unshift(rev);
          const now = this.notes.get(s.path);
          if (now !== undefined) fix.contents[commit] = now;
          this.history.set(s.path, fix);
        }
        this.emit({ type: "committed", commit });
        return { type: "committed", commit };
      }
      case "name_version": {
        const mark = (r: Revision) => {
          if (r.id === c.commit) r.name = c.name;
        };
        this.vaultHistory.forEach(mark);
        for (const fix of this.history.values()) fix.revisions.forEach(mark);
        return { type: "done" };
      }
      case "invoke_plugin_command": {
        if (c.plugin === "demo" && c.command === "stamp") {
          this.notes.set("stamp.md", "# Stamp\n");
          this.emit({ type: "note_changed", path: "stamp.md" });
          this.emit({ type: "reindexed", count: this.notes.size });
          // Echo args.n into the result so callers can observe arg passthrough.
          const n =
            c.args && typeof c.args === "object" && "n" in c.args
              ? " " + (c.args as { n: unknown }).n
              : "";
          return { type: "plugin_result", result: `stamp.md${n}` };
        }
        const err: ContractError = {
          type: "invalid_request",
          message: `unknown plugin command ${c.plugin}/${c.command}`,
        };
        throw err;
      }
      case "rename_note": {
        if (!this.notes.has(c.from)) {
          const err: ContractError = { type: "not_found", what: c.from };
          throw err;
        }
        if (this.notes.has(c.to)) {
          const err: ContractError = {
            type: "invalid_request",
            message: `already exists: ${c.to}`,
          };
          throw err;
        }
        const body = this.notes.get(c.from) as string;
        this.notes.delete(c.from);
        this.notes.set(c.to, body);
        const oldStem = stem(c.from);
        const newStem = stem(c.to);
        if (oldStem !== newStem) {
          for (const [p, raw] of [...this.notes]) {
            if (p === c.to) continue;
            const rewritten = rewriteWikilinks(raw, oldStem, newStem);
            if (rewritten !== raw) this.notes.set(p, rewritten);
          }
        }
        this.emit({ type: "note_deleted", path: c.from });
        this.emit({ type: "note_changed", path: c.to });
        this.emit({ type: "reindexed", count: this.notes.size });
        return { type: "done" };
      }
      case "restore_note": {
        const fix = this.history.get(c.path);
        const contents = fix?.contents[c.revision];
        if (contents === undefined) {
          const err: ContractError = { type: "not_found", what: c.revision };
          throw err;
        }
        this.notes.set(c.path, contents);
        this.emit({ type: "note_changed", path: c.path });
        this.emit({ type: "reindexed", count: this.notes.size });
        return { type: "done" };
      }
      default: {
        throw new Error(`mock: unsupported command ${(c as Command).type}`);
      }
    }
  }

  private buildGraph(
    notes: Map<string, string>,
    scope: GraphScope,
  ): { nodes: GraphNode[]; edges: GraphEdge[] } {
    const byStem = new Map<string, string>();
    for (const path of notes.keys()) byStem.set(stem(path), path);
    // Enriched nodes: path + display title + frontmatter tags + a synthetic
    // mtime. mtime is display-only for now (dual-basis; not used for diffing),
    // so a stable placeholder is fine until the temporal mock seeds real ones.
    // `degree` is attached at return (via withDegree) from the returned edge
    // set so it mirrors the engine's "within the returned graph" semantics.
    const nodes = [...notes.entries()]
      .map(([path, raw]) => ({
        path,
        title: displayTitle(path, raw),
        tags: extractTags(raw),
        mtime_secs: 0,
      }))
      .sort((a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0));
    const seen = new Set<string>();
    const edges: GraphEdge[] = [];
    for (const [from, raw] of notes.entries()) {
      for (const target of extractLinks(splitFrontmatter(raw).body)) {
        const to = byStem.get(target);
        if (to && !seen.has(`${from} ${to}`)) {
          seen.add(`${from} ${to}`);
          edges.push({ from, to });
        }
      }
    }
    edges.sort((a, b) =>
      a.from === b.from
        ? a.to < b.to
          ? -1
          : a.to > b.to
            ? 1
            : 0
        : a.from < b.from
          ? -1
          : 1,
    );
    // Attach undirected degree (forward links + backlinks) over the returned
    // edge set, producing the GraphNode shape the contract now requires.
    const withDegree = (
      ns: (typeof nodes)[number][],
      es: GraphEdge[],
    ): GraphNode[] => {
      const deg = new Map<string, number>();
      for (const e of es) {
        deg.set(e.from, (deg.get(e.from) ?? 0) + 1);
        deg.set(e.to, (deg.get(e.to) ?? 0) + 1);
      }
      return ns.map((n) => ({ ...n, degree: deg.get(n.path) ?? 0 }));
    };
    if (scope.type === "focused") {
      // BFS over undirected edges from the focus note to `depth` hops,
      // mirroring GraphScope::Focused. Unknown focus → empty graph.
      const { path: root, depth } = scope;
      if (!nodes.some((n) => n.path === root)) return { nodes: [], edges: [] };
      const adj = new Map<string, string[]>();
      const addAdj = (a: string, b: string) =>
        (adj.get(a) ?? adj.set(a, []).get(a)!).push(b);
      for (const e of edges) {
        addAdj(e.from, e.to);
        addAdj(e.to, e.from);
      }
      const reached = new Set([root]);
      let frontier = [root];
      for (let d = 0; d < depth && frontier.length; d++) {
        const next: string[] = [];
        for (const n of frontier)
          for (const m of adj.get(n) ?? [])
            if (!reached.has(m)) {
              reached.add(m);
              next.push(m);
            }
        frontier = next;
      }
      const fEdges = edges.filter(
        (e) => reached.has(e.from) && reached.has(e.to),
      );
      return {
        nodes: withDegree(
          nodes.filter((n) => reached.has(n.path)),
          fEdges,
        ),
        edges: fEdges,
      };
    }
    return { nodes: withDegree(nodes, edges), edges };
  }

  async runQuery(q: Query): Promise<QueryResponse> {
    switch (q.type) {
      case "get_note":
      case "render_note": {
        // The mock does not render markdown, so `render_note` returns the raw
        // contents just like `get_note` — both answer with a `note` response.
        const contents = this.notes.get(q.path);
        if (contents === undefined) {
          const err: ContractError = { type: "not_found", what: q.path };
          throw err;
        }
        return { type: "note", contents };
      }
      case "search": {
        const needle = q.query.toLowerCase();
        const results: SearchResult[] = [];
        for (const [path, raw] of this.notes) {
          const body = splitFrontmatter(raw).body;
          const lowerBody = body.toLowerCase();
          const idx = lowerBody.indexOf(needle);
          const pathMatch = path.toLowerCase().includes(needle);
          if (idx === -1 && !pathMatch) continue;
          let snippet: string;
          let highlights: [number, number][];
          if (idx !== -1) {
            const start = Math.max(0, idx - 20);
            const end = Math.min(body.length, idx + needle.length + 20);
            snippet = body.slice(start, end);
            highlights = [[idx - start, idx - start + needle.length]];
          } else {
            snippet = body.slice(0, 40);
            highlights = [];
          }
          const score = lowerBody.split(needle).length - 1;
          results.push({ path, score, snippet, highlights });
        }
        results.sort((a, b) =>
          a.path < b.path ? -1 : a.path > b.path ? 1 : 0,
        );
        return { type: "search_results", results };
      }
      case "get_backlinks": {
        const byStem = this.stemIndex();
        const paths = [
          ...new Set(
            [...this.notes.entries()]
              .filter(([, raw]) =>
                extractLinks(splitFrontmatter(raw).body).some(
                  (t) => byStem.get(t) === q.path,
                ),
              )
              .map(([path]) => path),
          ),
        ].sort();
        return { type: "paths", paths };
      }
      case "list_notes": {
        const notes: NoteSummary[] = [...this.notes.entries()]
          .map(([path, raw]) => ({
            path,
            title: displayTitle(path, raw),
            tags: extractTags(raw),
          }))
          .sort((a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0));
        return { type: "notes", notes };
      }
      case "get_graph":
        return { type: "graph", ...this.buildGraph(this.notes, q.scope) };
      case "list_tags": {
        const counts = new Map<string, number>();
        for (const raw of this.notes.values()) {
          for (const tag of extractTags(raw)) {
            counts.set(tag, (counts.get(tag) ?? 0) + 1);
          }
        }
        const tags = [...counts.entries()]
          .map(([tag, count]) => ({ tag, count }))
          .sort((a, b) => (a.tag < b.tag ? -1 : a.tag > b.tag ? 1 : 0));
        return { type: "tags", tags };
      }
      case "notes_by_tag": {
        const paths = [...this.notes.entries()]
          .filter(([, raw]) => extractTags(raw).includes(q.tag))
          .map(([path]) => path)
          .sort();
        return { type: "paths", paths };
      }
      case "list_plugins":
        return { type: "plugins", plugins: this.plugins };
      case "note_history": {
        const fix = this.history.get(q.path);
        // Snapshot: a real transport hands back deserialized values, so callers
        // must never receive (and be able to mutate) the mock's own state.
        return { type: "history", revisions: snapshot(fix?.revisions ?? []) };
      }
      case "vault_history": {
        // Newest-first, capped at `limit` (null = all).
        const revs =
          q.limit === null
            ? this.vaultHistory
            : this.vaultHistory.slice(0, q.limit);
        return { type: "history", revisions: snapshot(revs) };
      }
      case "structural_revisions": {
        // Pre-filtered structural subset, newest-first, capped at `limit`.
        const revs =
          q.limit === null
            ? this.structuralHistory
            : this.structuralHistory.slice(0, q.limit);
        return { type: "history", revisions: snapshot(revs) };
      }
      case "note_at": {
        const fix = this.history.get(q.path);
        const contents = fix?.contents[q.revision];
        if (contents === undefined) {
          const err: ContractError = { type: "not_found", what: q.revision };
          throw err;
        }
        return { type: "note", contents };
      }
      case "graph_at": {
        const snap = this.vaultSnapshots.get(q.revision);
        if (!snap) {
          const err: ContractError = { type: "not_found", what: q.revision };
          throw err;
        }
        const notes = new Map(Object.entries(snap.notes));
        return { type: "graph", ...this.buildGraph(notes, q.scope) };
      }
      case "graph_diff": {
        const load = (rev: string): Map<string, string> => {
          const snap = this.vaultSnapshots.get(rev);
          if (!snap) {
            const err: ContractError = { type: "not_found", what: rev };
            throw err;
          }
          return new Map(Object.entries(snap.notes));
        };
        const a = this.buildGraph(load(q.from), q.scope);
        const b = this.buildGraph(load(q.to), q.scope);
        const aByPath = new Map(a.nodes.map((n) => [n.path, n]));
        const bNodes = new Set(b.nodes.map((n) => n.path));
        const eKey = (e: GraphEdge) => `${e.from}->${e.to}`;
        const aEdges = new Set(a.edges.map(eKey));
        const bEdges = new Set(b.edges.map(eKey));
        // Present in both revisions but with changed metadata (title, degree,
        // tags, or mtime); carries the `to`-revision values — mirrors engine.
        const metaChanged = (x: GraphNode, y: GraphNode) =>
          x.title !== y.title ||
          x.degree !== y.degree ||
          x.mtime_secs !== y.mtime_secs ||
          x.tags.length !== y.tags.length ||
          x.tags.some((t, i) => t !== y.tags[i]);
        return {
          type: "graph_diff",
          nodes_added: b.nodes.filter((n) => !aByPath.has(n.path)),
          nodes_removed: a.nodes.filter((n) => !bNodes.has(n.path)),
          nodes_changed: b.nodes.filter((n) => {
            const prev = aByPath.get(n.path);
            return prev !== undefined && metaChanged(prev, n);
          }),
          edges_added: b.edges.filter((e) => !aEdges.has(eKey(e))),
          edges_removed: a.edges.filter((e) => !bEdges.has(eKey(e))),
        };
      }
      case "get_suggestions": {
        // Dev/demo suggestions so the overlay renders real dashed edges against
        // the mock backend (no engine). Curated between fixture notes that are
        // NOT explicitly linked; filtered to notes that actually exist. Weight
        // drives link width, `why` drives the hover tooltip.
        const curated: SuggestedEdge[] = [
          {
            from: "projects/demo.md",
            to: "todo.md",
            weight: 0.82,
            why: "shared tag: rust",
          },
          {
            from: "projects/demo.md",
            to: "ideas.md",
            weight: 0.61,
            why: "shared tag: ideas",
          },
          {
            from: "kitchensink.md",
            to: "index.md",
            weight: 0.34,
            why: "co-mention: ideas",
          },
        ].filter((e) => this.notes.has(e.from) && this.notes.has(e.to));
        const scope = q.scope;
        const suggestions =
          scope.type === "note"
            ? curated.filter(
                (e) => e.from === scope.path || e.to === scope.path,
              )
            : curated;
        return { type: "suggestions", suggestions };
      }
      default: {
        throw new Error(`mock: unsupported query ${(q as Query).type}`);
      }
    }
  }

  noteTags(): Promise<Record<string, string[]>> {
    const out: Record<string, string[]> = {};
    for (const [path, content] of this.notes) out[path] = extractTags(content);
    return Promise.resolve(out);
  }

  /** Deterministic fixtures: one tombstoned block (former content) + one
   *  overwritten block (an LWW-loser), keyed off the note so tests are
   *  stable. `restore` mutates `this.notes` so a subsequent `get_note`
   *  reflects the appended text. */
  openRecovery(note: string): Promise<RecoverySession> {
    const content = this.notes.get(note) ?? "";
    void content;
    const blocks: WireRecoverableBlock[] = [
      {
        id: { replica: "1", counter: "2" },
        tombstoned: true,
        versions: ["## Risks\n- vendor lock-in"],
      },
      {
        id: { replica: "1", counter: "3" },
        tombstoned: false,
        versions: ["Ship date: March 14"],
      },
    ];
    const session: RecoverySession = {
      blocks,
      restore: (id: WireBlockId, versionIndex: number) => {
        const b = blocks.find((x) => x.id.counter === id.counter);
        const text = b?.versions[versionIndex] ?? "";
        if (text)
          this.notes.set(
            note,
            (this.notes.get(note) ?? "") + "\n" + text + "\n",
          );
        return Promise.resolve();
      },
      close: () => {},
    };
    return Promise.resolve(session);
  }

  /** Mock streaming agent: emits a `sources` frame first (authoritative
   *  citations, drawn from a real seeded note), then a tool round, then text
   *  deltas, then completes. A query containing "fail" emits the failed path
   *  instead. Events fire on chained microtasks (ordered + async); unsubscribe
   *  cancels mid-stream. */
  ask(req: AskRequest, onEvent: (e: AnswerEvent) => void): Unsubscribe {
    let cancelled = false;
    const fail = req.query.toLowerCase().includes("fail");
    const firstPath = [...this.notes.keys()][0];
    const firstStem = firstPath ? stem(firstPath) : undefined;
    const cite = firstStem ? ` [[${firstStem}]]` : "";
    const sources: AnswerEvent = {
      type: "sources",
      paths: firstPath ? [firstPath] : [],
    };
    const seq: AnswerEvent[] = fail
      ? [
          sources,
          { type: "tool_started", tool: "search_notes" },
          { type: "tool_completed", tool: "search_notes", ok: true },
          { type: "failed", message: "stream interrupted (mock)" },
        ]
      : [
          sources,
          { type: "tool_started", tool: "search_notes" },
          { type: "tool_completed", tool: "search_notes", ok: true },
          { type: "text_delta", text: "Based on your notes, " },
          { type: "text_delta", text: "the answer is grounded in" },
          { type: "text_delta", text: cite },
          { type: "text_delta", text: " and complete." },
          { type: "turn_completed" },
          { type: "completed" },
        ];
    const step = (i: number) => {
      if (cancelled || i >= seq.length) return;
      onEvent(seq[i]);
      queueMicrotask(() => step(i + 1));
    };
    queueMicrotask(() => step(0));
    return () => {
      cancelled = true;
    };
  }

  /** Test/dev helper: current note paths. */
  paths(): string[] {
    return [...this.notes.keys()].sort();
  }

  /** Test hook: the handlers passed to the most recent `openCollab`, so tests
   *  can drive `onForeignOp`/etc. deterministically. */
  mockCollabHandlers: CollabHandlers | null = null;

  openCollab(_note: string, handlers: CollabHandlers): CollabSession {
    // Inert: the mock has no peers, so nothing fires on its own. Tests drive
    // handlers via `mockCollabHandlers`.
    this.mockCollabHandlers = handlers;
    return { close: () => {} };
  }
}
