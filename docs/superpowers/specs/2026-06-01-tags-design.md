# Frontmatter Tags — Design Spec

**Date:** 2026-06-01
**Status:** Approved (design); ready for implementation planning
**Builds on:** the engine on `main` (notes/search/links/graph/list/watcher).

---

## 1. Goal

Cairn reads **tags from each note's frontmatter `tags:` key** and exposes them so a
UI can show tag pills, a tag pane, and filter-by-tag. Tags are frontmatter-only —
no inline `#tag` parsing.

New surface:
- `Query::ListTags` → all tags with counts (the tag pane).
- `Query::NotesByTag { tag }` → the notes carrying a tag (filter).
- Each note's tags are added to `NoteSummary` (so `list_notes` shows them for free).

---

## 2. Domain — `Note::tags`

`Note::tags(&self) -> Vec<String>` — a dep-free hand-rolled parser over the raw
frontmatter block (consistent with `Note::display_title`'s `title:` line-scan). No
YAML crate.

Find the first frontmatter line whose trimmed start is `tags:`. Take the remainder
`rest` (trimmed) and parse by form:
- **Inline array** — `rest` starts with `[`: strip the surrounding `[`…`]`, split on
  `,`.
- **Block list** — `rest` is empty: consume the following lines whose trimmed start
  is `- `, taking the text after `- `, until a non-list line.
- **Scalar** — `rest` is non-empty and not `[`: split on commas and whitespace
  (`tags: a, b` and `tags: a b` both → `[a, b]`).

For every produced token: trim, strip one layer of surrounding `"`/`'`, drop empties.
**Dedup preserving first-seen order.** Only the literal key `tags:` is matched (so
`tagsfoo:` does not match); singular `tag:` is not matched. Nested tags like
`notes/ideas` are kept verbatim (one tag).

Examples:
```
tags: [rust, "ideas"]        -> [rust, ideas]
tags:\n  - rust\n  - ideas    -> [rust, ideas]
tags: rust, ideas            -> [rust, ideas]
tags: rust ideas             -> [rust, ideas]
tags: notes/ideas            -> [notes/ideas]
(no frontmatter / no tags:)  -> []
```

---

## 3. Application — `cairn-app`

Computed on-demand from all notes (like `graph`/`backlinks`; no tag index/memo yet):
- `Engine::list_tags(&self) -> Result<Vec<(String, usize)>, PortError>` — count tag
  occurrences across notes (each note contributes its deduped tags once), returned
  **sorted by tag** (use a `BTreeMap<String, usize>`).
- `Engine::notes_by_tag(&self, tag: &str) -> Result<Vec<NotePath>, PortError>` —
  notes whose `tags()` contains `tag`, **sorted by path**.

---

## 4. Contract — `cairn-contract` (+ regenerated TS)

- New struct `TagCount { tag: String, count: u32 }` (derives + `#[ts(export)]`).
- `Query` gains `ListTags` (field-less) and `NotesByTag { tag: String }`.
- `QueryResponse` gains `Tags { tags: Vec<TagCount> }` (response to `ListTags`).
  `NotesByTag` reuses the existing `Paths { paths }`.
- `NoteSummary` gains a field: `tags: Vec<String>` (so `list_notes`'
  `QueryResponse::Notes` carries each note's tags). This changes the existing
  `NoteSummary`/`Notes` shape and the generated `NoteSummary.ts`.

Tags (snake_case): `list_tags`, `notes_by_tag`, `tags`.

---

## 5. Dispatcher — `cairn-service`

`dispatch_query` gains:
- `Query::ListTags` → `engine.list_tags()?` → `QueryResponse::Tags { tags: TagCount[] }`.
- `Query::NotesByTag { tag }` → `engine.notes_by_tag(&tag)?` → `QueryResponse::Paths { paths }`.
- `Query::ListNotes` arm updated: each `NoteSummary` now also sets
  `tags: note.tags()`.

No new error paths. **No `cairn-daemon` change** — it serves any `Query` via
`dispatch_query`.

---

## 6. CLI — `cairn-cli`

Two new subcommands (build a `Query`, dispatch in-process, print):
- `cairn tags` → `Query::ListTags` → one line per tag: `tag\tcount`.
- `cairn tagged <tag>` → `Query::NotesByTag { tag }` → one note path per line.

(The existing `list` subcommand's output is unchanged in shape — it prints
`path\ttitle`; the added `tags` field rides along in the contract response but the
CLI's `list` need not print it.)

---

## 7. Testing

- **domain:** `Note::tags` for each form (inline array, block list, comma scalar,
  space scalar, single, nested), quote-stripping, dedup/order, no-frontmatter and
  no-`tags:` → empty, and that `tagsfoo:` is not matched.
- **app:** `list_tags` counts across notes (sorted, deduped per note);
  `notes_by_tag` returns the right sorted paths; a tag on no note → empty.
- **contract:** serde round-trip for `QueryResponse::Tags` (tag `tags`), `Query::ListTags`/`NotesByTag`; `NoteSummary` now serializes a `tags` array; codegen exports `TagCount` and the updated `NoteSummary`; TS fidelity.
- **service:** `ListTags`/`NotesByTag` dispatch over a small cairn (notes with
  frontmatter tags); `ListNotes` response now includes `tags`.
- **daemon:** one HTTP test — `POST /query {"type":"list_tags"}` → 200 + `{"type":"tags",...}` (proves flow-through).
- **cli:** integration — write notes with frontmatter tags, `cairn tags` shows them
  with counts, `cairn tagged <tag>` lists the right notes.

---

## 8. Out of scope

Inline `#tag` parsing, a persisted tag index/memo (computed on-demand for now),
tag rename/merge, tag hierarchy queries (nested tags are opaque strings), and typed
frontmatter parsing (the hand-rolled `tags:` parser covers the common forms; a YAML
crate can replace it later if typed frontmatter is needed).
