# Tier-3 sandboxed-iframe plugins — design

Date: 2026-06-14. Track 05 (see `.context/handoff/05-tier3-plugins.md`). Roadmap
**Phase 6**, the deepest tier of the 3-tier UI-plugin extensibility model.
This document is **design only** — no production code lands this wave.

## Locked decisions

| # | Decision | Choice | Why |
|---|----------|--------|-----|
| Q1 | Content delivery & origin | **Hybrid:** `srcdoc` + **null origin** now (6a); custom-protocol bundles reserved (6b) | Canonical untrusted-widget sandbox; aligns with existing strict CSP; one-variant contract delta; shippable in one wave |
| Q2 | Broker API breadth | **Read + write** (content-editor tier), permission model folded in | Headline use case is content-editing plugins (linter/templater/rewriter), not just observers |
| Q3 | Permission granularity | **All-or-nothing grant + Chrome-style grouped plain-language prompt**; version-gated re-prompt; revoke in Settings | Only consent models with production mileage (Figma all-or-nothing + Chrome warning-grouping); avoids per-toggle test explosion |
| Q4 | Placement | Reuse **`sidebar.section`** (one `WidgetView` branch); `panel.main` dock reserved (6b) | Honors handoff "reuse the Tier-2 slot model"; no new slot, no layout surgery |
| Q5 | Runtime policing | Minimal isolation **+ per-frame inbound message rate cap** | Rate cap closes a real host-freeze vector cheaply; a CPU watchdog fights an unwinnable battle (can't preempt iframe JS) |

## 0. Where this sits (verification against current main)

The 3-tier extensibility model ([[ui-plugin-extensibility]]):

- **Tier 1 — declarative themes.** DONE ([[themes]]).
- **Tier 2 — declarative slot-mount contributions.** DONE (PR #55,
  [[tier2-slot-mount]]). The engine (full-trust subprocess) emits
  `PluginContribution` JSON; the host renders **host-owned React** widgets
  (`text` / `action` / `list`) into slots. **No iframes, no `postMessage`, no
  plugin-supplied markup.** `pluginContributions.ts` sanitises (drop-unknown +
  clamp, never throws); `SlotRenderer` mounts each entry in an epoch-keyed
  `ErrorBoundary` with a quiet `WidgetError` fallback.
- **Tier 3 — this design.** The first tier where **untrusted plugin JS
  executes** in the user's app. NOT started.

**The trust inversion is the whole point.** Tiers 1–2 run full-trust plugin code
that emits only *data*. Tier 3 runs *untrusted code* in a sandbox and lets it
*read and write the user's notes* through a brokered, capability-gated API.
The two tiers have fundamentally different threat models and the design must say
so explicitly.

Verified against current main: `PluginWidget` (`web/src/contract/PluginWidget.ts`)
is a 3-variant union; `PluginSummary` carries `commands` + `contributions` but
**no `capabilities` field**; `PluginSlot` is `"sidebar.section" | "topbar.action"
| "command"`. The Tier-2 sanitizer test already asserts an unknown
`widget.kind: "iframe"` is **dropped** — reserved intent for this tier. The
Tauri CSP (`src-tauri/tauri.conf.json`) is strict: `script-src 'self'`,
`frame-ancestors 'none'`, `object-src 'none'`, no remote images
([[content-trust-hardening]]).

## 1. Problem, goals, non-goals

### Problem
Tier-2 plugins can only contribute host-rendered text/action/list widgets. They
cannot draw custom pixels or run their own logic. Roadmap Phase 6 calls for a
tier where a plugin author can ship arbitrary HTML/JS that augments the editor —
a Markdown linter, a template inserter, a live word-count/readability panel, an
AI-rewrite helper — without the host having to anticipate each widget.

### Goals
- Run **untrusted** plugin JS/HTML safely in a sandboxed iframe.
- Give it a **small, brokered, capability-gated** host API: read & write the
  active note, read/search the vault, run the plugin's own commands, show a toast.
- A **user-facing permission model**: declared, grouped, plain-language consent;
  revocable; re-prompted on expansion.
- **Error isolation**: a crashing/flooding/hanging plugin never takes down the host.
- Reuse the Tier-2 slot model, sanitiser posture, error-boundary, and
  `invokePlugin`/`notice`/`setBuffer` plumbing wherever possible.

### Non-goals (this wave, 6a)
- No custom Tauri protocol, no multi-file plugin bundles, no per-plugin
  persistent storage (→ 6b).
- No `panel.main` dock / dedicated large surface (→ 6b).
- No plugin network access (the sandbox + CSP forbid it; not revisited here).
- No per-capability toggles (deliberately rejected — see Q3).
- No production code: this is spec + plan only.

## 2. North-star

A plugin author writes one HTML file. The engine ships it as an `iframe` widget
contribution with a declared capability set. On first load the user sees a
grouped, plain-language consent prompt; on Allow, the widget mounts in the
sidebar and talks to the host only through the broker. A buggy or hostile plugin
can, at worst, draw wrong pixels inside its box and spin its own CPU — it cannot
escape the frame, reach the network, read un-granted data, or freeze the host.

## 3. Deliverable surface

**Engine contract (other repo `tau-rs/cairn`; re-synced here via the sync script —
do NOT hand-edit vendored contract, [[contract-sync-raw-format]]):**

```ts
// PluginWidget.ts — ONE new variant appended to the existing union
export type PluginWidget =
  | { kind: "text";   text: string; muted: boolean | null }
  | { kind: "action"; label: string; icon: PluginIcon | null; command: string; args: JsonValue | null }
  | { kind: "list";   items: Array<PluginListItem> }
  | { kind: "iframe"; html: string; height: number | null };   // NEW (6a)

// PluginSummary.ts — ONE new field
export type PluginSummary = {
  id: string; name: string; version: string;
  commands: Array<PluginCommandSummary>;
  contributions: Array<PluginContribution>;
  capabilities: Array<PluginCapability> | null;                 // NEW (6a)
};

// PluginCapability.ts — NEW enum
export type PluginCapability =
  | "activeNote.read" | "activeNote.write" | "notes.read" | "notes.search" | "command.invoke";
```

No new `PluginSlot` value. Everything below is **host-side**.

**Host (this repo, the implementable Wave-2 work):**

- `web/src/client/pluginBroker.ts` — the broker: message validation, capability
  map, param clamps, request/response/event protocol, timeouts, rate cap.
- `web/src/store/pluginGrantsSlice.ts` — grants slice (per the 00-SHARED
  single-slice-file rule; wired into `store.ts` with one import + one spread).
- `web/src/components/plugins/IframeHost.tsx` — mounts the sandboxed iframe,
  drives the lifecycle state machine, owns the broker instance for the frame.
- `web/src/components/plugins/PermissionPrompt.tsx` — the grouped consent UI.
- `web/src/components/plugins/WidgetView.tsx` — **modify**: add the `iframe` branch.
- `web/src/components/plugins/PluginsPanel.tsx` — **modify**: per-plugin grant
  status + Revoke.
- `web/src/client/pluginContributions.ts` — **modify**: accept `kind:"iframe"`,
  clamp `html` length and `height` range.

## 4. Sandbox & origin model (6a)

The iframe is created with:

```html
<iframe
  sandbox="allow-scripts"      <!-- NO allow-same-origin → opaque/null origin -->
  srcdoc="<plugin html>"
  ...>
```

Consequences, all desirable:

- **Null/opaque origin.** No access to the app's `localStorage`, IndexedDB,
  cookies, or `window.parent` internals across the origin boundary.
- **No network.** Inherited app CSP (`connect-src 'self' ipc:`,
  `script-src 'self'`) plus the opaque origin mean `fetch`/`XHR`/`ws` to the
  outside fail. Plugins are compute + brokered-data only.
- **No same-origin DOM reach.** The frame cannot read or mutate the host DOM.
- **One channel.** The only host contact is `window.postMessage` to/from the
  frame, mediated entirely by the broker.

Alignment with [[content-trust-hardening]]: the existing CSP already sets
`frame-ancestors 'none'` and `object-src 'none'`; Tier-3 adds *child* frames,
which `frame-ancestors` does not restrict. We do **not** loosen the app CSP.
`srcdoc` content runs under the embedding document's CSP for frame creation but
executes inline scripts within the sandboxed frame (allowed by
`sandbox="allow-scripts"`); since the frame is null-origin and network-blocked,
inline script there cannot exfiltrate. We add no `script-src` relaxation to the
app document.

### 6b upgrade path (documented, not built)
When a plugin needs multi-file bundles or persistent storage: register a Tauri
custom protocol `plugin-sandbox://<id>/...` serving the plugin's on-disk `ui/`
dir at a **distinct opaque origin** (separate storage partition, per-frame CSP),
and change the widget variant to carry `entry: string` instead of inline `html`.
The broker API, permission model, lifecycle, and `height` field are identical, so
6b is additive. 6b requires a Rust protocol handler with strict path-traversal
guards (the Trail-of-Bits VS Code failure mode).

## 5. The broker — host API surface

### 5.1 Wire protocol

```ts
// plugin → host
type PluginRequest =
  | { t: "req"; id: string; method: string; params?: JsonValue }
  | { t: "event-sub"; topic: string };
// host → plugin
type HostMessage =
  | { t: "ready"; capabilities: PluginCapability[] }        // handshake ack
  | { t: "res"; id: string; ok: true;  result: JsonValue }
  | { t: "res"; id: string; ok: false; error: string }
  | { t: "event"; topic: string; payload: JsonValue };
```

### 5.2 Methods

| Method | Capability | Reuses | Notes |
|--------|-----------|--------|-------|
| `host.info` | — (silent) | — | `{ appVersion, theme, activePath }` |
| `ui.notice` | — (silent) | existing `notice` system | host toast; text clamped |
| `activeNote.read` | `activeNote.read` | store `openNotes[active]` | `{ path, title, text }` |
| `activeNote.subscribe` | `activeNote.read` | store subscription | pushes `event` topic `"activeNote"` on change |
| `activeNote.write` | `activeNote.write` | `setBuffer` + autosave debounce | replaces active buffer text; ⚠ mutation |
| `notes.read` | `notes.read` | existing read path | `{ path, text }` for any path; ⚠ read-all |
| `notes.search` | `notes.search` | engine `search_results` | ⚠ read-all |
| `command.invoke` | `command.invoke` | existing `invokePlugin` | only the plugin's OWN declared commands |

`command.invoke` is constrained to commands the *same plugin* declared in
`PluginSummary.commands` — a plugin cannot drive another plugin or host-internal
commands.

### 5.3 Enforcement (the wall)

Every inbound message passes, in order:

```ts
if (e.source !== iframeEl.contentWindow) return;        // 1. identity (origin is null → use source)
const msg = parseRequest(e.data); if (!msg) return;     // 2. shape-validate, drop malformed
if (!withinRateCap()) return;                           // 3. per-frame inbound rate cap (flood guard)
const cap = CAPABILITY_OF[msg.method];                  // 4. method → capability
if (cap && !granted.has(cap)) return reply(msg.id, { ok:false, error:"denied" });
const params = clampParams(msg.params);                 // 5. size/depth clamp (sanitiser posture)
// ...dispatch to store/client, reply within timeout...
```

- Origin is `null`, so authentication is by **`event.source` identity**, never an
  origin string.
- Param clamps reuse `pluginContributions.ts` limits (string length, list size,
  args bytes).
- The consent prompt is *consent*; the broker is the *wall* — every call is
  re-checked against the granted set (defense in depth).

## 6. Permission model

### 6.1 Declaration & storage

The plugin declares `capabilities` in `PluginSummary` (engine manifest). Grants
persist host-side, mirroring `cairn.keybindings`:

```ts
// localStorage "cairn.pluginGrants"
type PluginGrant = { version: string; granted: PluginCapability[] };
type PluginGrants = Record<string /*pluginId*/, PluginGrant>;
```

### 6.2 Consent flow

```
loadPlugins() ──▶ plugin has iframe widget + declares capabilities
                       │
   grants[id] covers requested set AND version matches?
     ├─ no  ─▶ render PermissionPrompt (iframe NOT mounted)
     │          user Allow  ─▶ persist grant ─▶ mount
     │          user Don't run ─▶ stay unmounted
     └─ yes ─▶ mount IframeHost + broker
```

### 6.3 Grouped plain-language presentation (Chrome-style)

Capabilities collapse into a few human risk statements; low-risk ones are silent:

| Capability | Risk group shown | Severity |
|-----------|------------------|----------|
| `activeNote.write` | "Modify the current note" | HIGH |
| `notes.read`, `notes.search` | "Read across your whole vault" | HIGH |
| `activeNote.read` | "Read the current note" | shown |
| `command.invoke` | — | silent (its own commands) |
| `host.info`, `ui.notice` | — | silent |

The prompt is **all-or-nothing** (Allow / Don't run) — no per-capability toggles.
Rationale (Q3): per-toggle consent is rubber-stamped in practice (nobody in the
plugin ecosystem ships it), explodes the partial-grant test matrix, and yields
half-broken plugins (a linter with write denied is useless). Transparency comes
from the grouped *list*, not from toggles.

### 6.4 Re-prompt & revoke

- **Re-prompt** when `version` changes or the requested capability set **expands**
  beyond `granted` (Figma + Chrome both re-gate on expansion).
- **Revoke** in Settings → `PluginsPanel`: per-plugin status + a Revoke action
  that removes the grant; the broker drops it and `IframeHost` unmounts the frame.

## 7. Placement & rendering (6a)

Reuse `sidebar.section`. `SlotRenderer` already iterates that slot; `WidgetView`
gains one branch:

```
sidebar.section entry, widget.kind === "iframe"
   → <WidgetView> dispatches to <IframeHost plugin height html capabilities/>
   → rendered inside the existing section chrome, height-bounded
```

`height` is clamped to a sane range (e.g. 80–600px) by the sanitiser. The narrow
sidebar (~280px) suits observers/linters; a roomy `panel.main` dock is the 6b
upgrade for rich editors (additive: one slot value + one mount point).

## 8. Lifecycle & error isolation

### 8.1 State machine (per iframe instance)

```
loadPlugins (epoch++)
   └─ grant? ─no─▶ PERMISSION_PROMPT ─deny─▶ NOT_MOUNTED
            └─yes─▶ MOUNTING ─(srcdoc, sandbox="allow-scripts")
                      └─▶ HANDSHAKING ─timeout(3s)─▶ ERROR (WidgetError "didn't start ⟳")
                            └─ frame posts handshake; host replies {t:"ready"} ─▶ READY
                                  └─ epoch bump / revoke / unmount / tab close ─▶ TEARDOWN
```

`TEARDOWN`: remove iframe element, broker unsubscribes its `message` listener and
store subscriptions, all pending requests reject.

### 8.2 Error isolation (extends Tier-2)

- Each iframe is wrapped in the **existing** `ErrorBoundary`, keyed
  `${plugin}:${id}:${epoch}` → a contribution refresh remounts and clears errors.
- A crashed frame shows the quiet `WidgetError` ("widget unavailable — retry");
  **the host never tears down** (Tier-2 parity).
- Broker requests carry a **per-request timeout (5s)** → reject with
  `{ ok:false, error:"timeout" }`; the plugin/host never hang.

### 8.3 Runtime policing (Q5 = B)

- **Per-frame inbound message rate cap** (e.g. drop > N messages/sec) so a
  `postMessage` flood cannot jam the host event loop. ~15 lines, no new concepts.
- **No CPU watchdog.** Iframe JS cannot be cleanly preempted; teardown-on-revoke
  (and collapsing the sidebar section) is the user's kill switch. A liveness
  watchdog (Q5 option C) adds false-positive teardowns for little gain.

## 9. Testing

Tests are part of done (Wave-2). The spec mandates coverage of:

- **Broker unit tests** (`pluginBroker.test.ts`): source-identity rejection;
  malformed-message drop; capability-gating (granted vs denied per method);
  param clamping; per-request timeout; rate-cap drop; `command.invoke` confined
  to the plugin's own commands; each method's happy path against a mock store.
- **Grants slice tests** (`pluginGrantsSlice.test.ts`): grant/persist/restore;
  version-bump re-prompt; expansion re-prompt; revoke clears + unmounts.
- **Sanitiser tests** (extend `pluginContributions.test.ts`): `iframe` widget now
  **accepted**; oversized `html` clamped/dropped; out-of-range `height` clamped;
  unknown capability values dropped from `capabilities`.
- **Component tests**: `PermissionPrompt` grouping (HIGH shown, low-risk silent);
  `IframeHost` lifecycle (handshake timeout → WidgetError; ready → broker live;
  revoke → unmount); `WidgetView` iframe branch; `PluginsPanel` revoke.
- **e2e**: mock plugin with an iframe widget → consent prompt → Allow → widget
  reads active note → writes it back → revoke in Settings → widget gone.
- **MockClient**: seed a demo iframe plugin (word-count) for manual + e2e use.

Run the FULL local gate before claiming green — `prettier --check`/format is easy
to miss; eslint won't catch it ([[ci-local-gates]]).

## 10. Risks & mitigations

| Risk | Mitigation |
|------|-----------|
| Frame forges messages / impersonates another frame | `event.source === contentWindow` identity check; broker is the sole authority |
| Sandbox misconfig (accidental `allow-same-origin`) | Lock the `sandbox` attr in `IframeHost`; unit-assert it; never templated from plugin data |
| Untrusted note content injected into frame | Note text crosses the broker as data only; frame renders it in its own null-origin DOM — cannot reach host. Host never renders plugin HTML outside the sandbox |
| `postMessage` flood freezes host | Per-frame inbound rate cap (§8.3) |
| Hanging request | Per-request 5s timeout (§8.2) |
| Write capability corrupts notes | All-or-nothing informed consent + autosave/undo path reuse; write goes through `setBuffer` (same path as the editor), not a raw file write |
| Capability creep on plugin update | Version + expansion re-prompt (§6.4) |
| Contract drift (engine vs vendored) | Re-sync via sync script; contract-drift CI ([[content-trust-hardening]]); never hand-edit vendored contract |
| 6b protocol path traversal (future) | Documented as a 6b requirement: strict path canonicalisation in the Rust handler |

## 11. Scope split

- **Phase 6a (this design → Wave-2 implementation):** `srcdoc`/null-origin
  iframe, broker with read+write surface, grouped consent + grants + revoke,
  `sidebar.section` placement, lifecycle + isolation + rate cap, tests.
- **Phase 6b (documented, deferred):** custom `plugin-sandbox://` protocol +
  multi-file bundles + persistent per-plugin storage, `panel.main` dock. Additive
  to 6a; requires Rust work.

## 12. Open contract dependency

6a needs the engine to add the `iframe` widget variant, the `capabilities` field,
and the `PluginCapability` enum, then a vendored-contract re-sync here. That is
the one cross-repo dependency; everything else is host-side and can be built and
tested against `MockClient` + a hand-seeded `PluginSummary` ahead of the engine
landing.
