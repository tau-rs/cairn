# cairn-web-ui — Security & Design Audit

## Project overview
`cairn-web-ui` is the web-tech (React 18 + Zustand + CodeMirror 6 + Tailwind)
front end for **Cairn**, a git-backed, Obsidian-class note app. It is written
against a single transport-blind contract (commands / queries / an event stream)
and runs unchanged over an in-process **Tauri** transport (desktop) or a browser
**mock**; the same UI is meant to later target a networked daemon. Core features:
a markdown live-preview editor, folder tree, tabs, search, tags, backlinks, a
force-directed graph, a command palette, and (scaffolded) plugins.

## Findings by severity
| Severity | Count |
|----------|-------|
| Critical | 0 |
| High     | 4 |
| Medium   | 9 |
| Low      | 11 |
| **Total**| **24** |

Breakdown by document:
- **security.md** — 6 findings (1 High, 2 Medium, 3 Low) + verified non-findings
- **design.md** — 14 findings (2 High, 6 Medium, 6 Low) across Code design / DX / UX
- **diagnostics.md** — 5 findings (1 High, 3 Medium, 1 Low) + positives

(Some High/Medium items are cross-referenced between documents, e.g. the missing
error boundary appears as DX2 and DG1; the broken-image/asset issue spans S3/S4
and U5. The counts above tally distinct finding IDs per document.)

## Top 5 issues
1. **Tauri CSP is disabled (`csp: null`)** — `src-tauri/tauri.conf.json:34-36`.
   No in-webview defense-in-depth for an app that opens arbitrary folders and
   loads remote content. *(security.md S1, High)*
2. **Stale-response races in every async store action** — `web/src/store/store.ts:518-527`
   (and runSearch/loadGraph/filterByTag). Slow responses overwrite the
   now-current note/search/graph. *(design.md D1, High)*
3. **Self-induced refresh storm on every autosave** — `web/src/store/store.ts:197-206`.
   The user's own `write_note` echoes back as `note_changed`, triggering 2-5
   queries + a full graph rebuild per save. *(design.md D2, High)*
4. **No error boundary** — `web/src/main.tsx:9-15`. A render throw (editor
   decoration / graph lib) blanks the whole app with no recovery or diagnostic.
   *(design.md DX2 / diagnostics.md DG1, High)*
5. **Remote/`data:` images in notes auto-load + naive local-path join** —
   `web/src/components/editor/imageResolver.ts:6-9`, `web/src/client/tauri.ts:53-58`.
   Privacy beacons on note-open today; latent path-traversal once the asset
   protocol is enabled. *(security.md S2/S3, Medium)*

## Picking up from here

**Worktree & branch.** This audit was performed in a dedicated git worktree:
- Path: `/Users/titouanlebocq/code/cairn-ui-worktrees/audit`
- Branch: `audit/design-security`
- The canonical checkout is `/Users/titouanlebocq/code/cairn-ui` — **do not**
  edit it from this worktree; remediation should happen on a branch off this one
  (or a fresh feature branch) within this worktree to keep the audit isolated.

Only the `audit/` directory was added; **no source code was modified** and
nothing was pushed. The audit commit is the single commit on this branch on top
of the project history.

**Layout reminder for a fresh session.**
- App entry: `web/src/main.tsx` → `web/src/app/App.tsx` (the shell; see D5 — it's
  the monolith to decompose).
- State: `web/src/store/store.ts` (Zustand vanilla store; all backend I/O and the
  refresh logic live here — start here for D1/D2/D3/D7).
- Transport: `web/src/client/` (`types.ts` contract interface, `tauri.ts` real,
  `mock.ts` browser mock, `host.ts` cairn lifecycle).
- Contract types: `web/src/contract/` (vendored from the engine via
  `scripts/sync-contract.sh`; pinned in `contract/source.ts`).
- Editor: `web/src/components/editor/` (CodeMirror live-preview + widgets).
- Tauri shell: `src-tauri/` (`src/lib.rs` commands, `tauri.conf.json`,
  `capabilities/default.json`).

**Suggested remediation order.** S1 (CSP) → DX2/DG1 (error boundary) → D1+D2
(store races + refresh storm, ideally together since they interact) → S2/S3/S4/U5
(image trust + asset protocol) → D3/D4 (cairn-switch state + wire-or-delete the
plugin/notice dead code) → U1/U2/U3 (a11y + loading/error UX) → the DX/Low items.

See `security.md`, `design.md`, and `diagnostics.md` for the full set with
`path:line` evidence, impact, and concrete recommendations. See `devops.md` for
the CI/CD & DevOps audit: the canonical pipeline model applied to this Tauri
repo, current-state gaps with `path:line`, and an ordered implementation
checklist (justfile + lefthook, SHA-pinned actions, a `v*`-tag heavy tier, SBOM).
