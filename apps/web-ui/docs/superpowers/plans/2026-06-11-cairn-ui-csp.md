# Cairn UI — Restrictive Content-Security-Policy Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to
> implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the disabled Tauri CSP (`app.security.csp: null`) with a strict,
explicit Content-Security-Policy that still allows every feature the app legitimately
needs, restoring the primary in-webview defense.

**Architecture:** This is a config-driven change in `src-tauri/tauri.conf.json`. The
CSP is set as a single directive string. Tauri v2 processes this string and per-platform
rewrites `'self'` and auto-injects nonces/hashes for its own bootstrap scripts. Because
the change is config-driven, **runtime verification (build + run + devtools console)
matters more than a unit test** — Task 3 is the load-bearing task.

**Tech Stack:** Tauri v2 (2.11.2), Vite, React 19, Tailwind v4, CodeMirror, Radix,
react-force-graph-2d, `@fontsource-variable/inter` (bundled).

---

## Finding being remediated

`audit/security.md` **S1 — Content-Security-Policy is disabled in the Tauri shell**
(Severity: High). `src-tauri/tauri.conf.json` ships `app.security.csp: null`; the webview
runs with no script/style/connect/img restrictions, so any HTML-injection regression,
compromised dependency, or malicious note can exfiltrate data or load remote scripts.

## The CSP (and why each directive is what it is)

```
default-src 'self';
script-src 'self';
style-src 'self' 'unsafe-inline';
font-src 'self';
img-src 'self' asset: http://asset.localhost data:;
connect-src 'self' ipc: http://ipc.localhost;
object-src 'none';
base-uri 'self';
frame-ancestors 'none'
```

| Directive | Value | Why |
|-----------|-------|-----|
| `default-src` | `'self'` | Baseline: everything not otherwise specified must be same-origin (the bundled app). |
| `script-src` | `'self'` | Vite emits external module scripts from `'self'`. Tauri auto-appends hashes for its own injected IPC bootstrap, so **no `unsafe-inline`/`unsafe-eval`** is needed. If the production build emits an inline script (e.g. Vite's modulepreload polyfill), Task 3 will surface a violation and we add the specific hash — not `unsafe-inline`. |
| `style-src` | `'self' 'unsafe-inline'` | CodeMirror, Radix, and react-force-graph inject runtime `<style>` tags and inline `style=` attributes that cannot be hashed ahead of time. `'unsafe-inline'` for **styles only** (no script execution) is the standard accepted allowance and matches the security.md recommendation. Tailwind's compiled stylesheet loads from `'self'`. |
| `font-src` | `'self'` | `@fontsource-variable/inter` is bundled; `@font-face` URLs resolve same-origin. No remote fonts. |
| `img-src` | `'self' asset: http://asset.localhost data:` | Local vault images load through Tauri's asset protocol — `asset:` (macOS/iOS scheme) and `http://asset.localhost` (Windows/Linux). `data:` is required by the app's **own** bundle (`data:image/png` + `data:image/svg+xml` appear in the built JS) and is not a network beacon (no egress), so it does not undercut the S2 privacy goal. **`https:` is deliberately NOT allowed in this PR** — see coordination note. |
| `connect-src` | `'self' ipc: http://ipc.localhost` | Tauri IPC (`invoke`, `listen`) uses the `ipc:` custom scheme (macOS) / `http://ipc.localhost` (Windows/Linux). No other network endpoints. |
| `object-src` | `'none'` | Hardening: no `<object>`/`<embed>`/`<applet>`. |
| `base-uri` | `'self'` | Hardening: a markup injection cannot repoint relative URLs via `<base>`. |
| `frame-ancestors` | `'none'` | Hardening: the desktop webview must not be embeddable. |

**Deliberately NOT added** (YAGNI / would weaken posture): `unsafe-eval`, `unsafe-inline`
in `script-src`, wildcard hosts, `frame-src` overrides (the future sandboxed-iframe plugin
tier will adjust CSP when it lands). `blob:` is omitted from `img-src` until a real feature
needs it (Task 3 would surface it).

## Code-review outcomes & coordination

A reviewer pass produced two substantive points; both were verified against the codebase
before acting:

1. **`img-src https:` would undercut S2 (resolved — `https:` dropped).** `assetUrl`
   (`web/src/client/tauri.ts:53-58`) passes `https?:`/`data:` straight through with **no
   app-layer gate today** — the opt-in gate is owned by the not-yet-merged
   `11-cairn-ui-images` session. Shipping `img-src https:` now would let a malicious note's
   `![](https://attacker/beacon.png)` fire on note-open with CSP *permitting* it — i.e. the
   one mechanism that could backstop the beacon would be configured to allow it. So `https:`
   is dropped: until the gate lands, **CSP is the backstop** and blocks all remote images
   (which is also the images session's intended default-OFF end state). **Coordination:** the
   `11-cairn-ui-images` PR re-adds `https:` to `img-src` *together with* its opt-in gate, so
   the gate is always the primary control and CSP the secondary lock — never CSP alone
   permitting an ungated remote load. `data:` is kept (app's own bundle needs it; not a
   beacon).

2. **Browser/dev `<meta>` CSP (deferred follow-up, with reason).** S1 also names
   `web/index.html` (no `<meta>` CSP) for the dev/browser build. A *static* `<meta>` CSP is
   **not** a safe addition in this PR: (a) Tauri injects its IPC bootstrap as an inline
   script and adds that script's hash to the **header** CSP it generates at runtime — a
   developer `<meta>` with `script-src 'self'` carries no such hash, and the browser enforces
   the **intersection** of header + meta, so the meta would block Tauri's own bootstrap and
   break the shipped webview (the build verified in Task 3); (b) `pnpm dev` (Vite HMR) uses
   inline scripts + `eval` + a `ws:` connection that the same strict meta would break. The
   shipped product is Tauri-first, so the Tauri-config CSP (header-injected, with Tauri's
   nonces) is the real security boundary and is now closed. **Follow-up:** a dev/browser
   `<meta>` CSP (or a Vite-prod-only injected CSP that excludes the Tauri build) is tracked
   separately and best done alongside the dev-CSP work.

**Not in scope / dependency:** local-image rendering also requires the Tauri **asset
protocol** to be enabled (finding S4 — `capabilities/default.json` currently grants only
`core:default`/`dialog:default`). The `asset:`/`http://asset.localhost` tokens here are
correct and forward-looking, but local images won't actually load until S4 is wired; this
PR does not claim to enable them.

---

## Task 1: Replace `csp: null` with the strict CSP

**Files:**
- Modify: `src-tauri/tauri.conf.json` (the `app.security.csp` key)

- [ ] **Step 1: Edit `app.security` in `src-tauri/tauri.conf.json`**

Replace:
```json
    "security": {
      "csp": null
    }
```
with:
```json
    "security": {
      "csp": "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; font-src 'self'; img-src 'self' asset: http://asset.localhost https: data:; connect-src 'self' ipc: http://ipc.localhost; object-src 'none'; base-uri 'self'; frame-ancestors 'none'"
    }
```

- [ ] **Step 2: Validate JSON parses**

Run: `python3 -m json.tool src-tauri/tauri.conf.json > /dev/null && echo OK`
Expected: `OK`

---

## Task 2: Install web dependencies (prerequisite for build)

**Files:** none (environment setup)

- [ ] **Step 1: Install frontend deps**

Run: `cd web && pnpm install --frozen-lockfile`
Expected: completes; `web/node_modules` present.

- [ ] **Step 2: Confirm the frontend builds cleanly**

Run: `cd web && pnpm build`
Expected: `web/dist/` produced. Inspect `web/dist/index.html` for any **inline**
`<script>` tags (not `src=`-only). If present, note the exact script content — it will
need an explicit hash in `script-src` after Task 3 observes the violation.

---

## Task 3: Build, RUN, and verify no CSP violations (load-bearing)

**Files:** none (runtime verification)

This is the step the brief insists must not be skipped. The production CSP is injected
by Tauri into the **bundled** app (not necessarily into an external Vite `devUrl`), so
verification must exercise a real Tauri-served build.

- [ ] **Step 1: Determine the working Tauri build/run command**

Try in order (use whichever is wired in this repo):
- `cd src-tauri && cargo tauri dev` (cargo-tauri CLI), or
- `pnpm --dir web exec tauri dev`, or
- `cargo tauri build --debug` then launch the bundled app.

First build compiles the `cairn-*` git deps + Tauri (several minutes; needs network to
`github.com/tau-rs/cairn`). If the toolchain/network makes a full build infeasible in
this environment, fall back to the documented degraded path in Step 4 — **do not revert
to `csp: null`.**

- [ ] **Step 2: Open the app and exercise legitimate features**

With devtools open (Console + Network), confirm:
- App window loads (no blank screen).
- A note renders (markdown → rendered document).
- Styles/fonts apply (Inter font, Tailwind layout, CodeMirror editor styling).
- The graph view renders (react-force-graph canvas).
- The ⌘K command palette opens (Radix overlay).
- A **local** vault image renders (if the asset protocol is enabled by the images session).

- [ ] **Step 3: Read the devtools Console for CSP violations**

Expected: **zero** `Refused to … because it violates the following Content Security
Policy directive` messages for the legitimate features above.

If a violation appears:
- inline **style** → already permitted (`style-src 'unsafe-inline'`); should not occur.
- inline **script** with a hash in the message → add that exact `'sha256-…'` to
  `script-src` (NOT `unsafe-inline`), re-build, re-verify.
- `asset:`/`ipc:` blocked → confirm the platform host token (`http://asset.localhost`
  / `http://ipc.localhost`) is present; add if the message names another scheme.
- a **remote image** blocked → expected when remote images are OFF; this is correct,
  not a regression.

- [ ] **Step 4: Capture real output**

Paste the actual Console output (or the build/run command output if the build was
infeasible) into the verification notes. If degraded: document exactly what was and
wasn't verifiable, tighten as far as proven, and record a follow-up — never `csp: null`.

---

## Task 4: Review, commit, PR

- [ ] **Step 1:** `superpowers:requesting-code-review` on the diff.
- [ ] **Step 2:** Commit (`Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`).
- [ ] **Step 3:** Push; `gh pr create -R tau-rs/cairn-web-ui --base main`, citing
  `audit/security.md` S1 and documenting what each directive permits and why.
- [ ] **STOP — no merge.**

---

## Self-Review

- **Spec coverage:** Brief step 1 (define restrictive CSP allowing legit needs) → Task 1
  + directive table. Step 2 (implement in tauri.conf.json) → Task 1. Step 3
  (build/run/verify, don't skip) → Task 3. Step 4 (review/commit/PR) → Task 4. img-src
  coordination with `11-cairn-ui-images` → documented in directive table. ✅
- **Placeholders:** none — exact CSP string, exact file, exact commands. ✅
- **Constraints:** one finding, one PR; strict CSP with documented allowances
  (`style-src 'unsafe-inline'`, `img-src https:`/`data:`); explicit no-fallback-to-null. ✅
