# Tier-3 Plugin Sandbox Phase 6b Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Serve each plugin's on-disk `ui/` bundle at a distinct opaque origin via a custom `plugin-sandbox://` Tauri protocol (origin-native storage, per-frame CSP, strict path-canonicalisation), switch the iframe widget from inline `html` to a bundle `entry`, and add a resizable bottom `panel.main` dock — all additive over Phase 6a.

**Architecture:** A Rust custom-protocol handler resolves `plugin-sandbox://<id>/<path>` against a host-registered `{id → uiRoot}` allow-list, canonicalises within the root, and serves files with a locked-down per-frame CSP. The TS host pushes the allow-list after `loadPlugins`. `IframeHost` (from 6a) swaps its `srcdoc` mount for a `src` URL + `allow-same-origin`; a new `PanelDock` is a second mount point in the main column.

**Tech Stack:** Rust + Tauri 2.11 (`register_uri_scheme_protocol`), React + Zustand, Vitest, Playwright e2e, vendored ts-rs contract.

---

## ⚠️ Prerequisite gate — read before starting

**Phase 6a must be merged first.** 6b extends 6a's `IframeHost.tsx`, `pluginBroker.ts`, grants slice, `PermissionPrompt.tsx`, and the 6a contract deltas (`iframe` widget variant, `PluginSummary.capabilities`, `PluginCapability`). None of those exist in the repo yet. Do **not** begin Task 2+ until:

- 6a is merged to `main` (the broker, `IframeHost`, grants, and the `iframe` widget variant exist), **and**
- The engine (`tau-rs/cairn`) has added the 6b contract deltas (§ Task 1) and the vendored contract is re-synced here.

Tasks 2–6 (Rust + host registration) have **no 6a dependency** and can be built/tested against `MockClient` ahead of 6a landing. Tasks 7–11 (TS UI) depend on 6a. Each task notes its dependency.

**Commit convention:** Conventional Commits, imperative, scoped `feat(plugins)` / `feat(tauri)` / `test(plugins)`. Commit after each task's tests pass.

**Local gate before any "green" claim** (from `web/`): `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check`. For Rust (from `src-tauri/`): `cargo test && cargo clippy --all-targets -- -D warnings`. `prettier --check` is easy to miss — eslint won't catch formatting.

---

## File structure

**Rust (`src-tauri/src/`):**
- `plugin_roots.rs` (new) — `PluginRoots` Tauri-managed state (canonicalised `{id → PathBuf}` allow-list) + the `set_plugin_ui_roots` command.
- `plugin_protocol.rs` (new) — `resolve_in_root()` (pure, unit-testable canonicalisation) + `handle()` (the URI-scheme request handler: lookup, MIME, CSP).
- `lib.rs` (modify) — `mod` the two files, `register_uri_scheme_protocol`, `.manage(PluginRoots::default())`, add `set_plugin_ui_roots` to `generate_handler!`.

**TS:**
- `web/src/client/host.ts` (modify) — add `setPluginUiRoots` to `CairnHost`; impl in `MockHost`/`alwaysOpenHost` (no-op) and `TauriHost` (invoke).
- `web/src/client/pluginContributions.ts` (modify) — accept `iframe` widget with `entry`, the `panel.main` slot, and ingest `uiRoot`.
- `web/src/store/store.ts` (modify) — after `loadPlugins`, push `{id → uiRoot}` to the host; add a `panelDock` UI slice.
- `web/src/components/plugins/IframeHost.tsx` (modify, **6a file**) — `src` URL + `sandbox="allow-scripts allow-same-origin"`; assert frame origin ≠ host origin.
- `web/src/components/plugins/PanelDock.tsx` (new) — bottom dock: tab strip, active selection, collapse, resize; mounts `IframeHost`.
- `web/src/components/shells/AppShell.tsx` + `Shell.tsx` (modify) — render `<PanelDock/>` under the editor.
- `web/src/client/mock.ts` (modify) — seed a multi-file demo bundle plugin.
- `web/e2e/tier3-plugins-6b.spec.ts` (new) — e2e.

---

## Task 1: Engine contract deltas (other repo — coordination task, no host code)

**This task is done in `tau-rs/cairn`, not here.** It is listed so the executor blocks on it. The engine must:

- Change the `iframe` `PluginWidget` variant: replace `html: String` with `entry: String`.
- Add `ui_root: Option<PathBuf>` (serialised `uiRoot: string | null`) to `PluginSummary`.
- Add `"panel.main"` to the `PluginSlot` enum.

- [ ] **Step 1:** Confirm the engine PR adds the three deltas above and regenerates ts-rs bindings.
- [ ] **Step 2:** Re-sync the vendored contract here:

Run: `scripts/sync-contract.sh ../cairn`
Expected: `web/src/contract/PluginWidget.ts` now shows `entry: string` (not `html`); `PluginSummary.ts` has `uiRoot: string | null`; `PluginSlot.ts` includes `"panel.main"`.

- [ ] **Step 3: Commit**

```bash
git add web/src/contract
git commit -m "chore(contract): sync 6b deltas (entry, uiRoot, panel.main)"
```

> If the engine deltas are not ready, build Tasks 2–6 first (they don't import the contract), and hand-seed a `PluginSummary` with `uiRoot` in `MockClient` (Task 10) to exercise the TS path. Leave a `// TODO(contract-sync): drop after engine 6b deltas land` marker on any temporary cast.

---

## Task 2: Rust — `resolve_in_root` canonicalisation (the security core)

**Files:**
- Create: `src-tauri/src/plugin_protocol.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod plugin_protocol;`)

No 6a dependency. Pure function + unit tests — the traversal wall.

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/plugin_protocol.rs`:

```rust
use std::path::{Path, PathBuf};

/// Why a request could not be served. Maps to HTTP status in `handle`.
#[derive(Debug, PartialEq, Eq)]
pub enum ServeError {
    NotFound,   // 404: unknown file / missing / not a regular file
    Forbidden,  // 403: resolved path escapes the bundle root
    BadRequest, // 400: malformed path (NUL byte, etc.)
}

/// Resolve `rel` (the URL path after the host) against an already-canonicalised
/// `root`, returning the canonical file path IFF it is a regular file inside
/// `root`. This is the traversal wall: canonicalisation resolves `..` AND
/// symlinks, then the prefix test rejects anything that escaped.
pub fn resolve_in_root(root: &Path, rel: &str) -> Result<PathBuf, ServeError> {
    let decoded = percent_decode(rel)?;
    if decoded.as_bytes().contains(&0) {
        return Err(ServeError::BadRequest);
    }
    let trimmed = decoded.trim_start_matches('/');
    let candidate = root.join(trimmed);
    let canonical = std::fs::canonicalize(&candidate).map_err(|_| ServeError::NotFound)?;
    if !canonical.starts_with(root) {
        return Err(ServeError::Forbidden);
    }
    if !canonical.is_file() {
        return Err(ServeError::NotFound);
    }
    Ok(canonical)
}

/// Minimal percent-decode (no external dep). Invalid escapes → BadRequest.
fn percent_decode(s: &str) -> Result<String, ServeError> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                if i + 2 >= bytes.len() {
                    return Err(ServeError::BadRequest);
                }
                let hi = (bytes[i + 1] as char).to_digit(16).ok_or(ServeError::BadRequest)?;
                let lo = (bytes[i + 2] as char).to_digit(16).ok_or(ServeError::BadRequest)?;
                out.push((hi * 16 + lo) as u8);
                i += 3;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|_| ServeError::BadRequest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn bundle() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("ui")).unwrap();
        fs::write(dir.path().join("ui/index.html"), b"<h1>hi</h1>").unwrap();
        fs::write(dir.path().join("secret.txt"), b"top secret").unwrap();
        dir
    }

    fn root(d: &tempfile::TempDir) -> PathBuf {
        std::fs::canonicalize(d.path().join("ui")).unwrap()
    }

    #[test]
    fn serves_a_real_file() {
        let d = bundle();
        let got = resolve_in_root(&root(&d), "index.html").unwrap();
        assert!(got.ends_with("index.html"));
    }

    #[test]
    fn dotdot_traversal_is_forbidden() {
        let d = bundle();
        // ../secret.txt escapes ui/ — must be rejected (Forbidden or NotFound,
        // never served).
        let res = resolve_in_root(&root(&d), "../secret.txt");
        assert!(matches!(res, Err(ServeError::Forbidden) | Err(ServeError::NotFound)));
    }

    #[test]
    fn nul_byte_is_bad_request() {
        let d = bundle();
        assert_eq!(resolve_in_root(&root(&d), "a%00b.html"), Err(ServeError::BadRequest));
    }

    #[test]
    fn missing_file_is_not_found() {
        let d = bundle();
        assert_eq!(resolve_in_root(&root(&d), "nope.html"), Err(ServeError::NotFound));
    }

    #[test]
    fn directory_is_not_found() {
        let d = bundle();
        assert_eq!(resolve_in_root(&root(&d), ""), Err(ServeError::NotFound));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_is_rejected() {
        let d = bundle();
        std::os::unix::fs::symlink(d.path().join("secret.txt"), d.path().join("ui/link.txt")).unwrap();
        let res = resolve_in_root(&root(&d), "link.txt");
        assert!(matches!(res, Err(ServeError::Forbidden) | Err(ServeError::NotFound)));
    }
}
```

Add to `src-tauri/src/lib.rs` near the top (after the `use` block):

```rust
mod plugin_protocol;
mod plugin_roots;
```

(`plugin_roots` is created in Task 3; add both `mod` lines now and create the file in Task 3, or comment `plugin_roots` until then.)

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p cairn-ui-lib plugin_protocol 2>/dev/null || (cd src-tauri && cargo test plugin_protocol)`
Expected: FAIL to compile (`tempfile` dev-dep already present; if not, `cargo add --dev tempfile` in `src-tauri`). Once compiling, tests run.

- [ ] **Step 3: Implementation** — already written in Step 1 (the function is complete).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test plugin_protocol`
Expected: PASS (6 tests incl. unix symlink).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/plugin_protocol.rs src-tauri/src/lib.rs
git commit -m "feat(tauri): plugin-sandbox path canonicalisation with traversal wall"
```

---

## Task 3: Rust — `PluginRoots` state + `set_plugin_ui_roots` command

**Files:**
- Create: `src-tauri/src/plugin_roots.rs`
- Modify: `src-tauri/src/lib.rs`

No 6a dependency.

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/plugin_roots.rs`:

```rust
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

/// The serve allow-list: plugin id → canonicalised ui/ dir. Only dirs in here
/// are ever served by the plugin-sandbox protocol. Replaced wholesale on each
/// `set_plugin_ui_roots` so unloaded plugins immediately stop being serveable.
#[derive(Default)]
pub struct PluginRoots(pub Mutex<HashMap<String, PathBuf>>);

impl PluginRoots {
    /// Replace the whole map. Each incoming root is canonicalised; entries that
    /// don't exist / fail to canonicalise are dropped (never served).
    pub fn replace(&self, incoming: HashMap<String, String>) {
        let mut map = self.0.lock().expect("plugin roots mutex poisoned");
        map.clear();
        for (id, path) in incoming {
            if let Ok(canon) = std::fs::canonicalize(&path) {
                if canon.is_dir() {
                    map.insert(id, canon);
                }
            }
        }
    }

    pub fn get(&self, id: &str) -> Option<PathBuf> {
        self.0.lock().expect("plugin roots mutex poisoned").get(id).cloned()
    }
}

/// Replace the registered ui-root allow-list. Called by the host after
/// loadPlugins with `{ id: absoluteUiDir }` for every plugin that has a bundle.
#[tauri::command]
pub fn set_plugin_ui_roots(
    roots: std::collections::HashMap<String, String>,
    state: tauri::State<'_, PluginRoots>,
) -> Result<(), String> {
    state.replace(roots);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn replace_canonicalises_and_drops_missing() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("ui")).unwrap();
        let roots = PluginRoots::default();
        let mut incoming = HashMap::new();
        incoming.insert("good".into(), d.path().join("ui").to_string_lossy().into_owned());
        incoming.insert("missing".into(), "/no/such/dir".into());
        roots.replace(incoming);
        assert!(roots.get("good").is_some());
        assert!(roots.get("missing").is_none());
    }

    #[test]
    fn replace_drops_stale_ids() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("ui")).unwrap();
        let ui = d.path().join("ui").to_string_lossy().into_owned();
        let roots = PluginRoots::default();
        roots.replace(HashMap::from([("a".to_string(), ui.clone())]));
        assert!(roots.get("a").is_some());
        // second call without "a" must evict it
        roots.replace(HashMap::from([("b".to_string(), ui)]));
        assert!(roots.get("a").is_none());
        assert!(roots.get("b").is_some());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test plugin_roots`
Expected: FAIL to compile until `mod plugin_roots;` is in `lib.rs` (added in Task 2 Step 1).

- [ ] **Step 3: Implementation** — complete in Step 1.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test plugin_roots`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/plugin_roots.rs
git commit -m "feat(tauri): PluginRoots allow-list state + set_plugin_ui_roots command"
```

---

## Task 4: Rust — the protocol request handler (`handle`)

**Files:**
- Modify: `src-tauri/src/plugin_protocol.rs`

No 6a dependency. Wires `resolve_in_root` + `PluginRoots` into an HTTP-style response with MIME + the per-frame CSP.

- [ ] **Step 1: Write the failing test**

Append to `src-tauri/src/plugin_protocol.rs` (above `#[cfg(test)]`):

```rust
use crate::plugin_roots::PluginRoots;

/// The locked-down per-frame CSP set on EVERY served response. `'self'` =
/// plugin-sandbox://<id>. No network (connect-src 'none'), no inline scripts
/// (script-src 'self'), only the host may embed the frame.
const PLUGIN_CSP: &str = "default-src 'none'; script-src 'self'; \
    style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; \
    font-src 'self'; connect-src 'none'; base-uri 'none'; form-action 'none'; \
    frame-ancestors tauri://localhost";

fn mime_for(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html",
        Some("js") | Some("mjs") => "text/javascript",
        Some("css") => "text/css",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("woff2") => "font/woff2",
        Some("wasm") => "application/wasm",
        _ => "application/octet-stream",
    }
}

fn status_for(e: &ServeError) -> u16 {
    match e {
        ServeError::NotFound => 404,
        ServeError::Forbidden => 403,
        ServeError::BadRequest => 400,
    }
}

/// Resolve `(id, rel)` against the registered roots and build the response
/// (bytes + content-type + CSP), or an error status with an empty body.
pub fn build_response(
    roots: &PluginRoots,
    id: &str,
    rel: &str,
) -> (u16, &'static str, Vec<u8>) {
    let Some(root) = roots.get(id) else {
        return (404, "text/plain", Vec::new());
    };
    match resolve_in_root(&root, rel) {
        Ok(file) => match std::fs::read(&file) {
            Ok(bytes) => (200, mime_for(&file), bytes),
            Err(_) => (404, "text/plain", Vec::new()),
        },
        Err(e) => (status_for(&e), "text/plain", Vec::new()),
    }
}

/// Expose the CSP constant for the handler in lib.rs.
pub fn plugin_csp() -> &'static str {
    PLUGIN_CSP
}
```

Append these tests inside the existing `#[cfg(test)] mod tests`:

```rust
    use crate::plugin_roots::PluginRoots;
    use std::collections::HashMap;

    fn roots_with(id: &str, d: &tempfile::TempDir) -> PluginRoots {
        let r = PluginRoots::default();
        r.replace(HashMap::from([(
            id.to_string(),
            d.path().join("ui").to_string_lossy().into_owned(),
        )]));
        r
    }

    #[test]
    fn build_serves_html_with_mime() {
        let d = bundle();
        let r = roots_with("wc", &d);
        let (status, mime, body) = build_response(&r, "wc", "index.html");
        assert_eq!(status, 200);
        assert_eq!(mime, "text/html");
        assert_eq!(body, b"<h1>hi</h1>");
    }

    #[test]
    fn build_unknown_id_is_404() {
        let d = bundle();
        let r = roots_with("wc", &d);
        let (status, _, _) = build_response(&r, "other", "index.html");
        assert_eq!(status, 404);
    }

    #[test]
    fn build_traversal_is_403_or_404() {
        let d = bundle();
        let r = roots_with("wc", &d);
        let (status, _, _) = build_response(&r, "wc", "../secret.txt");
        assert!(status == 403 || status == 404);
    }

    #[test]
    fn csp_blocks_network_and_inline_script() {
        let csp = plugin_csp();
        assert!(csp.contains("connect-src 'none'"));
        assert!(csp.contains("script-src 'self'"));
        assert!(!csp.contains("script-src 'self' 'unsafe-inline'"));
    }
```

- [ ] **Step 2: Run tests to verify they fail/pass**

Run: `cd src-tauri && cargo test plugin_protocol`
Expected: the four new tests compile and PASS (functions are complete in Step 1).

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/plugin_protocol.rs
git commit -m "feat(tauri): plugin-sandbox response builder with MIME and per-frame CSP"
```

---

## Task 5: Rust — register the scheme + manage state in `lib.rs`

**Files:**
- Modify: `src-tauri/src/lib.rs` (the `run()` builder + `generate_handler!`)

No 6a dependency. This is the only non-TDD task (Tauri's runtime registration isn't unit-testable in isolation; it's exercised by the e2e in Task 11). Keep it a thin adapter over the Task-4 tested core.

- [ ] **Step 1: Add the imports** near the top of `lib.rs`:

```rust
use tauri::http::Response;
use plugin_roots::{set_plugin_ui_roots, PluginRoots};
```

- [ ] **Step 2: In `run()`**, register the scheme and manage the state. Modify the builder chain:

```rust
tauri::Builder::default()
    .plugin(tauri_plugin_dialog::init())
    .manage(CairnState::default())
    .manage(PluginRoots::default())
    .register_uri_scheme_protocol("plugin-sandbox", |ctx, request| {
        // host = <id>, path = /<rel>
        let id = request.uri().host().unwrap_or("").to_string();
        let rel = request.uri().path().trim_start_matches('/').to_string();
        let roots = ctx.app_handle().state::<PluginRoots>();
        let (status, mime, body) = plugin_protocol::build_response(&roots, &id, &rel);
        Response::builder()
            .status(status)
            .header("Content-Type", mime)
            .header("Content-Security-Policy", plugin_protocol::plugin_csp())
            .header("X-Content-Type-Options", "nosniff")
            .body(body)
            .unwrap()
    })
    .setup(|app| { /* ...unchanged... */ Ok(()) })
    .invoke_handler(tauri::generate_handler![
        send_command,
        run_query,
        pick_and_open_cairn,
        current_cairn,
        set_plugin_ui_roots
    ])
    .run(tauri::generate_context!())
    .expect("error while running cairn");
```

- [ ] **Step 3: Verify it compiles and existing Rust tests still pass**

Run: `cd src-tauri && cargo build && cargo test`
Expected: builds; all prior tests (engine + plugin_protocol + plugin_roots) PASS.

- [ ] **Step 4: Clippy clean**

Run: `cd src-tauri && cargo clippy --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat(tauri): register plugin-sandbox URI scheme and PluginRoots state"
```

---

## Task 6: Host — `setPluginUiRoots` on `CairnHost`

**Files:**
- Modify: `web/src/client/host.ts`
- Modify: `web/src/client/tauri.ts` (`TauriHost`)
- Test: `web/src/client/host.test.ts` (create if absent)

No 6a dependency.

- [ ] **Step 1: Write the failing test**

Create/append `web/src/client/host.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import { MockHost } from "./host";

describe("MockHost.setPluginUiRoots", () => {
  it("is a no-op that resolves (mock has no real protocol)", async () => {
    const host = new MockHost();
    await expect(host.setPluginUiRoots({ wc: "/tmp/wc/ui" })).resolves.toBeUndefined();
  });
});
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd web && pnpm test host.test`
Expected: FAIL — `setPluginUiRoots` is not a function.

- [ ] **Step 3: Implementation**

In `web/src/client/host.ts`, add to the `CairnHost` interface:

```ts
  /** Register the per-plugin ui/ dir allow-list with the native protocol
   *  handler. `{ pluginId: absoluteUiDir }`. No-op off Tauri (no protocol). */
  setPluginUiRoots(roots: Record<string, string>): Promise<void>;
```

Add to `alwaysOpenHost`:

```ts
  setPluginUiRoots: () => Promise.resolve(),
```

Add to `MockHost`:

```ts
  setPluginUiRoots(_roots: Record<string, string>) {
    return Promise.resolve();
  }
```

In `web/src/client/tauri.ts`, add to `TauriHost`:

```ts
  async setPluginUiRoots(roots: Record<string, string>): Promise<void> {
    await invoke("set_plugin_ui_roots", { roots });
  }
```

- [ ] **Step 4: Run to verify it passes**

Run: `cd web && pnpm test host.test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/client/host.ts web/src/client/tauri.ts web/src/client/host.test.ts
git commit -m "feat(plugins): host.setPluginUiRoots seam (Tauri invoke, mock no-op)"
```

---

## Task 7: Sanitizer — accept `iframe`+`entry`, `panel.main`, ingest `uiRoot`

**Files:**
- Modify: `web/src/client/pluginContributions.ts`
- Test: `web/src/client/pluginContributions.test.ts`

**Depends on 6a** (6a adds the `iframe` kind to `WIDGET_KINDS` with `html`; this task changes it to `entry`). If 6a's `iframe` branch isn't present yet, add the branch here in full.

- [ ] **Step 1: Write the failing tests**

Append to `web/src/client/pluginContributions.test.ts`:

```ts
import { sanitizeContributions, type PluginContribution } from "./pluginContributions";

describe("6b: iframe entry + panel.main", () => {
  const base = { id: "c1", slot: "panel.main" };

  it("accepts an iframe widget with a relative entry", () => {
    const out = sanitizeContributions([
      { ...base, widget: { kind: "iframe", entry: "index.html", height: 300 } },
    ]);
    expect(out).toHaveLength(1);
    expect(out[0].widget).toMatchObject({ kind: "iframe", entry: "index.html" });
  });

  it("drops an iframe whose entry is absolute or traverses", () => {
    const out = sanitizeContributions([
      { ...base, id: "abs", widget: { kind: "iframe", entry: "/etc/passwd", height: null } },
      { ...base, id: "dots", widget: { kind: "iframe", entry: "../x.html", height: null } },
    ]);
    expect(out).toHaveLength(0);
  });

  it("accepts the panel.main slot", () => {
    const out = sanitizeContributions([
      { id: "p", slot: "panel.main", widget: { kind: "iframe", entry: "a.html", height: null } },
    ]);
    expect(out[0].slot).toBe("panel.main");
  });
});
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd web && pnpm test pluginContributions`
Expected: FAIL — `iframe` kind / `panel.main` slot dropped as unknown.

- [ ] **Step 3: Implementation**

In `web/src/client/pluginContributions.ts`:

Add `"panel.main"` to `PLUGIN_SLOTS` and `"iframe"` to `WIDGET_KINDS`:

```ts
export const PLUGIN_SLOTS = [
  "sidebar.section",
  "topbar.action",
  "command",
  "panel.main",
] as const;
export const WIDGET_KINDS = ["text", "action", "list", "iframe"] as const;
```

Add a max-entry-length const near the others:

```ts
export const MAX_ENTRY = 512;
```

Add an `entry` validator (rejects absolute / `..` / backslash paths — a cheap pre-filter before the Rust wall):

```ts
/** A safe relative bundle path: non-empty, no leading slash, no `..` segment,
 *  no backslash, within the length budget. Returns null if unsafe. */
function safeEntry(x: unknown): string | null {
  if (typeof x !== "string" || x.length === 0 || x.length > MAX_ENTRY) return null;
  if (x.startsWith("/") || x.includes("\\")) return null;
  if (x.split("/").some((seg) => seg === ".." || seg === "")) return null;
  return x;
}
```

In `sanitizeWidget`, add the `iframe` branch before the `list` branch:

```ts
  if (kind === "iframe") {
    const entry = safeEntry(raw.entry);
    if (entry === null) return drop(report, "iframe widget: unsafe or missing entry");
    return {
      kind: "iframe",
      entry,
      height:
        typeof raw.height === "number" && Number.isFinite(raw.height)
          ? raw.height
          : null,
    };
  }
```

> The contract's `PluginWidget` iframe variant must be `{ kind:"iframe"; entry:string; height:number|null }` after the Task-1 sync. If not yet synced, the return needs `as PluginWidget` with a `// TODO(contract-sync)` marker.

- [ ] **Step 4: Run to verify it passes**

Run: `cd web && pnpm test pluginContributions`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/client/pluginContributions.ts web/src/client/pluginContributions.test.ts
git commit -m "feat(plugins): sanitize iframe entry + panel.main slot"
```

---

## Task 8: Store — push ui-roots after loadPlugins

**Files:**
- Modify: `web/src/store/store.ts` (`loadPlugins`)
- Test: `web/src/store/store.test.ts`

No 6a dependency (uses `uiRoot` from `PluginSummary`, hand-seeded in tests).

- [ ] **Step 1: Write the failing test**

Append to `web/src/store/store.test.ts` (follow the existing store-test harness that injects a mock host — mirror the nearby `loadPlugins` tests):

```ts
it("loadPlugins pushes {id: uiRoot} to the host", async () => {
  const calls: Record<string, string>[] = [];
  const host = {
    currentCairn: () => Promise.resolve("/v"),
    openCairn: () => Promise.resolve("/v"),
    assetUrl: () => "",
    setPluginUiRoots: (r: Record<string, string>) => {
      calls.push(r);
      return Promise.resolve();
    },
  };
  // client returns one plugin with a uiRoot and one without
  const store = makeStore({
    host,
    plugins: [
      { id: "wc", name: "WC", version: "1", commands: [], contributions: [], capabilities: null, uiRoot: "/v/.cairn/plugins/wc/ui" },
      { id: "noui", name: "N", version: "1", commands: [], contributions: [], capabilities: null, uiRoot: null },
    ],
  });
  await store.getState().loadPlugins();
  expect(calls.at(-1)).toEqual({ wc: "/v/.cairn/plugins/wc/ui" });
});
```

> Use whatever store-construction helper the existing tests use (`makeStore`/`createStore`); match the seam that injects `client` and `host`.

- [ ] **Step 2: Run to verify it fails**

Run: `cd web && pnpm test store.test -t "pushes"`
Expected: FAIL — host never called.

- [ ] **Step 3: Implementation**

In `loadPlugins()` in `web/src/store/store.ts`, after the plugin list is fetched and contributions grouped, add:

```ts
    // 6b: register the per-plugin ui/ dir allow-list with the native protocol.
    // Only plugins that ship a bundle (uiRoot != null). Replace-whole-map on
    // the Rust side drops unloaded plugins. Best-effort: a failure only means
    // iframe widgets can't load, never that loadPlugins fails.
    const roots: Record<string, string> = {};
    for (const p of plugins) {
      if (p.uiRoot) roots[p.id] = p.uiRoot;
    }
    void host.setPluginUiRoots(roots).catch(() => {});
```

> `host` is already available in the store closure (the same seam `assetUrl` uses). If the store doesn't hold `host`, thread it through the existing host injection.

- [ ] **Step 4: Run to verify it passes**

Run: `cd web && pnpm test store.test -t "pushes"`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/store/store.ts web/src/store/store.test.ts
git commit -m "feat(plugins): register plugin ui-roots with the host after loadPlugins"
```

---

## Task 9: `IframeHost` 6b delta + `PanelDock`

**Files:**
- Modify: `web/src/components/plugins/IframeHost.tsx` (**6a file — depends on 6a**)
- Create: `web/src/components/plugins/PanelDock.tsx`
- Modify: `web/src/store/store.ts` (`panelDock` UI slice)
- Test: `web/src/components/plugins/PanelDock.test.tsx`, `IframeHost.test.tsx`

**Depends on 6a.**

- [ ] **Step 1: `IframeHost` delta test**

In `web/src/components/plugins/IframeHost.test.tsx`, add:

```tsx
it("mounts the bundle via the plugin-sandbox protocol with allow-same-origin", () => {
  render(<IframeHost pluginId="wc" entry="index.html" height={200} capabilities={[]} />);
  const frame = screen.getByTitle(/wc/i) as HTMLIFrameElement;
  expect(frame.getAttribute("src")).toBe("plugin-sandbox://wc/index.html");
  expect(frame.getAttribute("sandbox")).toBe("allow-scripts allow-same-origin");
});
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd web && pnpm test IframeHost`
Expected: FAIL — 6a `IframeHost` used `srcdoc`/`allow-scripts` only and took `html`, not `entry`.

- [ ] **Step 3: Implementation — `IframeHost` delta**

Change the 6a `IframeHost` props from `html: string` to `pluginId: string; entry: string`, and the iframe element:

```tsx
  const src = `plugin-sandbox://${pluginId}/${entry}`;
  // Safety invariant: allow-same-origin is only sound because the bundle is on
  // a DIFFERENT origin than the host. Never templated from plugin-controlled
  // markup; the sandbox attr is a hard-coded literal.
  if (new URL(src).origin === window.location.origin) {
    return <WidgetError onRetry={...} />; // never serve same-origin as host
  }
  return (
    <iframe
      title={`plugin: ${pluginId}`}
      src={src}
      sandbox="allow-scripts allow-same-origin"
      style={{ height, width: "100%", border: 0 }}
      ref={frameRef}
    />
  );
```

The broker wiring (`frameRef.current.contentWindow`, message listener, handshake, rate cap, timeouts) is **unchanged from 6a** — only `srcdoc`→`src` and the sandbox attr change.

- [ ] **Step 4: `PanelDock` test**

Create `web/src/components/plugins/PanelDock.test.tsx`:

```tsx
import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { PanelDock } from "./PanelDock";

// Seed two panel.main contributions via the store test seam, then:
describe("PanelDock", () => {
  it("renders nothing when there are no panel.main contributions", () => {
    seedContributions({ "panel.main": [] });
    const { container } = render(<PanelDock />);
    expect(container).toBeEmptyDOMElement();
  });

  it("shows a tab strip and switches the active panel when >1", () => {
    seedContributions({ "panel.main": [
      { plugin: "a", c: { id: "a1", slot: "panel.main", widget: { kind: "iframe", entry: "a.html", height: null }, title: "Alpha", icon: null, order: null }, epoch: 1 },
      { plugin: "b", c: { id: "b1", slot: "panel.main", widget: { kind: "iframe", entry: "b.html", height: null }, title: "Beta", icon: null, order: null }, epoch: 1 },
    ]});
    render(<PanelDock />);
    expect(screen.getByRole("tab", { name: "Alpha" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: "Beta" }));
    expect(screen.getByTitle(/plugin: b/i)).toBeInTheDocument();
  });

  it("collapses and restores", () => {
    seedContributions({ "panel.main": [/* one entry */] });
    render(<PanelDock />);
    fireEvent.click(screen.getByRole("button", { name: /collapse/i }));
    expect(screen.queryByTitle(/plugin:/i)).not.toBeInTheDocument();
  });
});
```

> `seedContributions` mirrors the helper used in `SlotRenderer.test.tsx`. Reuse that pattern.

- [ ] **Step 5: Implementation — `panelDock` slice + `PanelDock`**

Add to the store's `UiState` (or a sibling `panelDock` slice) — persisted via the existing UI-persistence path:

```ts
  panelDock: { activeId: string | null; collapsed: boolean; height: number };
```

with a setter `setPanelDock(patch: Partial<...>)` mirroring `setUi`.

Create `web/src/components/plugins/PanelDock.tsx`:

```tsx
import { useShallow } from "zustand/react/shallow";
import { useCairn, useActions } from "../../app/cairnStore";
import { ErrorBoundary } from "../ErrorBoundary";
import { IframeHost } from "./IframeHost";

const DEFAULT_HEIGHT = 240;

export function PanelDock() {
  const entries = useCairn(useShallow((s) => s.pluginContributions["panel.main"] ?? []));
  const dock = useCairn(useShallow((s) => s.panelDock));
  const { setPanelDock } = useActions();
  if (entries.length === 0) return null;

  const activeId = dock.activeId ?? entries[0].c.id;
  const active = entries.find((e) => e.c.id === activeId) ?? entries[0];
  const height = dock.collapsed ? 0 : dock.height || DEFAULT_HEIGHT;

  return (
    <section
      className="shrink-0 border-t border-border bg-surface"
      style={{ height: dock.collapsed ? "auto" : height }}
      aria-label="Plugin panel"
    >
      <div className="flex items-center gap-1 border-b border-border px-2 py-1" role="tablist">
        {entries.map((e) => (
          <button
            key={`${e.plugin}:${e.c.id}`}
            role="tab"
            aria-selected={e.c.id === active.c.id}
            className={e.c.id === active.c.id ? "text-xs text-text" : "text-xs text-muted"}
            onClick={() => setPanelDock({ activeId: e.c.id, collapsed: false })}
          >
            {e.c.title ?? e.plugin}
          </button>
        ))}
        <button
          className="ml-auto text-xs text-muted"
          aria-label={dock.collapsed ? "Expand plugin panel" : "Collapse plugin panel"}
          onClick={() => setPanelDock({ collapsed: !dock.collapsed })}
        >
          {dock.collapsed ? "▴" : "▾"}
        </button>
      </div>
      {!dock.collapsed && (
        <ErrorBoundary
          key={`${active.plugin}:${active.c.id}:${active.epoch}`}
          fallback={(reset) => (
            <button onClick={reset} className="text-xs text-faint italic">
              panel unavailable — retry
            </button>
          )}
        >
          {active.c.widget.kind === "iframe" && (
            <IframeHost
              pluginId={active.plugin}
              entry={active.c.widget.entry}
              height={height}
              capabilities={/* from grants — same source SlotRenderer uses in 6a */ []}
            />
          )}
        </ErrorBoundary>
      )}
    </section>
  );
}
```

> Resize handle: add a 4px top-edge drag region updating `setPanelDock({ height })` on pointer-move. Keep it minimal; the test only asserts collapse + switch, but include the drag handle for the e2e.

- [ ] **Step 6: Run all component tests**

Run: `cd web && pnpm test IframeHost PanelDock`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add web/src/components/plugins/IframeHost.tsx web/src/components/plugins/PanelDock.tsx web/src/components/plugins/PanelDock.test.tsx web/src/components/plugins/IframeHost.test.tsx web/src/store/store.ts
git commit -m "feat(plugins): protocol-backed IframeHost + panel.main bottom dock"
```

---

## Task 10: Shell wiring + demo bundle in MockClient

**Files:**
- Modify: `web/src/components/Shell.tsx`, `web/src/components/shells/AppShell.tsx`
- Modify: `web/src/client/mock.ts`
- Test: `web/src/components/Shell.test.tsx`

**Shell change depends on 6a + Task 9.**

- [ ] **Step 1: Shell test**

In `web/src/components/Shell.test.tsx`, assert the dock mounts under the editor region:

```tsx
it("renders the plugin panel dock below the editor", () => {
  // seed one panel.main contribution, then render the shell
  render(<Shell {...regions} />);
  expect(screen.getByLabelText("Plugin panel")).toBeInTheDocument();
});
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd web && pnpm test Shell.test -t "plugin panel dock"`
Expected: FAIL — dock not rendered.

- [ ] **Step 3: Implementation**

In `Shell.tsx`, wrap the editor `<main>` in a vertical flex and add the dock under it:

```tsx
        <main className="flex min-w-0 flex-1 flex-col overflow-hidden">
          <div className="min-h-0 flex-1 overflow-auto p-3">{props.editor}</div>
          <PanelDock />
        </main>
```

Import `PanelDock`. Apply the equivalent change to `AppShell.tsx`'s desktop branch; tablet/mobile reuse the same `<PanelDock/>` (it renders nothing when no `panel.main` contributions exist, so it's safe everywhere).

- [ ] **Step 4: Seed the demo bundle**

In `web/src/client/mock.ts`, add a demo plugin to the `ListPlugins` response with a `panel.main` iframe contribution and a `uiRoot`. Since the mock has no real protocol, the bundle won't actually load in unit tests — the e2e (Task 11) uses a real fixture dir. Add:

```ts
{
  id: "readability",
  name: "Readability",
  version: "1.0.0",
  commands: [],
  capabilities: ["activeNote.read"],
  uiRoot: "(mock)/readability/ui",
  contributions: [
    {
      id: "readability-panel",
      slot: "panel.main",
      widget: { kind: "iframe", entry: "index.html", height: 240 },
      title: "Readability",
      icon: null,
      order: null,
    },
  ],
},
```

- [ ] **Step 5: Run shell + mock tests**

Run: `cd web && pnpm test Shell mock`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add web/src/components/Shell.tsx web/src/components/shells/AppShell.tsx web/src/client/mock.ts web/src/components/Shell.test.tsx
git commit -m "feat(plugins): mount panel.main dock in shells + seed demo plugin"
```

---

## Task 11: e2e — real bundle round trip

**Files:**
- Create: `web/e2e/fixtures/readability/ui/index.html` (+ `app.js`)
- Create: `web/e2e/tier3-plugins-6b.spec.ts`

**Depends on 6a + all above.** Runs against the Tauri build (or a test harness that registers the protocol). If the e2e suite runs against the web/mock build (no Tauri protocol), assert the consent→dock→read flow and skip the file-serve assertion with a `test.fixme` + comment; the Rust protocol is covered by Tasks 2–4.

- [ ] **Step 1: Create the fixture bundle**

`web/e2e/fixtures/readability/ui/index.html`:

```html
<!doctype html>
<html><head><meta charset="utf-8"></head>
<body>
  <div id="count">…</div>
  <script src="app.js"></script>
</body></html>
```

`web/e2e/fixtures/readability/ui/app.js` (no inline script — CSP forbids it):

```js
// Persist a counter to prove origin-native storage works.
const n = Number(localStorage.getItem("loads") || "0") + 1;
localStorage.setItem("loads", String(n));
// Handshake + request active note via the 6a broker protocol.
parent.postMessage({ t: "req", id: "1", method: "activeNote.read" }, "*");
window.addEventListener("message", (e) => {
  if (e.data?.t === "res" && e.data.id === "1" && e.data.ok) {
    const words = (e.data.result.text || "").split(/\s+/).filter(Boolean).length;
    document.getElementById("count").textContent = `words: ${words} · loads: ${n}`;
  }
});
```

- [ ] **Step 2: Write the e2e**

`web/e2e/tier3-plugins-6b.spec.ts`:

```ts
import { test, expect } from "@playwright/test";

test("panel.main plugin: consent → dock → reads note → storage persists", async ({ page }) => {
  await page.goto("/");
  // Open the readability panel (command palette or dock affordance).
  await page.getByRole("tab", { name: "Readability" }).click().catch(() => {});
  // Consent prompt (6a) — grant.
  await page.getByRole("button", { name: /allow/i }).click();
  const frame = page.frameLocator('iframe[title*="readability" i]');
  await expect(frame.locator("#count")).toContainText("loads: 1");
  // Reload → storage persists.
  await page.reload();
  await page.getByRole("tab", { name: "Readability" }).click().catch(() => {});
  await expect(frame.locator("#count")).toContainText("loads: 2");
});
```

- [ ] **Step 3: Run the e2e**

Run: `cd web && pnpm test:e2e tier3-plugins-6b`
Expected: PASS (or `test.fixme` documented if running against the mock build without the native protocol).

- [ ] **Step 4: Commit**

```bash
git add web/e2e/fixtures/readability web/e2e/tier3-plugins-6b.spec.ts
git commit -m "test(plugins): e2e panel.main bundle round trip with persisted storage"
```

---

## Final verification

- [ ] From `web/`: `pnpm test && pnpm typecheck && pnpm lint && pnpm format:check` — all green.
- [ ] From `src-tauri/`: `cargo test && cargo clippy --all-targets -- -D warnings` — all green.
- [ ] Manually in the running Tauri app: a `panel.main` plugin shows a consent prompt; Allow mounts it in the bottom dock; the dock resizes + collapses + persists; the plugin reads the active note; a value written to `localStorage` survives reload; uninstalling/disabling the plugin (re-`loadPlugins`) makes its bundle 404.
- [ ] Grep for stray `as PluginWidget` / `as never` casts left for the contract lag; each carries a `// TODO(contract-sync): drop after engine 6b deltas land` marker.

## Post-engine-sync follow-up (separate, tiny PR)

Once `tau-rs/cairn` ships the 6b deltas and the vendored contract is re-synced:
- Remove the temporary casts (Tasks 7, 8, 10).
- Confirm `PluginSummary.uiRoot`, the `iframe`/`entry` variant, and `panel.main` resolve to the generated contract types (or keep the host copies as the trust-boundary allow-list, matching `pluginContributions.ts`'s existing pattern — preferred).

## Out of scope (documented, not built)

Editor-tab "replace the note" plugin surface (Obsidian `WorkspaceLeaf` / VSCode `WebviewPanel` model) for *replace*-style rich editors; per-plugin storage quotas/inspection; plugin network allow-listing.
