# Security findings — cairn-web-ui

Scope: the web UI (`web/`) and the Tauri shell (`src-tauri/`). The UI consumes a
transport-blind contract (commands/queries/events) over either Tauri IPC or a
mock. Note content, search snippets, plugin metadata, and graph data all
originate from the backend / on-disk notes and are rendered in the webview.

General note: the app is, for an Obsidian-class editor, **mostly XSS-safe by
construction** — there is no `dangerouslySetInnerHTML`, no `innerHTML`, no
`eval`, and all backend-derived text is rendered through React (auto-escaped),
`textContent`, or CodeMirror decorations. The findings below are therefore
about transport/shell hardening, defense-in-depth, and content-trust gaps
rather than direct injection sinks.

---

## S1. Content-Security-Policy is disabled in the Tauri shell
**Severity: High**
**Location:** `src-tauri/tauri.conf.json:34-36` (`app.security.csp: null`); `web/index.html` (no `<meta http-equiv="Content-Security-Policy">`)

`csp` is explicitly `null`, and the HTML entry point ships no CSP meta tag
either. The webview therefore runs with no script/style/connect/img
restrictions. The app loads remote content (see S2) and renders user/plugin/
backend-derived strings; with CSP off, any future HTML-injection regression, a
compromised dependency (the app pulls in CodeMirror, react-force-graph,
Radix, fontsource, etc.), or a malicious note that manages to inject markup has
unrestricted ability to exfiltrate data or load remote scripts. Tauri's own
docs treat a real CSP as the primary in-webview mitigation.

**Impact:** Removes the main defense-in-depth layer for a desktop app that
opens arbitrary user folders and renders their content. A single rendering bug
or supply-chain compromise escalates straight to data exfiltration.

**Recommendation:** Set a strict CSP, e.g.
`default-src 'self'; img-src 'self' asset: https: data:; style-src 'self' 'unsafe-inline'; script-src 'self'; connect-src 'self' ipc:`,
then tighten `img-src`/`connect-src` per S2. Tauri rewrites `'self'`/`asset:`
correctly per-platform. Add a matching `<meta>` CSP for the browser/dev build.

---

## S2. Remote and `data:` image URLs in note content are fetched automatically
**Severity: Medium**
**Location:** `web/src/components/editor/imageResolver.ts:6-9`; `web/src/components/editor/livePreview.ts:307-325`; `web/src/components/editor/widgets/imageWidget.ts:24-29`; `web/src/client/tauri.ts:53-58` (`assetUrl` passes `https?:`/`data:` straight through)

Any `![alt](https://attacker.example/beacon.png)` in a note is rendered as a
live `<img src>` the moment the line scrolls into live-preview, with no user
opt-in. Because notes are git-backed and shareable, a note authored elsewhere
(or produced by a plugin) becomes a tracking/exfil beacon: opening it leaks the
victim's IP, time-of-open, and — via query strings — note identifiers to a
third-party server. `data:` URLs are also passed through unrestricted.

**Impact:** Privacy leak / silent network beacons from "just opening a note";
in a desktop app this is a meaningful deanonymization vector for shared vaults.

**Recommendation:** Default to not loading remote images (render a placeholder
with a click-to-load affordance, Obsidian-style), and constrain `img-src` in
the CSP (S1). At minimum, gate remote/`data:` images behind a setting.

---

## S3. `assetUrl` builds local file paths by naive string concatenation (path-traversal latent vuln)
**Severity: Medium** (High if/when the asset protocol is enabled)
**Location:** `web/src/client/tauri.ts:53-58`

```ts
assetUrl(relPath) {
  if (/^(https?:|data:)/i.test(relPath)) return relPath;
  if (!this.root) return relPath;
  const sep = this.root.endsWith("/") ? "" : "/";
  return convertFileSrc(`${this.root}${sep}${relPath}`);
}
```

`relPath` comes directly from a note's image markdown (`im[2]` in
`livePreview.ts:317`). It is concatenated onto the cairn root with zero
normalization, so `![](../../../../etc/passwd)` (or an absolute path) resolves to
a `convertFileSrc` URL pointing **outside the opened cairn**. Today this is
latent because no asset-protocol scope is configured (see S4), so the URL won't
actually load — but the moment someone enables the asset protocol to make local
images work, this becomes arbitrary local-file disclosure into the webview.

**Impact:** Latent local-file read / sandbox-escape-of-vault once asset access
is granted. Even now it is a correctness landmine.

**Recommendation:** Reject absolute paths and any `relPath` that escapes the
root after normalization; resolve against the root and verify the result is
still inside it before calling `convertFileSrc`. Pair with a tightly-scoped
asset-protocol capability (e.g. `$APP`/the cairn dir only).

---

## S4. Local images are silently non-functional (no asset-protocol capability)
**Severity: Low** (security-adjacent / correctness)
**Location:** `src-tauri/capabilities/default.json` (only `core:default`, `dialog:default`); `src-tauri/tauri.conf.json` (no `app.security.assetProtocol`)

`TauriHost.assetUrl` calls `convertFileSrc` (producing an `asset://` URL), but
the asset protocol is neither enabled nor scoped in the Tauri config or
capabilities. Local relative images in notes therefore never load in the real
app (they work only under the mock's blank-PNG stub). This is both a UX gap
(S-side: broken images, no fallback) and the reason S3 is currently latent.

**Recommendation:** When wiring local images, enable the asset protocol with a
scope restricted to the open cairn directory, and combine with S3's
normalization. Until then, document that local images are unsupported.

---

## S5. Backend event payloads are trusted without shape validation
**Severity: Low**
**Location:** `web/src/client/tauri.ts:22-34` (`listen<Event>` casts the payload); `web/src/store/store.ts:197-210` (subscribe handler switches on `e.type`)

The Tauri `listen` payload and `invoke` responses are cast to the contract
types with no runtime validation; the store dispatches on `e.type`/`res.type`
directly. This is acceptable because the backend is in the same trust domain,
but it means a contract drift (the vendored TS contract is pinned to commit
`293c60d…` in `web/src/contract/source.ts`, synced manually by
`scripts/sync-contract.sh`) produces silent misbehavior rather than a clear
error. There is no defense if the daemon transport (mentioned in the README) is
ever networked and less trusted.

**Recommendation:** Before/when a networked daemon transport lands, add a thin
runtime validator (zod/valibot or hand-rolled tag checks) at the client
boundary. Add a CI check that the vendored contract commit matches the engine.

---

## S6. `localStorage` deserialization is broadly defensive — minor residual risk
**Severity: Low (informational)**
**Location:** `web/src/components/shortcuts/keybindingPersistence.ts:6-23`; `web/src/components/graph/colorGroups.ts:9-30`; `web/src/components/graph/localGraph.ts:62-83`; `web/src/components/tabs/tabsPersistence.ts:25-44`; `web/src/components/tree/treePersistence.ts:4-21`

All `JSON.parse(localStorage…)` sites are wrapped in try/catch and validate
shape (array/string/`isValid` guards, depth clamping). Good. One nit:
`loadOverrides` copies arbitrary string keys from parsed JSON into an
`Overrides` object (`out[k] = v`); a `__proto__` key carries no exploit here
(value is a string, ignored by the prototype, and only `COMMAND_DEFS` ids are
ever read back), but the pattern is worth hardening with `Object.create(null)`
or a key allowlist.

**Recommendation:** Use `Object.create(null)` for parsed maps, or restrict keys
to known command ids.

---

## Non-findings (verified safe)
- No `dangerouslySetInnerHTML` / `innerHTML` / `eval` / `new Function` / `document.write` anywhere in `web/src`.
- Search snippets + highlight ranges are sliced and rendered through React (`searchHighlight.ts`, `SearchResults.tsx:48-60`) — escaped.
- Table cells, wikilink labels, image alt, and plugin notices are set via `textContent`/React children — escaped (`tableWidget.ts`, `wikilinkWidget.ts`, `store.ts:497`).
- No secrets/tokens in client code; no `fetch`/CORS/credentials usage (all backend I/O is Tauri IPC).
- GitHub Actions: no `pull_request_target` + checkout-of-PR-code pattern; `claude*.yml` run on `issue_comment`/`pull_request` with default tokens.
