# Content-Trust / Transport Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Harden the backend↔webview boundary: enable a tightly-scoped asset protocol so note images load (S4), give broken images a fallback (U5), runtime-validate event/response shapes at the client boundary (S5), add a CI contract-drift guard (DX3), and null-proto the keybinding override map (S6).

**Architecture:** Five small, independent changes grouped as one PR under the shared "trust the bytes crossing into the webview" theme. Web changes are TDD (vitest). Tauri config (S4) and CI drift (DX3) are verified with captured real output. No unrelated refactors.

**Tech Stack:** TypeScript + vitest (web), Rust + Tauri v2 (`src-tauri`), bash + GitHub Actions (CI).

---

## File Structure

- `web/src/components/shortcuts/keybindingPersistence.ts` — (S6) parse overrides into a null-prototype map.
- `web/src/client/contractGuards.ts` — **new** (S5) thin tag-checking validators for `Event` / `CommandResponse` / `QueryResponse`.
- `web/src/client/tauri.ts` — (S5) call the validators on `invoke` responses and `listen` payloads.
- `web/src/components/editor/widgets/imageWidget.ts` + `livePreview.css` — (U5) `onerror` fallback on the `<img>`.
- `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`, `src-tauri/src/lib.rs` — (S4) enable + runtime-scope the asset protocol to the open cairn dir.
- `scripts/sync-contract.sh` — (DX3) prettier-format the vendored contract so it matches the committed form.
- `scripts/check-contract-drift.sh` — **new** (DX3) re-sync against the pinned engine commit and fail on a diff.
- `.github/workflows/ci.yml` — (DX3) a `contract-drift` job + summary wiring.

---

## Task 1 (S6): Null-prototype keybinding override map

**Files:**
- Modify: `web/src/components/shortcuts/keybindingPersistence.ts:13-16`
- Test: `web/src/components/shortcuts/keybindingPersistence.test.ts`

- [ ] **Step 1: Write the failing test** — append inside the `describe`:

```ts
  it("ignores a __proto__ key and yields a null-prototype map", () => {
    localStorage.setItem(
      "cairn.keybindings",
      JSON.stringify({ "new-note": "Mod+J", __proto__: "Mod+X" }),
    );
    const out = loadOverrides();
    expect(out["new-note"]).toBe("Mod+J");
    // No prototype: a `__proto__` data key cannot poison Object.prototype and
    // the map has no inherited members.
    expect(Object.getPrototypeOf(out)).toBeNull();
    expect(Object.prototype.hasOwnProperty.call(out, "__proto__")).toBe(false);
  });
```

- [ ] **Step 2: Run it, verify it fails**

Run: `cd web && pnpm test -- keybindingPersistence`
Expected: FAIL on `Object.getPrototypeOf(out)` not being null (current `{}` literal has `Object.prototype`).

- [ ] **Step 3: Implement** — change the map construction:

```ts
    const out: Overrides = Object.create(null) as Overrides;
    for (const [k, v] of Object.entries(parsed as Record<string, unknown>)) {
      if (v === null || typeof v === "string") out[k] = v;
    }
    return out;
```

Note: `JSON.parse` already treats `__proto__` as an own data key (not the prototype setter), so `Object.entries` enumerates it; copying into a null-proto map makes it an inert own key. Keep the existing `if (v === null || typeof v === "string")` filter.

- [ ] **Step 4: Run tests, verify green** — `cd web && pnpm test -- keybindingPersistence` → PASS (all 5).

- [ ] **Step 5: Commit** — `git add` the two files; `feat(shortcuts): parse keybinding overrides into a null-prototype map (S6)`.

---

## Task 2 (S5): Runtime shape validation at the Tauri client boundary

**Files:**
- Create: `web/src/client/contractGuards.ts`
- Create: `web/src/client/contractGuards.test.ts`
- Modify: `web/src/client/tauri.ts`
- Test: `web/src/client/tauri.test.ts`

- [ ] **Step 1: Write the failing guard unit test** — `web/src/client/contractGuards.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import {
  assertEvent,
  assertCommandResponse,
  assertQueryResponse,
  ContractShapeError,
} from "./contractGuards";

describe("contractGuards", () => {
  it("passes a known-tag event through unchanged", () => {
    const e = { type: "committed", commit: "c1" };
    expect(assertEvent(e)).toBe(e);
  });
  it("passes known-tag command/query responses through", () => {
    const c = { type: "done" };
    const q = { type: "paths", paths: ["a.md"] };
    expect(assertCommandResponse(c)).toBe(c);
    expect(assertQueryResponse(q)).toBe(q);
  });
  it("rejects an unknown tag with a clear ContractShapeError", () => {
    expect(() => assertEvent({ type: "bogus" })).toThrow(ContractShapeError);
    expect(() => assertEvent({ type: "bogus" })).toThrow(/event/);
    expect(() => assertEvent({ type: "bogus" })).toThrow(/bogus/);
  });
  it("rejects a missing/invalid type, null, and non-objects", () => {
    expect(() => assertCommandResponse({})).toThrow(ContractShapeError);
    expect(() => assertQueryResponse(null)).toThrow(ContractShapeError);
    expect(() => assertEvent("nope")).toThrow(ContractShapeError);
    expect(() => assertEvent({ type: 5 })).toThrow(ContractShapeError);
  });
});
```

- [ ] **Step 2: Run it, verify it fails** — `cd web && pnpm test -- contractGuards` → FAIL (module not found).

- [ ] **Step 3: Implement `web/src/client/contractGuards.ts`**:

```ts
import type { Event, CommandResponse, QueryResponse } from "../contract";

/** The known discriminant tags for each contract union. Kept in lockstep with
 *  the vendored contract; the DX3 drift check guards the contract itself, and
 *  an unknown tag here means engine↔UI contract drift (S5). */
const EVENT_TYPES = [
  "note_changed",
  "note_deleted",
  "committed",
  "reindexed",
] as const;
const COMMAND_RESPONSE_TYPES = ["done", "committed", "plugin_result"] as const;
const QUERY_RESPONSE_TYPES = [
  "note",
  "paths",
  "search_results",
  "notes",
  "graph",
  "tags",
  "plugins",
] as const;

/** Raised when a value crossing the backend boundary doesn't carry a known
 *  discriminant `type`. Thin by design (S5): we tag-check the union, not the
 *  inner fields — a full schema rebuild is out of scope. */
export class ContractShapeError extends Error {
  constructor(what: string, got: unknown) {
    const tag =
      typeof got === "string" ? JSON.stringify(got) : "missing/invalid `type`";
    super(`Malformed ${what} from backend: unexpected ${tag}`);
    this.name = "ContractShapeError";
  }
}

function tagOf(x: unknown): string | undefined {
  if (typeof x !== "object" || x === null) return undefined;
  const t = (x as { type?: unknown }).type;
  return typeof t === "string" ? t : undefined;
}

function assertTagged<T>(
  x: unknown,
  allowed: readonly string[],
  what: string,
): T {
  const tag = tagOf(x);
  if (tag === undefined || !allowed.includes(tag)) {
    throw new ContractShapeError(what, tag);
  }
  return x as T;
}

export const assertEvent = (x: unknown): Event =>
  assertTagged(x, EVENT_TYPES, "event");
export const assertCommandResponse = (x: unknown): CommandResponse =>
  assertTagged(x, COMMAND_RESPONSE_TYPES, "command response");
export const assertQueryResponse = (x: unknown): QueryResponse =>
  assertTagged(x, QUERY_RESPONSE_TYPES, "query response");
```

- [ ] **Step 4: Run guard test, verify green** — `cd web && pnpm test -- contractGuards` → PASS.

- [ ] **Step 5: Write the failing tauri-boundary tests** — append to `web/src/client/tauri.test.ts` inside `describe("TauriClient", ...)`:

```ts
  it("runQuery rejects a malformed response with a clear error", async () => {
    invoke.mockResolvedValueOnce({ type: "not_a_real_query_response" });
    const c = new TauriClient();
    await expect(c.runQuery({ type: "list_notes" })).rejects.toThrow(
      /Malformed query response/,
    );
  });

  it("sendCommand rejects a malformed response with a clear error", async () => {
    invoke.mockResolvedValueOnce({ nope: true });
    const c = new TauriClient();
    await expect(
      c.sendCommand({ type: "write_note", path: "a.md", contents: "x" }),
    ).rejects.toThrow(/Malformed command response/);
  });

  it("subscribe routes a malformed event payload to onError and never calls cb", async () => {
    let handler: (e: { payload: unknown }) => void = () => {};
    listen.mockImplementationOnce(
      (_name: string, h: (e: { payload: unknown }) => void) => {
        handler = h;
        return Promise.resolve(vi.fn());
      },
    );
    const c = new TauriClient();
    const cb = vi.fn();
    const onError = vi.fn();
    c.subscribe(cb, onError);
    handler({ payload: { type: "garbage" } });
    expect(cb).not.toHaveBeenCalled();
    expect(onError).toHaveBeenCalledTimes(1);
    expect(String(onError.mock.calls[0][0])).toMatch(/Malformed event/);
  });
```

- [ ] **Step 6: Run them, verify they fail** — `cd web && pnpm test -- tauri` → 3 new FAIL (responses pass through unvalidated; cb called with garbage).

- [ ] **Step 7: Implement in `web/src/client/tauri.ts`** — add the import and wire the guards:

Add after the existing imports:

```ts
import {
  assertEvent,
  assertCommandResponse,
  assertQueryResponse,
} from "./contractGuards";
```

Replace `sendCommand` / `runQuery`:

```ts
  async sendCommand(command: Command): Promise<CommandResponse> {
    return assertCommandResponse(
      await invoke<unknown>("send_command", { command }),
    );
  }
  async runQuery(query: Query): Promise<QueryResponse> {
    return assertQueryResponse(await invoke<unknown>("run_query", { query }));
  }
```

In `subscribe`, replace the `listen` line so the payload is validated before dispatch; a bad shape routes to `onError` (the same degraded-state seam as an attach failure) instead of mis-dispatching:

```ts
    const pending = listen<unknown>("cairn://event", (e) => {
      try {
        cb(assertEvent(e.payload));
      } catch (err) {
        onError?.(err);
      }
    });
```

(`noteTags` already shape-checks `res.type !== "notes"`; leave it — it now consumes the validated `runQuery`.)

- [ ] **Step 8: Run tauri tests, verify green** — `cd web && pnpm test -- tauri` → PASS (all, incl. the 3 new). Then `pnpm typecheck`.

- [ ] **Step 9: Commit** — `feat(client): validate event/response shapes at the Tauri boundary (S5)`.

---

## Task 3 (U5): Broken-image fallback in ImageWidget

**Files:**
- Modify: `web/src/components/editor/widgets/imageWidget.ts:33-43`
- Modify: `web/src/components/editor/livePreview.css` (after `.cm-lp-img.block`)
- Test: `web/src/components/editor/widgets/imageWidget.test.ts`

Constraint: the widget MUST NOT restructure its own DOM (remove the `<img>`, insert a span) — CodeMirror's MutationObserver reconciles childList changes inside `cm-content` as text edits and would delete the markdown (see the existing `onLoadImage` comment + test). So the `onerror` handler only mutates the existing `<img>` node in place (a class + a marker attribute); CSS turns it into a visible "unavailable" placeholder, and the browser's native broken-image rendering surfaces the `alt`.

- [ ] **Step 1: Write the failing test** — append inside `describe("ImageWidget", ...)`:

```ts
  it("marks a ready <img> as unavailable on load error without restructuring DOM", () => {
    const w = new ImageWidget(
      { kind: "ready", url: "asset://missing/x.png" },
      "diagram alt",
      false,
      0,
      vi.fn(),
      vi.fn(),
    );
    const el = w.toDOM() as HTMLImageElement;
    const parent = document.createElement("div");
    parent.appendChild(el);
    el.dispatchEvent(new Event("error"));
    // Same node, still an <img>, alt preserved for native fallback rendering.
    expect(parent.firstChild).toBe(el);
    expect(el.tagName).toBe("IMG");
    expect(el.alt).toBe("diagram alt");
    expect(el.classList.contains("cm-lp-img-error")).toBe(true);
  });
```

- [ ] **Step 2: Run it, verify it fails** — `cd web && pnpm test -- imageWidget` → FAIL (no `cm-lp-img-error` class added).

- [ ] **Step 3: Implement** — in `imageEl`, add an `error` listener before the `mousedown` listener:

```ts
  private imageEl(url: string): HTMLImageElement {
    const img = document.createElement("img");
    img.className = this.block ? "cm-lp-img block" : "cm-lp-img";
    img.src = url;
    img.alt = this.alt;
    // The asset failed to load (missing file, asset protocol disabled/unscoped,
    // …). Mark the node so CSS renders a clear "unavailable" placeholder; the
    // browser already paints the `alt` for a broken <img>. We must NOT swap the
    // widget's DOM here (remove the <img>, insert a span): CodeMirror's
    // MutationObserver reconciles childList changes in cm-content as text edits
    // and would delete the markdown. A class/attribute change on the existing
    // node is safe.
    img.addEventListener("error", () => {
      img.classList.add("cm-lp-img-error");
      img.dataset.cairnUnavailable = "true";
    });
    img.addEventListener("mousedown", (e) => {
      e.preventDefault();
      this.onEdit(this.from);
    });
    return img;
  }
```

- [ ] **Step 4: Add CSS** — in `web/src/components/editor/livePreview.css`, after the `.cm-lp-img.block { … }` rule (line ~120):

```css
.cm-lp-img-error {
  min-width: 8em;
  min-height: 2.5em;
  padding: 0.25em 0.5em;
  border: 1px dashed #3a3a45;
  border-radius: 6px;
  color: #9a9ba6;
  font-size: 0.85em;
  object-fit: contain;
}
```

- [ ] **Step 5: Run tests, verify green** — `cd web && pnpm test -- imageWidget` → PASS (all 7).

- [ ] **Step 6: Commit** — `feat(editor): show an unavailable fallback for images that fail to load (U5)`.

---

## Task 4 (S4): Enable + runtime-scope the asset protocol to the open cairn dir

**Files:**
- Modify: `src-tauri/tauri.conf.json` (add `app.security.assetProtocol`)
- Modify: `src-tauri/Cargo.toml:24` (add the `protocol-asset` feature)
- Modify: `src-tauri/src/lib.rs` (`open_at`, ~line 100)

No capability change: in Tauri v2 the asset protocol is gated by `assetProtocol.enable` + scope, not a capability permission. The CSP already allows `img-src … asset: http://asset.localhost`. Scope stays tight — the empty static scope allows nothing; only the dir the user actually opens is granted at runtime. We never broaden to `$APP`/home.

- [ ] **Step 1: Enable the protocol in `tauri.conf.json`** — add a sibling to `csp` under `app.security`:

```json
    "security": {
      "csp": "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; font-src 'self'; img-src 'self' asset: http://asset.localhost data:; connect-src 'self' ipc: http://ipc.localhost; object-src 'none'; base-uri 'self'; frame-ancestors 'none'",
      "assetProtocol": {
        "enable": true,
        "scope": []
      }
    }
```

- [ ] **Step 2: Add the cargo feature** — `src-tauri/Cargo.toml` line 24:

```toml
tauri = { version = "2.11.2", features = ["protocol-asset"] }
```

- [ ] **Step 3: Grant the scope at open time** — in `src-tauri/src/lib.rs`, inside `open_at`, after `persist_path` (the existing best-effort block) and before `Ok(())`:

```rust
    // Scope the asset protocol to exactly this cairn dir so local note images
    // (asset://) can load while no other directory is exposed (S4). Best-effort:
    // a failure only means images won't render, never that opening fails.
    if let Err(e) = app.asset_protocol_scope().allow_directory(dir, true) {
        eprintln!("cairn: failed to scope asset protocol to {dir:?}: {e}");
    }
```

`open_at` is the single chokepoint for both the launch-restore (`setup`) and `pick_and_open_cairn` paths, so both grant the scope. `Manager` (providing `asset_protocol_scope`) is already imported.

- [ ] **Step 4: Verify the Rust build + tests** —

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: PASS, including `open_at_sets_state_and_path` (exercises the new scope grant against the mock app's managed `Scopes`).

Run: `cargo build --manifest-path src-tauri/Cargo.toml`
Expected: builds clean.

If `open_at_sets_state_and_path` panics on `state::<Scopes>()` (it should NOT — `Scopes` is managed in the standard mock build), fall back to moving the grant out of `open_at` into the two callers (`setup` closure + `pick_and_open_cairn`), keeping `open_at` pure. Record the observed outcome.

- [ ] **Step 5: Validate the config parses** —

Run: `cargo build --manifest-path src-tauri/Cargo.toml` already validates `tauri.conf.json` via `generate_context!`. Also confirm JSON is well-formed: `python3 -m json.tool src-tauri/tauri.conf.json >/dev/null && echo OK`.

- [ ] **Step 6: Commit** — `feat(tauri): enable the asset protocol scoped to the open cairn dir (S4)`.

---

## Task 5 (DX3): Contract drift CI guard

The vendored contract under `web/src/contract` is the **prettier-formatted** form of ts-rs output, but `sync-contract.sh` currently emits raw ts-rs output — so re-running it produces a (formatting-only) diff. Fix the script to format its output (making it reproduce the committed form), then add a check script + CI job that re-syncs against the pinned engine commit and fails on any diff.

**Files:**
- Modify: `scripts/sync-contract.sh`
- Create: `scripts/check-contract-drift.sh`
- Modify: `.github/workflows/ci.yml`

- [ ] **Step 1: Make `sync-contract.sh` format its output** — after the `cat > "$DEST/source.ts"` heredoc and before the final `echo`, add:

```bash
# Normalize to the repo's prettier style: the vendored copy is committed
# formatted, but ts-rs emits unformatted output. Without this the copy would
# "drift" from itself on every sync (and the DX3 drift check would never pass).
( cd web && pnpm exec prettier --log-level warn --write "src/contract/**/*.ts" )
```

- [ ] **Step 2: Verify the script now round-trips clean against the pinned commit** —

Run (from repo root):
```bash
COMMIT="$(grep -oE '[0-9a-f]{40}' web/src/contract/source.ts | head -1)"
TMP="$(mktemp -d)"; git clone --quiet https://github.com/tau-rs/cairn.git "$TMP/cairn"
git -C "$TMP/cairn" checkout --quiet "$COMMIT"
bash scripts/sync-contract.sh "$TMP/cairn"
git diff --quiet -- web/src/contract && echo "CLEAN" || { echo "DRIFT"; git --no-pager diff --stat -- web/src/contract; }
git checkout -- web/src/contract; rm -rf "$TMP"
```
Expected: `CLEAN`. (A local engine checkout at `/Users/titouanlebocq/code/cairn` may be substituted for the clone URL during dev.)

- [ ] **Step 3: Create `scripts/check-contract-drift.sh`**:

```bash
#!/usr/bin/env bash
# DX3: verify the vendored TS contract (web/src/contract) is byte-identical to
# what sync-contract.sh regenerates from the engine commit recorded in
# source.ts. Catches hand-edits / partial syncs that S5's runtime validator
# would otherwise only surface at runtime. Run on a clean checkout (CI).
set -euo pipefail

ENGINE_REPO="${ENGINE_REPO:-https://github.com/tau-rs/cairn.git}"
COMMIT="$(grep -oE '[0-9a-f]{40}' web/src/contract/source.ts | head -1)"
[ -n "$COMMIT" ] || { echo "could not read CONTRACT_SOURCE_COMMIT from web/src/contract/source.ts"; exit 1; }

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
git clone --quiet "$ENGINE_REPO" "$TMP/cairn"
git -C "$TMP/cairn" checkout --quiet "$COMMIT"

scripts/sync-contract.sh "$TMP/cairn"

if ! git diff --quiet -- web/src/contract; then
  echo "::error::vendored contract drifted from engine @ $COMMIT — re-run scripts/sync-contract.sh and commit"
  git --no-pager diff --stat -- web/src/contract
  exit 1
fi
echo "contract in sync with engine @ $COMMIT"
```

Then `chmod +x scripts/check-contract-drift.sh`.

- [ ] **Step 4: Demonstrate the check fails on synthetic drift and passes clean** —

Inject drift, expect non-zero exit:
```bash
printf '\n// synthetic drift\n' >> web/src/contract/Event.ts
ENGINE_REPO=/Users/titouanlebocq/code/cairn bash scripts/check-contract-drift.sh; echo "exit=$?"
git checkout -- web/src/contract
```
Expected: prints the `::error::` line + a diff stat, `exit=1`.

Clean run, expect zero exit:
```bash
ENGINE_REPO=/Users/titouanlebocq/code/cairn bash scripts/check-contract-drift.sh; echo "exit=$?"
git checkout -- web/src/contract
```
Expected: `contract in sync with engine @ <commit>`, `exit=0`.

Capture both outputs for verification-before-completion.

- [ ] **Step 5: Add the CI job** — in `.github/workflows/ci.yml`:

(a) Add `'scripts/**'` to the `web` paths-filter so the job re-runs when the scripts change:

```yaml
            web:
              - 'web/**'
              - 'scripts/**'
              - '.github/workflows/ci.yml'
              - 'justfile'
```

(b) Add a `contract-drift` job after the `web` job (uses the same SHA-pinned action refs already in the file):

```yaml
  contract-drift:
    name: contract-drift
    needs: changes
    if: ${{ github.event_name != 'pull_request' || needs.changes.outputs.web == 'true' }}
    runs-on: ubuntu-latest
    timeout-minutes: 10
    steps:
      - uses: actions/checkout@df4cb1c069e1874edd31b4311f1884172cec0e10 # v6.0.3
      - uses: pnpm/action-setup@0e279bb959325dab635dd2c09392533439d90093 # v6.0.8
        with:
          version: 10.14.0
      - uses: actions/setup-node@48b55a011bda9f5d6aeb4c2d9c7362e8dae4041e # v6.4.0
        with:
          node-version: 20
          cache: pnpm
          cache-dependency-path: web/pnpm-lock.yaml
      # prettier (run by sync-contract.sh) comes from the web workspace deps.
      - run: pnpm install --frozen-lockfile
        working-directory: web
      # DX3: re-sync the contract from the pinned engine commit and fail on a diff.
      - run: bash scripts/check-contract-drift.sh
```

(c) Add `contract-drift` to `ci-summary`'s `needs`:

```yaml
    needs: [changes, web, e2e, tauri, contract-drift]
```

- [ ] **Step 6: Validate the workflow YAML** —

Run: `python3 -c "import yaml,sys; yaml.safe_load(open('.github/workflows/ci.yml')); print('YAML OK')"`
Expected: `YAML OK`.

- [ ] **Step 7: Commit** — `ci(contract): add a contract-drift guard re-syncing against the pinned engine commit (DX3)`.

---

## Final verification (verification-before-completion)

- [ ] `cd web && pnpm lint && pnpm format:check && pnpm typecheck && pnpm test && pnpm build` → all green (run the FULL gate; `format:check` is easy to miss).
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml && cargo build --manifest-path src-tauri/Cargo.toml` → green.
- [ ] `cargo fmt --manifest-path src-tauri/Cargo.toml --check && cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` → clean.
- [ ] Re-capture the DX3 drift-check pass/fail outputs (Task 5 Step 4).
- [ ] `requesting-code-review` on the diff; check for scope creep (no S3 normalization beyond the existing `confineToRoot` guard; thin S5 validator; S6 only `keybindingPersistence`).
- [ ] Commit, push, `gh pr create -R tau-rs/cairn-web-ui --base main` citing S4, U5, S5, DX3, S6. **STOP — no merge.**

## Notes / scope guards
- S3 (path-traversal normalization) is Medium and out of scope. Enabling S4 does NOT make it live unguarded: `TauriHost.assetUrl` already runs `confineToRoot` (rejects absolute paths + `..` escapes) before `convertFileSrc`, and the runtime asset scope is restricted to the open cairn dir. Note S3 as the real fix in the PR body; add no new normalization here.
- Asset scope is process-scoped and additive across opens (each opened cairn dir is granted). It is never broadened to globs/`$APP`/home, satisfying the "tight scope" constraint.
