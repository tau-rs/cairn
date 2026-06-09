# ADR-0007: In-memory note cache

**Status:** Accepted
**Date:** 2026-06-02

## Context

`list_notes`/`graph`/`get_backlinks`/`list_tags`/`notes_by_tag` each called
`load_all_notes`, re-reading and re-parsing the whole vault on every call — in the
daemon, on every UI refresh.

## Decision

Add a lazy `RefCell<Option<HashMap<NotePath, Note>>>` cache on `Engine`, populated on
first metadata query and kept live by the single-note apply paths (`apply_write` /
`apply_change` / `apply_removal`). No bulk invalidation — `reindex` / `reconcile`
rebuild the index but never change note files, so a populated cache stays valid across
them. `RefCell` keeps the change inside `cairn-app` (the query methods stay `&self` and
`dispatch_query` is untouched); the daemon's `Mutex` serializes all access, so the
`RefCell` is never borrowed concurrently. `Graph::build` now takes an iterator of
`&Note` so `graph` / `backlinks` build from `cache.values()` without cloning.

## Consequences

- Metadata queries are O(in-memory) after the first; the cache holds all parsed notes
  in RAM (~vault text size, like Obsidian); the watcher keeps it current across
  external edits.
- Out of scope: caching the built `Graph` (backlinks still constructs it per call),
  persisting the cache, eviction/size limits.
