# Note Rename / Move (Link-Aware) — Design Spec

**Date:** 2026-06-01
**Status:** Approved (design); ready for implementation planning
**Builds on:** the engine on `main`.

---

## 1. Goal

An atomic, link-aware **rename / move**: `Command::RenameNote { from, to }` moves a
note (rename in place, or move to another directory — same command) and rewrites
`[[wikilinks]]` in other notes that pointed at it, so links stay intact. Done the
hexagonal way: a new `VaultStore` port capability, an app use-case over ports, and
pure-domain link rewriting.

---

## 2. Hexagonal layering (architecture)

| Layer | Adds | Depends on |
|---|---|---|
| domain | `rewrite_link_target` (pure string fn) | nothing |
| ports | `VaultStore::rename` + `PortError::AlreadyExists` | domain |
| infra | `LocalFsStore::rename` | ports, std::fs |
| app | `Engine::rename_note` (use-case) | ports, domain |
| contract | `Command::RenameNote` | — |
| service | `dispatch_command` arm + error mapping | app, contract |
| cli | `cairn rename <from> <to>` | service, contract |

No layer reaches around another: the capability is a port method, adapters implement
it, the use-case orchestrates ports, link rewriting is pure domain.

---

## 3. Ports (`cairn-ports`)

- Add to `VaultStore`:
  ```rust
  /// Move a note from `from` to `to`.
  ///
  /// # Errors
  /// `PortError::NotFound` if `from` is missing; `PortError::AlreadyExists` if
  /// `to` already exists; `PortError::Adapter` on other failures.
  fn rename(&mut self, from: &NotePath, to: &NotePath) -> Result<(), PortError>;
  ```
- Add a `PortError` variant:
  ```rust
  /// The target of a create/rename already exists.
  #[error("already exists: {0}")]
  AlreadyExists(String),
  ```

---

## 4. Infra (`cairn-infra`) — `LocalFsStore::rename`

- If `from` doesn't exist → `PortError::NotFound(from)`.
- If `to` already exists → `PortError::AlreadyExists(to)` (no clobber).
- Else create `to`'s parent directory (so moves into a subfolder work), then
  `std::fs::rename(full(from), full(to))` (atomic on the same filesystem), mapping IO
  errors to `PortError::Adapter`.

---

## 5. Domain (`cairn-domain`) — `rewrite_link_target`

```rust
/// Rewrite `[[from]]` -> `[[to]]` and `[[from|alias]]` -> `[[to|alias]]` in
/// `content`, matching link targets by exact (trimmed) text. Non-matching links
/// and all other text are left unchanged. Operates on raw content (no frontmatter
/// parsing), mirroring `extract_links`' scanner.
#[must_use]
pub fn rewrite_link_target(content: &str, from: &str, to: &str) -> String;
```
Scan `[[ … ]]` spans; for each, split the inner text on the first `|` into
`target`/`alias`; if `target.trim() == from`, emit `[[to]]` or `[[to|alias]]`
(preserving the original alias text); otherwise keep the original span verbatim.

---

## 6. Application (`cairn-app`) — `Engine::rename_note`

```rust
/// Rename/move a note, link-aware: moves the file, then rewrites links pointing
/// at the old stem in every note. Emits NoteDeleted(from) + NoteChanged(to) +
/// a NoteChanged per rewritten note (+ Reindexed), all via `apply_change`.
///
/// # Errors
/// Propagates `VaultStore::rename` errors (NotFound / AlreadyExists / Adapter).
pub fn rename_note(&mut self, from: &NotePath, to: &NotePath, sink: &mut dyn EventSink)
    -> Result<(), PortError>;
```
Steps:
1. `self.store.rename(from, to)?` — move the file (or fail without side effects).
2. `self.apply_change(&FsChange::Removed(from.clone()), sink)?` then
   `self.apply_change(&FsChange::Changed(to.clone()), sink)?` — index/memo + move events.
3. If `from.stem() != to.stem()` (a pure directory move keeps the stem → skip): for
   each `path` in `self.store.list()?`, read the raw content,
   `rewrite_link_target(&raw, old_stem, new_stem)`; if it changed, `store.write` the
   note and `apply_change(&FsChange::Changed(path), sink)`. (The loop includes `to`,
   so a self-link in the moved note is fixed too; unchanged notes are skipped.)

---

## 7. Contract (`cairn-contract`)

- `Command` gains:
  ```rust
  /// Rename or move a note (link-aware).
  RenameNote {
      /// Current relative path.
      from: String,
      /// New relative path (may be in a different directory).
      to: String,
  },
  ```
- Response: `CommandResponse::Done` (reused). No new event variant — the move and
  rewrites flow as `NoteDeleted`/`NoteChanged`/`Reindexed` events. Regenerate the
  `Command.ts` binding.

---

## 8. Dispatcher (`cairn-service`)

- `dispatch_command` `Command::RenameNote { from, to }`: validate both paths
  (`NotePath::new` → `InvalidRequest` on failure), `engine.rename_note(&from, &to, sink)`,
  return `CommandResponse::Done`.
- Extend `From<PortError> for ServiceError`:
  `PortError::AlreadyExists(s) => ServiceError::InvalidRequest(s)` (so a clobber →
  400, not 500). `NotFound` (missing source) stays → 404.

---

## 9. CLI (`cairn-cli`)

- `cairn rename <from> <to>` → `Command::RenameNote { from, to }` → prints
  `renamed <from> -> <to>`.

---

## 10. Testing

- **domain:** `rewrite_link_target` — `[[a]]`→`[[b]]`, `[[a|alias]]`→`[[b|alias]]`,
  non-matching `[[c]]` untouched, no change when the target is absent, multiple
  occurrences.
- **infra:** `LocalFsStore::rename` — moves a note; moves into a new subdirectory
  (parent created); `to` exists → `AlreadyExists`; `from` missing → `NotFound`; the
  content is preserved.
- **app:** `rename_note` — file moved (old gone, new present with same content);
  emits `NoteDeleted(from)` + `NoteChanged(to)`; a note linking `[[from-stem]]` is
  rewritten to `[[to-stem]]` and emits `NoteChanged`; a pure directory move (same
  stem) does NOT rewrite links; rename onto an existing note → `AlreadyExists`.
- **contract:** `Command::RenameNote` serde round-trip (tag `rename_note`) + TS
  binding includes it.
- **service:** dispatch `RenameNote` success; target-exists → `InvalidRequest`;
  missing source → `NotFound`; invalid path → `InvalidRequest`.
- **cli:** integration — write `a.md` and a `b.md` containing `[[a]]`, `cairn rename
  a.md c.md`, then assert `a.md` is gone, `c.md` exists, and `b.md` now contains
  `[[c]]`.
- **daemon:** one HTTP flow-through — `POST /command {"type":"rename_note", ...}` → 200
  + `{"type":"done"}`.

---

## 11. Out of scope

A dedicated `NoteRenamed` event (reuse delete+changed); rewriting links by *path*
(only stem-based `[[wikilinks]]` are rewritten — Markdown `[](path.md)` links are
not); renaming with stem collisions (two notes sharing a stem — links rewrite
ambiguously, same limitation the graph already documents); folder rename (move whole
directories) — only single-note rename/move.
