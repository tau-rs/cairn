# Image Handling Security Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stop note images from being privacy/security hazards: don't auto-load remote/`data:` images (gate behind opt-in, default off), and confine local image paths to the vault root so a crafted `../` path can't escape.

**Architecture:** The image resolver becomes the single trust boundary. `makeImageResolver` returns a discriminated `ResolvedImage` (`ready` vs `blocked`) instead of a bare URL string; remote/`data:` srcs are `blocked` unless the global "load remote images" setting is on. `ImageWidget` renders a click-to-load placeholder for `blocked` images (per-image opt-in) and a real `<img>` for `ready` ones. Separately, a pure `confineToRoot` helper normalizes and validates local relative paths before `TauriHost.assetUrl` ever calls `convertFileSrc`, rejecting absolute paths and `..` escapes.

**Tech Stack:** TypeScript, React, Zustand store, CodeMirror 6 widgets, Vitest.

---

### Task 1: Path confinement helper (`confineToRoot`)

**Files:**
- Create: `web/src/client/vaultPath.ts`
- Test: `web/src/client/vaultPath.test.ts`

- [ ] **Step 1: Write the failing test**

```ts
import { describe, it, expect } from "vitest";
import { confineToRoot } from "./vaultPath";

describe("confineToRoot", () => {
  it("joins a simple relative path onto the root", () => {
    expect(confineToRoot("/vault", "img/logo.png")).toBe("/vault/img/logo.png");
  });
  it("tolerates a trailing slash on the root", () => {
    expect(confineToRoot("/vault/", "img/logo.png")).toBe("/vault/img/logo.png");
  });
  it("normalizes interior `.` and `..` that stay inside the root", () => {
    expect(confineToRoot("/vault", "a/../img/./logo.png")).toBe("/vault/img/logo.png");
  });
  it("rejects a `..` sequence that escapes the root", () => {
    expect(confineToRoot("/vault", "../../etc/passwd")).toBeNull();
  });
  it("rejects an absolute POSIX path", () => {
    expect(confineToRoot("/vault", "/etc/passwd")).toBeNull();
  });
  it("rejects a Windows drive-absolute path", () => {
    expect(confineToRoot("/vault", "C:\\Windows\\system32")).toBeNull();
  });
  it("rejects a UNC path", () => {
    expect(confineToRoot("/vault", "\\\\server\\share")).toBeNull();
  });
  it("rejects a path that resolves to the root itself", () => {
    expect(confineToRoot("/vault", "a/..")).toBeNull();
  });
  it("treats backslashes as separators when confining", () => {
    expect(confineToRoot("/vault", "img\\..\\..\\secret")).toBeNull();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd web && npx vitest run src/client/vaultPath.test.ts`
Expected: FAIL — `confineToRoot` not exported / module missing.

- [ ] **Step 3: Write minimal implementation**

```ts
/** Join `relPath` onto `root`, confining the result to the root directory.
 *  Rejects absolute paths (POSIX, Windows drive, UNC) and any `..` sequence
 *  that would escape `root`. Returns the confined absolute path, or `null`
 *  if the path is absolute or escapes. Backslashes are treated as separators.
 *
 *  This is the guard that must run BEFORE a local image path reaches the
 *  Tauri asset protocol, where an unconfined path is local-file disclosure. */
export function confineToRoot(root: string, relPath: string): string | null {
  // Absolute: leading slash/backslash, `X:\`/`X:/` drive, or `\\` UNC.
  if (/^([/\\]|[a-zA-Z]:[/\\]|\\\\)/.test(relPath)) return null;
  const stack: string[] = [];
  for (const seg of relPath.split(/[/\\]+/)) {
    if (seg === "" || seg === ".") continue;
    if (seg === "..") {
      if (stack.length === 0) return null; // would climb above the root
      stack.pop();
      continue;
    }
    stack.push(seg);
  }
  if (stack.length === 0) return null; // resolves to the root itself
  const base = root.replace(/[/\\]+$/, "");
  return `${base}/${stack.join("/")}`;
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd web && npx vitest run src/client/vaultPath.test.ts`
Expected: PASS (9 tests).

- [ ] **Step 5: Commit**

```bash
git add web/src/client/vaultPath.ts web/src/client/vaultPath.test.ts
git commit -m "feat(images): add confineToRoot vault path guard"
```

---

### Task 2: Wire confinement into `TauriHost.assetUrl`

**Files:**
- Modify: `web/src/client/tauri.ts:53-58`
- Test: `web/src/client/tauri.test.ts` (add cases)

- [ ] **Step 1: Add failing tests**

Append inside the `describe("TauriHost", ...)` block:

```ts
  it("assetUrl refuses a path that escapes the vault root", async () => {
    invoke.mockResolvedValueOnce("/tmp/c");
    const h = new TauriHost();
    await h.openCairn();
    expect(h.assetUrl("../../etc/passwd")).toBe("");
  });

  it("assetUrl refuses an absolute path", async () => {
    invoke.mockResolvedValueOnce("/tmp/c");
    const h = new TauriHost();
    await h.openCairn();
    expect(h.assetUrl("/etc/passwd")).toBe("");
  });

  it("assetUrl normalizes a safe interior `..` before resolving", async () => {
    invoke.mockResolvedValueOnce("/tmp/c");
    const h = new TauriHost();
    await h.openCairn();
    expect(h.assetUrl("a/../img/x.png")).toBe("asset:///tmp/c/img/x.png");
  });
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd web && npx vitest run src/client/tauri.test.ts`
Expected: FAIL — escaping paths currently produce an `asset://` URL, not `""`.

- [ ] **Step 3: Implement**

Add import at top of `web/src/client/tauri.ts`:

```ts
import { confineToRoot } from "./vaultPath";
```

Replace the `assetUrl` method body:

```ts
  assetUrl(relPath: string): string {
    if (/^(https?:|data:)/i.test(relPath)) return relPath;
    if (!this.root) return relPath;
    const full = confineToRoot(this.root, relPath);
    if (full === null) return ""; // path escapes the vault — refuse to resolve
    return convertFileSrc(full);
  }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd web && npx vitest run src/client/tauri.test.ts`
Expected: PASS (existing + 3 new).

- [ ] **Step 5: Commit**

```bash
git add web/src/client/tauri.ts web/src/client/tauri.test.ts
git commit -m "fix(images): confine local asset paths to the vault root"
```

---

### Task 3: Resolver returns `ResolvedImage`, gating remote/`data:`

**Files:**
- Modify: `web/src/components/editor/imageResolver.ts`
- Test: `web/src/components/editor/imageResolver.test.ts` (rewrite assertions)

- [ ] **Step 1: Rewrite tests to the new contract**

```ts
import { describe, it, expect, vi } from "vitest";
import { makeImageResolver } from "./imageResolver";

describe("makeImageResolver", () => {
  it("blocks http(s) URLs by default (no opt-in)", () => {
    const assetUrl = vi.fn();
    const r = makeImageResolver(assetUrl);
    expect(r("https://x/y.png")).toEqual({ kind: "blocked", src: "https://x/y.png" });
    expect(assetUrl).not.toHaveBeenCalled();
  });
  it("blocks data URLs by default", () => {
    const r = makeImageResolver(vi.fn());
    expect(r("data:image/png;base64,AAAA")).toEqual({
      kind: "blocked",
      src: "data:image/png;base64,AAAA",
    });
  });
  it("passes remote URLs through as ready when loadRemote is on", () => {
    const r = makeImageResolver(vi.fn(), { loadRemote: true });
    expect(r("https://x/y.png")).toEqual({ kind: "ready", url: "https://x/y.png" });
  });
  it("resolves local relative paths via assetUrl regardless of loadRemote", () => {
    const assetUrl = vi.fn().mockReturnValue("asset://img/logo.png");
    const r = makeImageResolver(assetUrl);
    expect(r("img/logo.png")).toEqual({ kind: "ready", url: "asset://img/logo.png" });
    expect(assetUrl).toHaveBeenCalledWith("img/logo.png");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd web && npx vitest run src/components/editor/imageResolver.test.ts`
Expected: FAIL — resolver returns strings, not `ResolvedImage` objects.

- [ ] **Step 3: Implement**

Replace `web/src/components/editor/imageResolver.ts`:

```ts
export type AssetUrl = (relPath: string) => string;

/** A resolved image: either safe to load now, or blocked pending opt-in. */
export type ResolvedImage =
  | { kind: "ready"; url: string }
  | { kind: "blocked"; src: string };

export type ImageResolver = (src: string) => ResolvedImage;

/** Map an image markdown `src` to a `ResolvedImage`. Remote (`http(s):`) and
 *  `data:` srcs are `blocked` unless `loadRemote` is set — they are tracking /
 *  exfil beacons that must not fire on note-open without explicit opt-in. Local
 *  relative paths are resolved through the host's `assetUrl` (itself confined
 *  to the vault root) and are always `ready`. */
export function makeImageResolver(
  assetUrl: AssetUrl,
  opts?: { loadRemote?: boolean },
): ImageResolver {
  const loadRemote = opts?.loadRemote ?? false;
  return (src: string): ResolvedImage => {
    if (/^(https?:|data:)/i.test(src)) {
      return loadRemote ? { kind: "ready", url: src } : { kind: "blocked", src };
    }
    return { kind: "ready", url: assetUrl(src) };
  };
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd web && npx vitest run src/components/editor/imageResolver.test.ts`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add web/src/components/editor/imageResolver.ts web/src/components/editor/imageResolver.test.ts
git commit -m "feat(images): resolver gates remote/data images behind opt-in"
```

---

### Task 4: `ImageWidget` renders ready images and blocked placeholders

**Files:**
- Modify: `web/src/components/editor/widgets/imageWidget.ts`
- Modify: `web/src/components/editor/livePreview.ts:21,317-323`
- Modify: `web/src/components/editor/livePreview.test.ts:11` (stub returns `ResolvedImage`)
- Modify: `web/src/components/editor/livePreview.css` (placeholder styling)

- [ ] **Step 1: Update the livePreview test stub to the new contract**

In `web/src/components/editor/livePreview.test.ts`, change the `resolveImage` stub:

```ts
  resolveImage: (src: string) => ({ kind: "ready" as const, url: "resolved:" + src }),
```

- [ ] **Step 2: Run the suite to confirm the type/contract break**

Run: `cd web && npx vitest run src/components/editor/livePreview.test.ts`
Expected: PASS for decoration/atomic tests (they don't call `toDOM`), but `tsc` would flag the widget constructor mismatch — proceed to implement.

- [ ] **Step 3: Implement `ImageWidget`**

Replace `web/src/components/editor/widgets/imageWidget.ts`:

```ts
import { WidgetType } from "@codemirror/view";
import type { ResolvedImage } from "../imageResolver";

export class ImageWidget extends WidgetType {
  constructor(
    readonly image: ResolvedImage,
    readonly alt: string,
    readonly block: boolean,
    readonly from: number,
    readonly onEdit: (from: number) => void,
  ) {
    super();
  }
  eq(other: ImageWidget): boolean {
    return (
      sameImage(other.image, this.image) &&
      other.alt === this.alt &&
      other.block === this.block &&
      other.from === this.from
    );
  }
  toDOM(): HTMLElement {
    if (this.image.kind === "blocked") return this.placeholder(this.image.src);
    return this.imageEl(this.image.url);
  }
  private imageEl(url: string): HTMLImageElement {
    const img = document.createElement("img");
    img.className = this.block ? "cm-lp-img block" : "cm-lp-img";
    img.src = url;
    img.alt = this.alt;
    img.addEventListener("mousedown", (e) => {
      e.preventDefault();
      this.onEdit(this.from);
    });
    return img;
  }
  /** Click-to-load placeholder for a remote/`data:` image that has not been
   *  opted into. Loading is per-image and ephemeral (does not touch settings):
   *  clicking "Load" swaps in the real <img> in place. */
  private placeholder(src: string): HTMLElement {
    const box = document.createElement("span");
    box.className = this.block ? "cm-lp-img-blocked block" : "cm-lp-img-blocked";
    const label = document.createElement("span");
    label.className = "cm-lp-img-blocked-label";
    label.textContent = "Remote image blocked";
    const load = document.createElement("button");
    load.type = "button";
    load.className = "cm-lp-img-blocked-load";
    load.textContent = "Load";
    load.addEventListener("mousedown", (e) => {
      e.preventDefault();
      e.stopPropagation();
      box.replaceWith(this.imageEl(src));
    });
    // Clicking the placeholder body (not the button) reveals raw markdown,
    // matching a rendered image's edit affordance.
    box.addEventListener("mousedown", (e) => {
      e.preventDefault();
      this.onEdit(this.from);
    });
    box.append(label, load);
    return box;
  }
  ignoreEvent(): boolean {
    return false;
  }
}

function sameImage(a: ResolvedImage, b: ResolvedImage): boolean {
  if (a.kind !== b.kind) return false;
  return a.kind === "blocked"
    ? a.src === (b as { src: string }).src
    : a.url === (b as { url: string }).url;
}
```

- [ ] **Step 4: Update `livePreview.ts` to pass `ResolvedImage`**

Change the `resolveImage` field type (line 21):

```ts
  resolveImage: (src: string) => import("./imageResolver").ResolvedImage;
```

Better — add a top import and use the named type. Add to the import block near line 13:

```ts
import { ImageWidget } from "./widgets/imageWidget";
import type { ResolvedImage } from "./imageResolver";
```

and set the field to:

```ts
  resolveImage: (src: string) => ResolvedImage;
```

Then update the image loop (around line 317):

```ts
    const image = opts.resolveImage(im[2]);
    const line = state.doc.lineAt(from);
    const block = line.text.trim() === im[0];
    decos.push(
      Decoration.replace({
        widget: new ImageWidget(image, alt, block, from, opts.onEditImage),
      }).range(from, to),
    );
```

- [ ] **Step 5: Add placeholder styling**

Append to `web/src/components/editor/livePreview.css`:

```css
.cm-lp-img-blocked {
  display: inline-flex;
  align-items: center;
  gap: 0.5rem;
  padding: 0.25rem 0.5rem;
  border: 1px dashed var(--border, #555);
  border-radius: 4px;
  color: var(--muted, #999);
  font-size: 0.85em;
  cursor: pointer;
}
.cm-lp-img-blocked.block {
  display: flex;
}
.cm-lp-img-blocked-load {
  border: 1px solid var(--border, #555);
  border-radius: 3px;
  padding: 0 0.4rem;
  background: transparent;
  color: inherit;
  cursor: pointer;
  font: inherit;
}
.cm-lp-img-blocked-load:hover {
  color: var(--text, #ddd);
}
```

- [ ] **Step 6: Run the editor test suites**

Run: `cd web && npx vitest run src/components/editor`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add web/src/components/editor/widgets/imageWidget.ts web/src/components/editor/livePreview.ts web/src/components/editor/livePreview.test.ts web/src/components/editor/livePreview.css
git commit -m "feat(images): blocked-image placeholder with per-image click-to-load"
```

---

### Task 5: `loadRemoteImages` setting + Editor wiring + Settings UI

**Files:**
- Modify: `web/src/store/store.ts:20-36` (Settings type + defaults)
- Modify: `web/src/components/Editor.tsx:22,36-39` (prop + resolver opts)
- Modify: `web/src/app/App.tsx:82,306` (read setting, pass to Editor)
- Modify: `web/src/components/Settings.tsx` (checkbox)

- [ ] **Step 1: Add a failing store test**

In `web/src/store/store.test.ts`, add (near other settings tests):

```ts
  it("defaults loadRemoteImages to off", () => {
    const store = makeStore(makeDeps());
    expect(store.getState().settings.loadRemoteImages).toBe(false);
  });
```

(If the existing test setup uses different helpers, mirror the file's existing pattern for constructing the store; assert `settings.loadRemoteImages === false`.)

- [ ] **Step 2: Run test to verify it fails**

Run: `cd web && npx vitest run src/store/store.test.ts`
Expected: FAIL — `loadRemoteImages` undefined.

- [ ] **Step 3: Add the setting**

In `web/src/store/store.ts`, extend `Settings` (after `editorMode`):

```ts
  editorMode: "livepreview" | "source";
  loadRemoteImages: boolean;
```

and `DEFAULT_SETTINGS`:

```ts
  editorMode: "livepreview",
  loadRemoteImages: false,
```

- [ ] **Step 4: Run store test to verify it passes**

Run: `cd web && npx vitest run src/store/store.test.ts`
Expected: PASS.

- [ ] **Step 5: Thread the setting into the Editor**

In `web/src/components/Editor.tsx`, add to props:

```ts
  assetUrl: (relPath: string) => string;
  loadRemoteImages: boolean;
```

Update the resolver memo:

```ts
  const resolveImage = useMemo(
    () => makeImageResolver(props.assetUrl, { loadRemote: props.loadRemoteImages }),
    [props.assetUrl, props.loadRemoteImages],
  );
```

In `web/src/app/App.tsx`, near line 82:

```ts
  const loadRemoteImages = useCairn((s) => s.settings.loadRemoteImages);
```

and pass it to `<Editor>` (after `assetUrl={actions.assetUrl}`):

```tsx
                      assetUrl={actions.assetUrl}
                      loadRemoteImages={loadRemoteImages}
```

- [ ] **Step 6: Add the Settings checkbox**

In `web/src/components/Settings.tsx`, add a section after the auto-commit block (before the closing `</div>`):

```tsx
      <span className="mb-1 mt-2">
        <SectionLabel>Privacy</SectionLabel>
      </span>
      <label className="flex items-center gap-2 text-muted">
        <input
          type="checkbox"
          checked={s.loadRemoteImages}
          onChange={(e) => props.onChange({ loadRemoteImages: e.target.checked })}
        />
        Load remote images automatically
      </label>
```

- [ ] **Step 7: Run the full web suite + typecheck**

Run: `cd web && npx vitest run && npx tsc --noEmit`
Expected: PASS, no type errors.

- [ ] **Step 8: Commit**

```bash
git add web/src/store/store.ts web/src/store/store.test.ts web/src/components/Editor.tsx web/src/app/App.tsx web/src/components/Settings.tsx
git commit -m "feat(images): loadRemoteImages setting wired through editor + Settings UI"
```

---

### Task 6: Verify, lint, and open the PR

- [ ] **Step 1: Full verification**

Run: `cd web && npx vitest run && npx tsc --noEmit && npm run lint`
Expected: all green. If `npm run lint` differs, use the repo's configured lint script.

- [ ] **Step 2: Manual smoke (best effort)**

Open the app, a note with a local image still renders; a note with `![](https://…)` shows the blocked placeholder; clicking "Load" loads that one image; toggling the Settings "Load remote images" checkbox makes remote images render on note re-open.

- [ ] **Step 3: requesting-code-review, then PR**

Use superpowers:requesting-code-review, address findings, then:

```bash
git push -u origin <branch>
gh pr create -R tau-rs/cairn-web-ui --base main --title "..." --body "..."
```

Cite audit findings S2 and S3. STOP — no merge.

---

## Notes / Coordination

- **CSP (`08-cairn-ui-csp`):** `img-src` should be tightened to match (no broad remote allowance needed now that remote images are opt-in). Do not edit CSP here to avoid conflicting with that branch; call it out in the PR.
- **S4 (asset protocol not enabled):** out of scope; `confineToRoot` makes S3 safe *before* the protocol is ever turned on.
