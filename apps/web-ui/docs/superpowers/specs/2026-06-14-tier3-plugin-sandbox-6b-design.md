# Tier-3 sandboxed-iframe plugins — Phase 6b design

Date: 2026-06-14. Track 05, **Phase 6b** — the protocol/bundle upgrade to the
Phase 6a sandboxed-iframe design (`2026-06-14-tier3-plugin-sandbox-design.md`,
referred to below as *the 6a spec*). This document is **design only** — no
production code lands from this doc; implementation is a sequenced Wave that
**depends on 6a being merged first**.

## 0. Dependency & framing (read first)

6b is **purely additive to 6a**. It does **not** touch the broker API, the
permission/grants model, the lifecycle state machine, the rate cap, or the error
isolation — all of those are inherited unchanged from 6a. 6b changes only:

1. **How plugin UI is delivered** — from an inline `srcdoc` HTML string
   (null-origin, no storage) to a multi-file bundle served at a **distinct
   per-plugin opaque origin** via a custom Tauri protocol (origin-native storage,
   per-frame CSP).
2. **Where a roomy plugin can mount** — a new `panel.main` slot rendering a
   **resizable, collapsible bottom dock** in the main editor column, for augment
   tools that want more room than the ~280px sidebar.

**Hard prerequisite:** 6a is **not yet implemented** anywhere in this repo (only
6a's spec+plan docs exist, PR #60). 6b's contract deltas stack on 6a's, and 6b's
host code extends 6a's `IframeHost`/broker. **This plan cannot start until 6a is
merged.** Everything below assumes 6a's surface exists.

## 1. Problem, goals, non-goals

### Problem
6a delivers a plugin as one inline HTML string in a `srcdoc` iframe at a **null
origin**. That is deliberately the most-locked-down sandbox, but it has two hard
ceilings: (a) a null-origin frame has **no `localStorage`/IndexedDB at all**, so a
plugin cannot persist anything across reloads; (b) one inline string cannot carry
a real multi-file bundle (separate JS/CSS/asset files, a small library). And 6a's
only placement is the narrow `sidebar.section` rail — too cramped for a "rich
editor" augment tool (a readability dashboard, an AI-rewrite chat, a linter list).

### Goals
- Serve each plugin's on-disk `ui/` bundle (multi-file) at a **distinct opaque
  origin** `plugin-sandbox://<id>/…` via a Rust custom-protocol handler.
- Give each plugin **origin-native persistent storage** (`localStorage`/
  IndexedDB), automatically partitioned per-plugin by its origin.
- Enforce a **strict per-frame CSP** and **strict path-canonicalisation** in the
  Rust handler (the bundle can never serve files outside its own `ui/` dir).
- Add a `panel.main` slot rendering a **resizable, collapsible bottom dock** for
  roomy augment tools.
- Keep **everything else identical to 6a**: broker methods, capabilities,
  consent/grants, lifecycle, rate cap, error boundary.

### Non-goals (this wave)
- No change to the broker API, permission model, capabilities, lifecycle, rate
  cap, or error isolation (all inherited from 6a).
- No plugin network access — `connect-src 'none'` in the per-frame CSP; the
  protocol is local-file-only.
- No editor-tab "replace the note" plugin surface (the Obsidian/VSCode
  `WorkspaceLeaf`/`WebviewPanel` model). That is a different feature for
  *replace*-style editors; documented here as future, not built.
- No production code: spec + plan only.

## 2. Locked decisions

| # | Decision | Choice | Why |
|---|----------|--------|-----|
| Q1 | Plugin storage | **Origin-native** — serve at `plugin-sandbox://<id>/` + `sandbox="allow-scripts allow-same-origin"`; frame is same-origin *to itself*, gets `localStorage`/IndexedDB, partitioned per-plugin, still cross-origin to the host app | Zero new broker methods (keeps "broker unchanged"); standard web storage; matches the spec's "distinct opaque origin (separate storage partition)". Safe **only because** the bundle is on a *different* origin than the host — asserted in code |
| Q2 | Widget delivery field | **Replace** `html` with `entry: string` (relative path into the bundle) | One mount path / one origin model / one audit surface; matches 6a-spec §4 wording; no migration cost (6a unshipped) |
| Q3 | `id → ui/ dir` source | **Engine-reported + host-registered** — engine adds `uiRoot` (abs dir) to `PluginSummary`; host pushes `{id → uiRoot}` to Rust via `set_plugin_ui_roots`; handler serves only registered dirs | Engine is the single source of truth (plugins may live per-cairn *or* global); Rust gets an explicit allow-list, same posture as the existing `asset_protocol_scope` |
| Q4 | `panel.main` placement | **Resizable, collapsible bottom dock** in the main column; backlinks rail untouched | cairn's broker is `activeNote.read/write` → these *augment* the visible note (UX consensus = docked, not modal/tab); "panel.**main**" = within the main column; no turf war with the existing right rail; responsive-friendly; matches the "dock" wording and the VSCode "Panel" idiom |
| Q5 | Per-frame `script-src` | **Strict `'self'`** (no `'unsafe-inline'`) — plugin JS must be bundle files, not inline `<script>` | Mirrors the host app's own `script-src 'self'`; a small authoring constraint for a real XSS-surface reduction |

Prior art / UX backing for Q4: Obsidian & VSCode put *replace*-style rich plugins
in an editor tab; Logseq/Roam and VSCode `WebviewView` use docked panels for
*augment* tools; NN/g flags route/modal takeover as a poor default and "don't mix
a tool into the document tab strip"; Cloudscape/Material favour docked panels for
reference-while-working. cairn's active-note broker ⇒ augment ⇒ dock.

## 3. Deliverable surface

### 3.1 Engine contract (other repo `tau-rs/cairn`; re-synced via the sync script — never hand-edit vendored contract)

```ts
// PluginWidget.ts — the iframe variant: html → entry
| { kind: "iframe"; entry: string; height: number | null }   // was: html: string

// PluginSummary.ts — +1 field
uiRoot: string | null;        // absolute on-disk dir of the plugin's ui/ bundle; null = no ui

// PluginSlot.ts — +1 value
"sidebar.section" | "topbar.action" | "command" | "panel.main"
```

`PluginCapability` and the broker wire protocol are **unchanged from 6a**.

### 3.2 Host (this repo, the implementable Wave work)

**Rust (`src-tauri/src/`):**
- `plugin_protocol.rs` (new) — registers the `plugin-sandbox` URI scheme; the
  request handler: id→root lookup, strict path-canonicalisation, MIME, per-frame
  CSP header, regular-file-only.
- `plugin_roots.rs` (new) — Tauri-managed state holding the canonicalised
  `{id → PathBuf}` allow-list + the `set_plugin_ui_roots` command.
- `lib.rs` — **modify**: register the scheme in `tauri::Builder`, `manage` the
  roots state, add `set_plugin_ui_roots` to the invoke handler.

**TS:**
- `web/src/components/plugins/IframeHost.tsx` — **modify** (6a file): switch
  `srcdoc`/null-origin mount to `src="plugin-sandbox://<id>/<entry>"` +
  `sandbox="allow-scripts allow-same-origin"`. Broker/lifecycle untouched.
- `web/src/components/plugins/PanelDock.tsx` (new) — the bottom dock host:
  reads `panel.main` contributions, tab strip if >1, active-selection + collapsed
  state (persisted), mounts `IframeHost` for the active one, resizable height.
- `web/src/components/shells/*` / `Shell.tsx` — **modify**: render `<PanelDock/>`
  in the main column under the editor (desktop; tablet/mobile reuse).
- `web/src/store/*` — **modify**: after `loadPlugins`, `invoke("set_plugin_ui_roots", …)`;
  a small `panelDock` UI slice (active contribution id, collapsed, height).
- `web/src/client/pluginContributions.ts` — **modify**: accept `entry` (clamp
  length, reject absolute/`..`-leading paths as a cheap pre-filter), accept the
  `panel.main` slot, ingest `uiRoot`.
- `web/src/client/mock.ts` — **modify**: seed a multi-file demo plugin bundle for
  manual + e2e use (e.g. a readability panel using `localStorage`).

## 4. Protocol & origin model

### 4.1 The frame

```html
<iframe
  sandbox="allow-scripts allow-same-origin"   <!-- same-origin = to ITSELF -->
  src="plugin-sandbox://<id>/<entry>"          <!-- e.g. .../index.html -->
  ...>
```

- The frame's origin is `plugin-sandbox://<id>` — **distinct per plugin** (the id
  is the origin host), **distinct from the host app** (`tauri://localhost`).
- `allow-same-origin` makes the frame same-origin **to itself** → real
  `localStorage`/IndexedDB, partitioned by origin (i.e. per plugin). It is **not**
  same-origin to the host, so it still cannot read host DOM/storage/cookies.
- **Safety invariant:** `allow-scripts + allow-same-origin` is safe here *only
  because* the content is on a different origin than the parent. `IframeHost`
  must assert the constructed `src` origin ≠ the host origin, and the `sandbox`
  attr is a hard-coded literal, never templated from plugin data.
- `connect-src 'none'` (§4.3) ⇒ no network; the frame is compute + brokered-data
  + own-storage only.

### 4.2 Path-canonicalisation (Rust handler — the security core)

This is the Trail-of-Bits VS Code failure mode the 6a spec flags. For each
request to `plugin-sandbox://<id>/<rel>`:

1. `root = roots.get(id)` (already canonicalised at registration); unknown id → **404**.
2. Percent-decode `<rel>` once; reject if it contains a NUL byte → **400**.
3. `candidate = root.join(decoded_rel)`.
4. `canonical = std::fs::canonicalize(candidate)` (resolves `..` **and**
   symlinks); failure (missing) → **404**.
5. **Assert `canonical.starts_with(root)`** — else **403**. This single check is
   the wall: it catches `..` traversal *and* symlinks that point outside the
   bundle, because canonicalisation resolves both before the prefix test.
6. Must be a **regular file** (no directory listings) → else **404**.
7. Resolve `Content-Type` by extension (html/js/css/svg/png/jpg/json/woff2/wasm…);
   unknown → `application/octet-stream`.
8. Respond `200` with the bytes + the per-frame CSP header (§4.3).

`root` is canonicalised **once at registration** (`set_plugin_ui_roots`), not per
request. Windows note: `canonicalize` yields `\\?\` verbatim paths; the
`starts_with` test holds because both sides are canonicalised.

### 4.3 Per-frame CSP (handler sets on every response)

```
default-src 'none';
script-src 'self';
style-src 'self' 'unsafe-inline';
img-src 'self' data: blob:;
font-src 'self';
connect-src 'none';
base-uri 'none';
form-action 'none';
frame-ancestors tauri://localhost
```

`'self'` = `plugin-sandbox://<id>` (the bundle's own files). `connect-src 'none'`
kills all network. `script-src 'self'` forbids inline `<script>` (Q5) — JS ships
as bundle files. `frame-ancestors` lets only the host app embed the frame. This
is **stricter than and independent of** the app document's CSP — the per-frame
lockdown is the whole reason 6b moves off `srcdoc`. The app CSP is **not** loosened.

## 5. `uiRoot` registry & lifecycle

```
loadPlugins() ─▶ PluginSummary[]
   └─ build { id → uiRoot } for every plugin with uiRoot != null
        └─ invoke("set_plugin_ui_roots", { roots })
              └─ Rust: REPLACE the whole map; canonicalize each root;
                 drop entries that don't exist / fail to canonicalize.
```

- **Replace-whole-map** semantics: unloaded/uninstalled plugins immediately stop
  being serveable — no stale dirs linger (matters for uninstall/revoke).
- The map **is the serve allow-list**: the handler serves *only* registered,
  canonicalised dirs — same defensive posture as `asset_protocol_scope`.
- Registration covers all loaded plugins with a `uiRoot`; **mounting still
  requires consent** (6a lifecycle: grant → mount), so an un-granted plugin's
  files are never actually requested. Registration ≠ exposure.
- On cairn switch, `loadPlugins` re-runs and the map is replaced.

## 6. `panel.main` bottom dock

- `PanelDock` reads all `panel.main` contributions. If none, it renders nothing.
- A plugin's `panel.main` contribution is **opened** via the command palette
  and/or a dock affordance (reuse existing command registration); the dock is not
  always-on.
- The dock shows **one active contribution at a time**; if >1 exists, a small
  VSCode-Panel-style tab strip switches between them.
- The dock is **resizable** (drag the top edge) and **collapsible**; active id +
  collapsed + height persist in a small `panelDock` UI slice (localStorage,
  mirroring existing UI persistence).
- The active contribution mounts a 6a `IframeHost` (same broker, same consent,
  same lifecycle) — `PanelDock` is just a second **mount point**, not a second
  iframe/broker implementation.
- `sidebar.section` (6a) is unchanged; `height` clamp still applies to sidebar
  widgets. In the dock, height is governed by the dock, not the widget field.

## 7. Testing (Wave work)

- **Rust path-canonicalisation** (`plugin_protocol` tests): serve a real file ok;
  `..` traversal → 403; symlink-escape → 403; unknown id → 404; missing file →
  404; NUL byte → 400; directory → 404; correct MIME by extension; CSP header
  present on every response. Cross-platform-safe temp dirs.
- **`set_plugin_ui_roots`**: replace-whole-map drops stale ids; non-existent root
  dropped; canonicalisation applied; only registered ids serve.
- **`IframeHost` (6b delta)**: `src` is `plugin-sandbox://<id>/<entry>`; `sandbox`
  is exactly `"allow-scripts allow-same-origin"` (unit-assert the literal);
  asserts frame origin ≠ host origin. Broker/lifecycle tests inherited from 6a.
- **`PanelDock`**: empty → renders nothing; single contribution mounts;
  multiple → tab strip switches; collapse/resize persist & restore; revoke/unload
  unmounts.
- **Sanitiser** (`pluginContributions.test.ts`): `entry` accepted; absolute or
  `..`-leading `entry` rejected; `panel.main` slot accepted; `uiRoot` ingested;
  oversized `entry` clamped.
- **e2e**: demo bundle (readability panel) → consent → Allow → opens in bottom
  dock → reads active note → writes a value to `localStorage` → reload → value
  persists → resize/collapse persist → revoke in Settings → dock entry gone.

Run the FULL local gate before claiming green (`prettier --check`/format is easy
to miss; eslint won't catch it).

## 8. Risks & mitigations

| Risk | Mitigation |
|------|-----------|
| Path traversal / symlink escape out of the bundle | Canonicalise both sides + `starts_with` prefix test (§4.2); regular-file-only; unit-tested with `..` and symlink cases |
| Accidental `allow-same-origin` coinciding with the host origin (sandbox escape) | Hard-coded `sandbox` literal; `IframeHost` asserts frame origin ≠ host origin; never templated from plugin data |
| Stale/uninstalled plugin dir stays serveable | `set_plugin_ui_roots` replaces the whole map; handler serves only the current allow-list |
| Plugin reaches the network | `connect-src 'none'` per-frame CSP; protocol is local-file-only |
| Cross-platform scheme rewrite (Windows `http://plugin-sandbox.<id>.localhost`) | Per-plugin origins stay distinct (storage partition holds); broker authenticates by `event.source` identity, not origin string; documented + tested per-platform |
| Inline-script XSS inside the bundle | `script-src 'self'` forbids inline `<script>` (Q5) |
| Bottom dock steals editor height | Resizable + collapsible; editor stays primary above it |
| Contract drift (engine vs vendored) | Re-sync via sync script; contract-drift CI; never hand-edit vendored contract |
| 6a not yet merged | Plan is explicitly sequenced after 6a; 6b extends 6a's `IframeHost`/broker and cannot start until 6a lands |

## 9. Scope

- **In (6b Wave):** custom `plugin-sandbox://` Rust protocol + strict
  canonicalisation + per-frame CSP; origin-native storage; `entry` replaces
  `html`; `uiRoot` registry + `set_plugin_ui_roots`; `panel.main` bottom dock;
  tests; demo multi-file bundle.
- **Out (future):** editor-tab "replace the note" plugin surface (Obsidian/VSCode
  `WorkspaceLeaf`/`WebviewPanel` model) for *replace*-style rich editors; per-plugin
  storage quotas/inspection; plugin network allow-listing.

## 10. Open contract dependency

6b needs `tau-rs/cairn` to: change the iframe `PluginWidget` variant from `html`
to `entry`, add `PluginSummary.uiRoot`, and add the `panel.main` `PluginSlot`
value — then a vendored-contract re-sync here. This is one cross-repo dependency,
**on top of** 6a's (which adds the `iframe` variant, `capabilities`, and the
`PluginCapability` enum). The host work can be built and tested against
`MockClient` + a hand-seeded `PluginSummary`/bundle ahead of the engine landing,
but **6a must merge before 6b implementation begins**.
