import { describe, it, expect, vi } from "vitest";
import { MockClient } from "./mock";
import type { Event, AnswerEvent } from "../contract";

function freshNotes() {
  return { "a.md": "links to [[b]]", "b.md": "target note" };
}

describe("MockClient", () => {
  it("get_note returns the note variant", async () => {
    const c = new MockClient(freshNotes());
    expect(await c.runQuery({ type: "get_note", path: "a.md" })).toEqual({
      type: "note",
      contents: "links to [[b]]",
    });
  });

  it("get_note rejects with not_found for a missing note", async () => {
    const c = new MockClient(freshNotes());
    await expect(
      c.runQuery({ type: "get_note", path: "missing.md" }),
    ).rejects.toEqual({
      type: "not_found",
      what: "missing.md",
    });
  });

  it("search matches body and path, case-insensitive, sorted by path", async () => {
    const c = new MockClient({
      "zeta.md": "alpha note",
      "alpha.md": "zeta body",
    });
    expect(await c.runQuery({ type: "search", query: "ALPHA" })).toEqual({
      type: "search_results",
      results: [
        { path: "alpha.md", score: 0, snippet: "zeta body", highlights: [] },
        {
          path: "zeta.md",
          score: 1,
          snippet: "alpha note",
          highlights: [[0, 5]],
        },
      ],
    });
  });

  it("get_backlinks resolves by stem, sorted and deduped", async () => {
    const c = new MockClient(freshNotes());
    expect(await c.runQuery({ type: "get_backlinks", path: "b.md" })).toEqual({
      type: "paths",
      paths: ["a.md"],
    });
  });

  it("list_notes returns a NoteSummary per note with display titles, sorted by path", async () => {
    const c = new MockClient({
      "a.md": "---\ntitle: Alpha\n---\nbody",
      "b.md": "# Heading B\ntext",
      "c.md": "no title here",
    });
    expect(await c.runQuery({ type: "list_notes" })).toEqual({
      type: "notes",
      notes: [
        { path: "a.md", title: "Alpha", tags: [] },
        { path: "b.md", title: "Heading B", tags: [] },
        { path: "c.md", title: "c", tags: [] },
      ],
    });
  });

  it("get_graph returns sorted nodes and resolved directed edges", async () => {
    const c = new MockClient(freshNotes());
    expect(
      await c.runQuery({ type: "get_graph", scope: { type: "full" } }),
    ).toEqual({
      type: "graph",
      nodes: [
        { path: "a.md", title: "a", degree: 1, tags: [], mtime_secs: 0 },
        { path: "b.md", title: "b", degree: 1, tags: [], mtime_secs: 0 },
      ],
      edges: [{ from: "a.md", to: "b.md" }],
    });
  });

  it("get_graph populates undirected degree and frontmatter tags per node", async () => {
    // hub links to two leaves; hub has frontmatter tags, leaves have none.
    const c = new MockClient({
      "hub.md": "---\ntags: [x, y]\n---\n[[leaf1]] and [[leaf2]]",
      "leaf1.md": "one",
      "leaf2.md": "two",
    });
    const res = await c.runQuery({
      type: "get_graph",
      scope: { type: "full" },
    });
    expect(res).toEqual({
      type: "graph",
      nodes: [
        {
          path: "hub.md",
          title: "hub",
          degree: 2,
          tags: ["x", "y"],
          mtime_secs: 0,
        },
        {
          path: "leaf1.md",
          title: "leaf1",
          degree: 1,
          tags: [],
          mtime_secs: 0,
        },
        {
          path: "leaf2.md",
          title: "leaf2",
          degree: 1,
          tags: [],
          mtime_secs: 0,
        },
      ],
      edges: [
        { from: "hub.md", to: "leaf1.md" },
        { from: "hub.md", to: "leaf2.md" },
      ],
    });
  });

  it("get_graph focused scope returns the neighborhood within depth", async () => {
    // a -> b -> c chain; focus b @ depth 1 reaches a and c, not the isolated d.
    const c = new MockClient({
      "a.md": "links [[b]]",
      "b.md": "links [[c]]",
      "c.md": "leaf",
      "d.md": "isolated",
    });
    expect(
      await c.runQuery({
        type: "get_graph",
        scope: { type: "focused", path: "b.md", depth: 1 },
      }),
    ).toEqual({
      type: "graph",
      nodes: [
        { path: "a.md", title: "a", degree: 1, tags: [], mtime_secs: 0 },
        { path: "b.md", title: "b", degree: 2, tags: [], mtime_secs: 0 },
        { path: "c.md", title: "c", degree: 1, tags: [], mtime_secs: 0 },
      ],
      edges: [
        { from: "a.md", to: "b.md" },
        { from: "b.md", to: "c.md" },
      ],
    });
  });

  it("graph_at builds the graph from the vault snapshot at that revspec", async () => {
    const c = new MockClient(
      freshNotes(),
      {},
      {
        r1: { notes: { "a.md": "start" } },
        r2: { notes: { "a.md": "links [[b]]", "b.md": "hi" } },
      },
    );
    expect(
      await c.runQuery({
        type: "graph_at",
        revision: "r2",
        scope: { type: "full" },
      }),
    ).toEqual({
      type: "graph",
      nodes: [
        { path: "a.md", title: "a", degree: 1, tags: [], mtime_secs: 0 },
        { path: "b.md", title: "b", degree: 1, tags: [], mtime_secs: 0 },
      ],
      edges: [{ from: "a.md", to: "b.md" }],
    });
  });

  it("graph_at rejects an unknown revspec", async () => {
    const c = new MockClient(freshNotes());
    await expect(
      c.runQuery({
        type: "graph_at",
        revision: "nope",
        scope: { type: "full" },
      }),
    ).rejects.toEqual({ type: "not_found", what: "nope" });
  });

  it("graph_diff reports added, removed and changed nodes and edges", async () => {
    const c = new MockClient(
      freshNotes(),
      {},
      {
        r1: { notes: { "a.md": "lone" } },
        r2: { notes: { "a.md": "links [[b]]", "b.md": "hi" } },
      },
    );
    expect(
      await c.runQuery({
        type: "graph_diff",
        from: "r1",
        to: "r2",
        scope: { type: "full" },
      }),
    ).toEqual({
      type: "graph_diff",
      nodes_added: [
        { path: "b.md", title: "b", degree: 1, tags: [], mtime_secs: 0 },
      ],
      nodes_removed: [],
      // a.md gains the link to b, so its degree changes 0 → 1.
      nodes_changed: [
        { path: "a.md", title: "a", degree: 1, tags: [], mtime_secs: 0 },
      ],
      edges_added: [{ from: "a.md", to: "b.md" }],
      edges_removed: [],
    });
  });

  it("write_note upserts and emits note_changed then reindexed; returns done", async () => {
    const c = new MockClient(freshNotes());
    const events: Event[] = [];
    c.subscribe((e) => events.push(e));
    const res = await c.sendCommand({
      type: "write_note",
      path: "c.md",
      contents: "new [[a]]",
    });
    expect(res).toEqual({ type: "done" });
    await vi.waitFor(() =>
      expect(events).toEqual([
        { type: "note_changed", path: "c.md" },
        { type: "reindexed", count: 3 },
      ]),
    );
  });

  it("delete_note removes and emits note_deleted then reindexed; returns done", async () => {
    const c = new MockClient(freshNotes());
    const events: Event[] = [];
    c.subscribe((e) => events.push(e));
    const res = await c.sendCommand({ type: "delete_note", path: "b.md" });
    expect(res).toEqual({ type: "done" });
    await vi.waitFor(() =>
      expect(events).toEqual([
        { type: "note_deleted", path: "b.md" },
        { type: "reindexed", count: 1 },
      ]),
    );
    expect(await c.runQuery({ type: "search", query: "target" })).toEqual({
      type: "search_results",
      results: [],
    });
  });

  it("delete_note rejects with not_found for a missing note", async () => {
    const c = new MockClient(freshNotes());
    await expect(
      c.sendCommand({ type: "delete_note", path: "ghost.md" }),
    ).rejects.toEqual({
      type: "not_found",
      what: "ghost.md",
    });
  });

  it("commit returns committed with a short id and emits committed", async () => {
    const c = new MockClient(freshNotes());
    const events: Event[] = [];
    c.subscribe((e) => events.push(e));
    // Something must be dirty: like the daemon, a commit with nothing to seal
    // reports nothing_to_commit regardless of whether a message was given.
    await c.sendCommand({ type: "write_note", path: "a.md", contents: "x y" });
    const res = await c.sendCommand({ type: "commit", message: "first" });
    expect(res).toEqual({ type: "committed", commit: "c0001" });
    await vi.waitFor(() =>
      expect(events).toContainEqual({ type: "committed", commit: "c0001" }),
    );
  });

  it("noteTags parses tags from note content", async () => {
    const c = new MockClient({
      "a.md": "---\ntags: [x, y]\n---\nbody #z",
      "b.md": "plain",
    });
    expect(await c.noteTags()).toEqual({ "a.md": ["x", "y"], "b.md": [] });
  });

  it("list_tags counts distinct frontmatter tags, sorted", async () => {
    const c = new MockClient({
      "a.md": "---\ntags: [rust, ideas]\n---\nx",
      "b.md": "---\ntags: [rust]\n---\ny",
      "c.md": "no tags",
    });
    expect(await c.runQuery({ type: "list_tags" })).toEqual({
      type: "tags",
      tags: [
        { tag: "ideas", count: 1 },
        { tag: "rust", count: 2 },
      ],
    });
  });
  it("notes_by_tag returns matching paths, sorted", async () => {
    const c = new MockClient({
      "a.md": "---\ntags: [rust]\n---\nx",
      "b.md": "---\ntags: [ideas]\n---\ny",
      "z.md": "---\ntags: [rust]\n---\nz",
    });
    expect(await c.runQuery({ type: "notes_by_tag", tag: "rust" })).toEqual({
      type: "paths",
      paths: ["a.md", "z.md"],
    });
  });
  it("rename_note moves the note and rewrites [[wikilinks]] when the stem changes", async () => {
    const c = new MockClient({
      "a.md": "# A",
      "x.md": "see [[a]] and [[a|alias]]",
    });
    await c.sendCommand({ type: "rename_note", from: "a.md", to: "c.md" });
    expect(await c.runQuery({ type: "get_note", path: "c.md" })).toEqual({
      type: "note",
      contents: "# A",
    });
    await expect(
      c.runQuery({ type: "get_note", path: "a.md" }),
    ).rejects.toEqual({ type: "not_found", what: "a.md" });
    expect(await c.runQuery({ type: "get_note", path: "x.md" })).toEqual({
      type: "note",
      contents: "see [[c]] and [[c|alias]]",
    });
  });
  it("rename_note keeps links when only the folder changes (stem unchanged)", async () => {
    const c = new MockClient({ "a.md": "# A", "x.md": "see [[a]]" });
    await c.sendCommand({ type: "rename_note", from: "a.md", to: "sub/a.md" });
    expect(await c.runQuery({ type: "get_note", path: "x.md" })).toEqual({
      type: "note",
      contents: "see [[a]]",
    });
  });
  it("rename_note errors on a missing source and an existing target", async () => {
    const c = new MockClient({ "a.md": "x", "b.md": "y" });
    await expect(
      c.sendCommand({ type: "rename_note", from: "missing.md", to: "z.md" }),
    ).rejects.toEqual({ type: "not_found", what: "missing.md" });
    await expect(
      c.sendCommand({ type: "rename_note", from: "a.md", to: "b.md" }),
    ).rejects.toMatchObject({ type: "invalid_request" });
  });
  it("list_plugins returns the seeded demo + bare + wordcount plugins", async () => {
    const c = new MockClient({});
    const res = await c.runQuery({ type: "list_plugins" });
    expect(res.type).toBe("plugins");
    if (res.type !== "plugins") return;
    expect(res.plugins.map((p) => p.id)).toEqual(["demo", "bare", "wordcount"]);
    const demo = res.plugins[0];
    expect(demo).toMatchObject({
      id: "demo",
      name: "Demo plugin",
      version: "1.0.0",
      commands: [{ id: "stamp", title: "Insert stamp note" }],
    });
    // demo seeds contributions across all three slots; bare seeds none.
    expect(demo.contributions.map((c) => c.slot)).toEqual([
      "sidebar.section",
      "topbar.action",
      "command",
    ]);
    expect(res.plugins[1].contributions).toEqual([]);
  });
  it("invoke_plugin_command demo/stamp writes a note and returns its path", async () => {
    const c = new MockClient({});
    const res = await c.sendCommand({
      type: "invoke_plugin_command",
      plugin: "demo",
      command: "stamp",
      args: null,
    });
    expect(res).toEqual({ type: "plugin_result", result: "stamp.md" });
    expect(
      await c.runQuery({ type: "get_note", path: "stamp.md" }),
    ).toMatchObject({ type: "note" });
  });
  it("invoke_plugin_command errors on an unknown command", async () => {
    const c = new MockClient({});
    await expect(
      c.sendCommand({
        type: "invoke_plugin_command",
        plugin: "demo",
        command: "nope",
        args: null,
      }),
    ).rejects.toMatchObject({ type: "invalid_request" });
  });
});

describe("mock history ops", () => {
  function withHistory() {
    const revs: import("../contract").Revision[] = [
      {
        id: "r2",
        message: "second",
        timestamp_secs: 2,
        author: "tau",
        summary: null,
        name: null,
      },
      {
        id: "r1",
        message: "first",
        timestamp_secs: 1,
        author: "tau",
        summary: null,
        name: null,
      },
    ];
    return new MockClient(
      { "n.md": "current body" },
      {
        "n.md": { revisions: revs, contents: { r2: "body v2", r1: "body v1" } },
      },
    );
  }

  it("note_history returns seeded revisions newest-first", async () => {
    const c = withHistory();
    const res = await c.runQuery({ type: "note_history", path: "n.md" });
    expect(res).toEqual({
      type: "history",
      revisions: [
        {
          id: "r2",
          message: "second",
          timestamp_secs: 2,
          author: "tau",
          summary: null,
          name: null,
        },
        {
          id: "r1",
          message: "first",
          timestamp_secs: 1,
          author: "tau",
          summary: null,
          name: null,
        },
      ],
    });
  });

  it("note_history returns [] for a note with no seeded history", async () => {
    const c = withHistory();
    const res = await c.runQuery({ type: "note_history", path: "other.md" });
    expect(res).toEqual({ type: "history", revisions: [] });
  });

  it("note_at returns historical contents", async () => {
    const c = withHistory();
    const res = await c.runQuery({
      type: "note_at",
      path: "n.md",
      revision: "r1",
    });
    expect(res).toEqual({ type: "note", contents: "body v1" });
  });

  it("note_at rejects an unknown revision with not_found", async () => {
    const c = withHistory();
    await expect(
      c.runQuery({ type: "note_at", path: "n.md", revision: "nope" }),
    ).rejects.toMatchObject({ type: "not_found" });
  });

  it("vault_history returns seeded vault-wide revisions, capped by limit", async () => {
    const revs = [
      {
        id: "c3",
        message: "third",
        timestamp_secs: 3,
        author: "tau",
        summary: null,
        name: null,
      },
      {
        id: "c2",
        message: "second",
        timestamp_secs: 2,
        author: "tau",
        summary: null,
        name: null,
      },
      {
        id: "c1",
        message: "first",
        timestamp_secs: 1,
        author: "tau",
        summary: null,
        name: null,
      },
    ];
    const c = new MockClient(freshNotes(), {}, {}, revs);
    expect(await c.runQuery({ type: "vault_history", limit: null })).toEqual({
      type: "history",
      revisions: revs,
    });
    expect(await c.runQuery({ type: "vault_history", limit: 2 })).toEqual({
      type: "history",
      revisions: revs.slice(0, 2),
    });
  });

  it("vault_history returns [] when no history is seeded", async () => {
    const c = new MockClient(freshNotes());
    expect(await c.runQuery({ type: "vault_history", limit: null })).toEqual({
      type: "history",
      revisions: [],
    });
  });

  it("render_note returns the note's contents (mock does not render)", async () => {
    const c = new MockClient({ "a.md": "# hi" });
    expect(await c.runQuery({ type: "render_note", path: "a.md" })).toEqual({
      type: "note",
      contents: "# hi",
    });
  });

  it("restore_note overwrites the working copy and emits note_changed", async () => {
    const c = withHistory();
    const events: string[] = [];
    c.subscribe((e) => events.push(e.type));
    const res = await c.sendCommand({
      type: "restore_note",
      path: "n.md",
      revision: "r1",
    });
    expect(res).toEqual({ type: "done" });
    const note = await c.runQuery({ type: "get_note", path: "n.md" });
    expect(note).toEqual({ type: "note", contents: "body v1" });
    await new Promise<void>((r) => queueMicrotask(r));
    expect(events).toContain("note_changed");
  });
});

describe("MockClient.ask", () => {
  function collect(client: MockClient, q: string): Promise<AnswerEvent[]> {
    return new Promise((resolve) => {
      const events: AnswerEvent[] = [];
      client.ask({ query: q, top_k: null }, (e) => {
        events.push(e);
        if (e.type === "completed" || e.type === "failed") resolve(events);
      });
    });
  }

  it("emits sources first and completed last on success", async () => {
    const client = new MockClient({ "store.md": "# Store\n" });
    const events = await collect(client, "how does it work?");
    const types = events.map((e) => e.type);
    expect(types[0]).toBe("sources");
    expect(types).toContain("text_delta");
    expect(types[types.length - 1]).toBe("completed");
    const sources = events.find((e) => e.type === "sources");
    expect(sources).toEqual({ type: "sources", paths: ["store.md"] });
  });

  it("emits the failed path when the question contains 'fail'", async () => {
    const client = new MockClient({ "store.md": "x" });
    const events = await collect(client, "please fail");
    expect(events[events.length - 1]).toEqual({
      type: "failed",
      message: "stream interrupted (mock)",
    });
  });

  it("unsubscribe stops further events", async () => {
    const client = new MockClient({ "store.md": "x" });
    const seen: AnswerEvent[] = [];
    const unsub = client.ask({ query: "hello", top_k: null }, (e) =>
      seen.push(e),
    );
    unsub();
    await new Promise<void>((r) => queueMicrotask(() => queueMicrotask(r)));
    expect(seen.length).toBe(0);
  });
});

describe("MockClient get_suggestions", () => {
  const notes = {
    "index.md": "# Index [[ideas]] [[todo]]",
    "ideas.md": "# Ideas [[index]]",
    "todo.md": "---\ntags: [rust]\n---\n# Todo",
    "projects/demo.md": "---\ntags: [rust, ideas]\n---\n# Demo",
    "kitchensink.md": "# Kitchen sink [[ideas]]",
  };

  it("returns curated vault-scope suggestions between existing notes", async () => {
    const c = new MockClient(notes);
    const res = await c.runQuery({
      type: "get_suggestions",
      scope: { type: "vault" },
    });
    expect(res).toEqual({
      type: "suggestions",
      suggestions: [
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
      ],
    });
  });

  it("filters to suggestions touching the note for note scope", async () => {
    const c = new MockClient(notes);
    const res = await c.runQuery({
      type: "get_suggestions",
      scope: { type: "note", path: "todo.md" },
    });
    expect(res).toEqual({
      type: "suggestions",
      suggestions: [
        {
          from: "projects/demo.md",
          to: "todo.md",
          weight: 0.82,
          why: "shared tag: rust",
        },
      ],
    });
  });

  it("drops curated edges whose endpoints are absent", async () => {
    const c = new MockClient({ "todo.md": "x" });
    const res = await c.runQuery({
      type: "get_suggestions",
      scope: { type: "vault" },
    });
    expect(res).toEqual({ type: "suggestions", suggestions: [] });
  });
});

describe("MockClient.openCollab", () => {
  it("openCollab returns an inert session and exposes handlers for driving", () => {
    const c = new MockClient({ "n.md": "# N\n" });
    let opSeen: string | null = null;
    const session = c.openCollab("n.md", {
      onForeignOp: (note) => {
        opSeen = note;
      },
    });
    expect(typeof session.close).toBe("function");
    // No peers in the mock: nothing fired on its own.
    expect(opSeen).toBeNull();
    // Tests can drive a foreign op through the captured handlers.
    c.mockCollabHandlers?.onForeignOp?.("n.md", {
      op: "delete",
      id: { replica: "1", counter: "2" } as never,
      lamport: "5" as never,
    });
    expect(opSeen).toBe("n.md");
    session.close(); // idempotent, no throw
    session.close();
  });
});

describe("MockClient.openRecovery", () => {
  it("returns retained blocks and restore() reflects into the note", async () => {
    const c = new MockClient({ "draft.md": "# Draft\nkeep\n" });
    const s = await c.openRecovery("draft.md");
    expect(Array.isArray(s.blocks)).toBe(true);
    expect(s.blocks.length).toBeGreaterThan(0);
    const before = await c.runQuery({ type: "get_note", path: "draft.md" });
    await s.restore(s.blocks[0].id, 0);
    const after = await c.runQuery({ type: "get_note", path: "draft.md" });
    // restore appended the chosen version's text
    if (before.type === "note" && after.type === "note") {
      expect(after.contents.length).toBeGreaterThan(before.contents.length);
    }
    s.close();
  });
});

describe("MockClient C0 versions seam", () => {
  it("seals without a message: generates a summary message and a vault_history row", async () => {
    const client = new MockClient({ "roadmap.md": "# Roadmap\n\nalpha beta" });
    await client.sendCommand({
      type: "write_note",
      path: "roadmap.md",
      contents: "# Roadmap\n\nalpha beta gamma delta",
    });
    const res = await client.sendCommand({ type: "commit", message: null });
    expect(res).toEqual({ type: "committed", commit: "c0001" });
    const hist = await client.runQuery({ type: "vault_history", limit: 1 });
    if (hist.type !== "history") throw new Error("bad response");
    const rev = hist.revisions[0];
    // Byte-identical to the engine's template (counts elide the zero side).
    // Titled by display_title (the `# Roadmap` heading), like the engine.
    expect(rev.message).toBe('Edit "Roadmap" (+2 words)');
    expect(rev.summary).toEqual({
      files_changed: 1,
      words_added: 2,
      words_removed: 0,
    });
    expect(rev.name).toBeNull();
  });

  it("sealing with no changes reports nothing_to_commit and adds no version", async () => {
    const client = new MockClient({ "a.md": "hi" });
    const res = await client.sendCommand({ type: "commit", message: null });
    expect(res).toEqual({ type: "nothing_to_commit" });
    const hist = await client.runQuery({ type: "vault_history", limit: null });
    if (hist.type !== "history") throw new Error("bad response");
    expect(hist.revisions).toHaveLength(0);
  });

  it("multi-note seal rolls up into one version", async () => {
    const client = new MockClient({ "a.md": "x", "b.md": "y" });
    await client.sendCommand({
      type: "write_note",
      path: "a.md",
      contents: "x x",
    });
    await client.sendCommand({
      type: "write_note",
      path: "b.md",
      contents: "y y",
    });
    const res = await client.sendCommand({ type: "commit", message: null });
    if (res.type !== "committed") throw new Error("bad response");
    const hist = await client.runQuery({ type: "vault_history", limit: 1 });
    if (hist.type !== "history") throw new Error("bad response");
    expect(hist.revisions[0].message).toBe('Update 2 notes: "a", "b"');
  });

  it("an explicit message is used verbatim (back-compat)", async () => {
    const client = new MockClient({ "a.md": "x" });
    await client.sendCommand({
      type: "write_note",
      path: "a.md",
      contents: "x y",
    });
    const res = await client.sendCommand({
      type: "commit",
      message: "snapshot",
    });
    expect(res).toEqual({ type: "committed", commit: "c0001" });
    // An explicit message overrides the SUBJECT ONLY — the daemon still
    // computes the summary and attaches the revision to per-note history.
    const note = await client.runQuery({ type: "note_history", path: "a.md" });
    if (note.type !== "history") throw new Error("bad response");
    expect(note.revisions[0]).toMatchObject({
      message: "snapshot",
      summary: { files_changed: 1, words_added: 1, words_removed: 0 },
    });
  });

  it("an explicit message with nothing dirty is still nothing_to_commit", async () => {
    const client = new MockClient({ "a.md": "x" });
    const res = await client.sendCommand({
      type: "commit",
      message: "snapshot",
    });
    expect(res).toEqual({ type: "nothing_to_commit" });
  });

  it("name_version marks the revision named in vault and note history", async () => {
    const client = new MockClient({ "a.md": "x" });
    await client.sendCommand({
      type: "write_note",
      path: "a.md",
      contents: "x y",
    });
    await client.sendCommand({ type: "commit", message: null });
    const res = await client.sendCommand({
      type: "name_version",
      commit: "c0001",
      name: "Draft 1",
    });
    expect(res).toEqual({ type: "done" });
    const vault = await client.runQuery({ type: "vault_history", limit: 1 });
    if (vault.type !== "history") throw new Error("bad response");
    expect(vault.revisions[0].name).toBe("Draft 1");
    const note = await client.runQuery({ type: "note_history", path: "a.md" });
    if (note.type !== "history") throw new Error("bad response");
    expect(note.revisions[0].name).toBe("Draft 1");
  });

  it("history queries hand back snapshots, not the mock's live arrays", async () => {
    const client = new MockClient({ "a.md": "x" });
    await client.sendCommand({
      type: "write_note",
      path: "a.md",
      contents: "x y",
    });
    await client.sendCommand({ type: "commit", message: null });
    const first = await client.runQuery({ type: "note_history", path: "a.md" });
    if (first.type !== "history") throw new Error("bad response");
    first.revisions[0].name = "tampered";
    first.revisions.push(first.revisions[0]);
    const again = await client.runQuery({ type: "note_history", path: "a.md" });
    if (again.type !== "history") throw new Error("bad response");
    expect(again.revisions).toHaveLength(1);
    expect(again.revisions[0].name).toBeNull();
  });
});
