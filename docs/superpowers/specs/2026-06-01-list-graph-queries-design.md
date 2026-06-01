# List & Graph Queries — Design Spec

**Date:** 2026-06-01
**Status:** Approved (design); ready for implementation planning
**Builds on:** the engine (walking skeleton + transport) on `main`.

---

## 1. Goal

The contract can read/search a single note and fetch backlinks, but a UI needs
two more reads on day one: **list all notes** (for a file tree) and **fetch the
link graph** (for a graph view). Both are small additions that flow through the
existing dispatcher and daemon unchanged.

- `ListNotes` → every note's path + a display title.
- `GetGraph` → all note nodes + directed link edges.

No daemon code changes: the new `Query` variants are handled by `dispatch_query`
in `cairn-service`, which the daemon already calls for any query.

---

## 2. Domain (`cairn-domain`)

The only new logic lives here, pure and unit-tested.

- **`NotePath::stem(&self) -> &str`** — the filename without its directory or
  `.md` extension. `graph.rs` currently has this as a private free function
  `stem`; refactor it to call `NotePath::stem` (DRY).
- **`Note::display_title(&self) -> String`** — title precedence:
  1. A `title:` line in the raw frontmatter block (line scan: first line whose
     trimmed start is `title:`; take the remainder, trim, strip one layer of
     surrounding `"`/`'`). No YAML dependency.
  2. Else the first Markdown `# ` heading in the body (line whose trimmed start
     is `# `; take the remainder, trimmed, non-empty).
  3. Else `self.path.stem()`.
- **`Graph::nodes(&self) -> Vec<&NotePath>`** — every note path in the graph
  (the keys of the forward map; sorted, since it is a `BTreeMap`).
- **`Graph::edges(&self) -> Vec<(&NotePath, &NotePath)>`** — directed
  `(from, to)` pairs: for each note and each of its resolved forward links.

---

## 3. Contract (`cairn-contract`) + regenerated TS bindings

All new types keep `#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]`
+ `#[ts(export)]`; enums keep `#[serde(tag = "type", rename_all = "snake_case")]`.

- **`Query`** gains two field-less variants:
  - `ListNotes` → tag `"list_notes"`.
  - `GetGraph` → tag `"get_graph"`.
- **New struct `NoteSummary { path: String, title: String }`**.
- **New struct `GraphEdge { from: String, to: String }`**.
- **`QueryResponse`** gains two variants:
  - `Notes { notes: Vec<NoteSummary> }` → tag `"notes"` (response to `ListNotes`).
  - `Graph { nodes: Vec<String>, edges: Vec<GraphEdge> }` → tag `"graph"`
    (response to `GetGraph`).

Existing `Command`/`Query`/`Event`/`CommandResponse`/`ContractError` variants are
unchanged. Bindings regenerate via `cargo test -p cairn-contract`; new files:
`NoteSummary.ts`, `GraphEdge.ts` (and updated `Query.ts`, `QueryResponse.ts`).

---

## 4. Application (`cairn-app`)

- **`Engine::list_notes(&self) -> Result<Vec<Note>, PortError>`** — returns every
  parsed note (reuses the existing private `load_all_notes`).
- **`Engine::graph(&self) -> Result<Graph, PortError>`** — loads all notes and
  returns `Graph::build(&notes)`.

The app stays free of contract types; `display_title()` and node/edge extraction
are domain methods that `cairn-service` calls when mapping to DTOs.

---

## 5. Dispatcher (`cairn-service`)

`dispatch_query` gains two arms:
- `Query::ListNotes` → `engine.list_notes()?`, map each `Note` to
  `NoteSummary { path: note.path.as_str().into(), title: note.display_title() }`,
  return `QueryResponse::Notes { notes }`.
- `Query::GetGraph` → `engine.graph()?`, build
  `QueryResponse::Graph { nodes: graph.nodes()→strings, edges: graph.edges()→GraphEdge{from,to} }`.

No new error paths (these queries can't 404). **No `cairn-daemon` change** — it
serves any `Query` via `dispatch_query`.

---

## 6. CLI (`cairn-cli`)

Two new subcommands (build a `Query`, dispatch in-process, print):
- **`cairn list`** → `Query::ListNotes` → prints one line per note: `path\ttitle`.
- **`cairn graph`** → `Query::GetGraph` → prints one line per edge: `from -> to`.

(Existing subcommands and behavior unchanged.)

---

## 7. Testing

- **`cairn-domain`:** `display_title` precedence — frontmatter `title:` wins;
  else first `# heading`; else stem; quote-stripping; `NotePath::stem` cases
  (nested dir, no extension); `Graph::nodes`/`edges` on a small linked set.
- **`cairn-contract`:** serde round-trip for `QueryResponse::Notes`/`Graph` (tags
  `"notes"`/`"graph"`); codegen exports `NoteSummary`/`GraphEdge`; TS fidelity.
- **`cairn-service`:** `ListNotes` over a 2-note cairn (one with a frontmatter
  title, one without) → expected `NoteSummary` list; `GetGraph` over `a → b` →
  nodes `{a,b}`, edge `{from:a, to:b}`.
- **`cairn-daemon`:** one HTTP test — `POST /query {"type":"list_notes"}` → 200 +
  `{"type":"notes", ...}` — proving the new query flows through unchanged.
- **`cairn-cli`:** integration test — write two linked notes, `cairn list`
  contains both paths, `cairn graph` contains `a.md -> b.md`.

---

## 8. Out of scope

Tantivy, auth/TLS, real watcher, CRDT, tau, the UI itself. Richer note metadata
(tags, mtime) and graph weights are deferred until the UI asks for them.
