# Presence Cluster + Human Versions (UI track) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the git-flavored Commit UI with automatic engine-driven "Versions" (UI = consumer + hint-sender) and unify the two corner live/sync cards into one calm presence cluster, keeping the per-tab ● dirty dot.

**Architecture:** The web UI stops deciding *when* to commit (frontend idle/interval timers die); it only sends "seal now" hints (commit with no message) and consumes enriched history. Because the engine C0 contract has not landed, the new commands/fields are host-copy types (`contractExt.ts`) mirrored in `MockClient` — the UI builds fully against the mock and swaps to `DaemonClient` at Phase-3 integration. Presence stays receive-only/anonymous but the new `PresenceCluster` accepts a `peers[]` roster so engine identity slots in later with no rewrite.

**Tech Stack:** React 18 + zustand vanilla store + Tailwind + vitest/@testing-library (existing stack; no new deps).

**Spec:** `docs/superpowers/specs/2026-08-21-presence-and-versions-redesign-design.md`

## Global Constraints

- **Never hand-edit `web/src/contract/`** — it is vendored ts-rs output, drift-checked in CI. New shapes go in `web/src/client/contractExt.ts` with a `TODO(contract-sync)` marker (Tier-3 host-copy precedent).
- **Copy rules:** "Saved" is reserved for the disk-flush axis; the git layer is always "Versions" (never "saved", never "commit" in user-facing copy). Offline copy: "Offline — changes saved locally". Conflict dialog title: "This note also changed on another device"; buttons "Keep my version" / "See their version". No danger-red primary anywhere in the conflict path.
- **Keep the ● dirty dot** (`web/src/components/tabs/TabStrip.tsx:91`) and the Layer-1 write_note autosave debounce (`store.ts` editBuffer, ~L691–697) untouched.
- **Conflict resolution is never one-click destructive**: "See their version" is read-only; overwriting the buffer requires an explicit second action inside that view.
- All commands run from `web/`: `pnpm test -- --run`, gate = `pnpm lint && pnpm format:check && pnpm test -- --run && pnpm build` (prettier check is part of done — CI catches it, eslint won't).
- Conventional commits, imperative, scoped. The branch `presence-and-versions` is claimed and pushed (flag commit `a254cd8`); commit to it directly.
- `sealNow` failures never toast (hints are background garnish); `nameVersion` failures do toast (explicit user act).

## File structure (locked)

```
web/src/client/contractExt.ts            NEW  host-copy C0 types + asCommand cast seam
web/src/client/mock.ts                   MOD  seal-without-message, name_version, enriched rows
web/src/store/store.ts                   MOD  sealNow/nameVersion/lastVersion; timers+settings removed
web/src/store/collabSlice.ts             MOD  peers[]/theirs, keepMine/viewTheirs/exitTheirs
web/src/components/StatusBar.tsx         NEW  bottom strip (U2)
web/src/components/collab/PresenceCluster.tsx  NEW  top-right cluster (U3)
web/src/components/collab/ConflictDialog.tsx   NEW  calm conflict choice (U3)
web/src/components/collab/TheirVersionView.tsx NEW  read-only "their version" diff (U3)
web/src/components/history/DiffTable.tsx       NEW  extracted from RevisionView, shared
web/src/components/history/groupRevisions.ts   NEW  day/session grouping (pure) (U4)
web/src/components/history/versionSummary.ts   NEW  word-delta from fields or message (U4)
web/src/components/history/NameVersionDialog.tsx NEW (U4)
web/src/components/history/{HistoryList,HistoryPane}.tsx MOD  Versions browser (U4)
web/src/app/useSealHints.ts              NEW  note-switch/blur seal hints (U5)
DELETED: CommitBar.tsx, CommitDialog.tsx, CollabPresencePill.tsx,
         LiveUpdatesBanner.tsx, CollabReloadDialog.tsx (+ their tests)
```

---

### Task 1: C0 host-copy contract seam (`contractExt.ts`)

**Files:**
- Create: `web/src/client/contractExt.ts`
- Test: `web/src/client/contractExt.test.ts`

**Interfaces:**
- Produces: `SealCommand`, `NameVersionCommand`, `CommandEx`, `asCommand(c: CommandEx): Command`, `RevisionEx` — used by every later task.

- [ ] **Step 1: Write the failing test**

```ts
// web/src/client/contractExt.test.ts
import { describe, it, expect } from "vitest";
import { asCommand, type CommandEx, type RevisionEx } from "./contractExt";
import type { Revision } from "../contract/Revision";

describe("contractExt", () => {
  it("asCommand passes C0 commands through the vendored Command seam", () => {
    const seal: CommandEx = { type: "commit" };
    const name: CommandEx = { type: "name_version", commit: "c0001", name: "Draft 1" };
    expect(asCommand(seal)).toEqual({ type: "commit" });
    expect(asCommand(name)).toEqual({ type: "name_version", commit: "c0001", name: "Draft 1" });
  });

  it("RevisionEx is assignable from a plain vendored Revision", () => {
    const plain: Revision = { id: "c1", message: "m", timestamp_secs: 1, author: "a" };
    const ex: RevisionEx = plain; // pre-C0 daemons omit every new field
    expect(ex.is_named).toBeUndefined();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test -- --run src/client/contractExt.test.ts`
Expected: FAIL — module `./contractExt` not found.

- [ ] **Step 3: Write the implementation**

```ts
// web/src/client/contractExt.ts
import type { Command } from "../contract/Command";
import type { Revision } from "../contract/Revision";

// TODO(contract-sync): host-copy of the engine C0 contract for
// presence-and-versions (spec 2026-08-21). Replace with vendored ts-rs types
// once the engine lands; web/src/contract is generated and cannot be
// hand-edited (drift-checked in CI).

/** Command::Commit with `message` optional — omitted ⇒ the engine generates a
 *  deterministic message and this call means "seal the session now". */
export type SealCommand = { type: "commit"; message?: string };

/** Command::NameVersion — names (tags) an existing version. */
export type NameVersionCommand = {
  type: "name_version";
  commit: string;
  name: string;
};

export type CommandEx = Command | SealCommand | NameVersionCommand;

/** The single sanctioned cast seam for sending C0 commands through the
 *  current vendored `Command` union. Delete when the contract catches up. */
export function asCommand(c: CommandEx): Command {
  return c as Command;
}

/** Revision enriched with C0 change-summary + naming fields. All optional:
 *  pre-C0 daemons omit them and the UI must degrade gracefully. */
export type RevisionEx = Revision & {
  op?: "add" | "edit" | "rename" | "delete";
  words_added?: number;
  words_removed?: number;
  first_heading?: string | null;
  is_named?: boolean;
  name?: string | null;
};
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm test -- --run src/client/contractExt.test.ts`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add web/src/client/contractExt.ts web/src/client/contractExt.test.ts
git commit -m "feat(versions): add C0 host-copy contract seam (seal/name_version/RevisionEx)"
```

---

### Task 2: MockClient C0 stubs (seal, name_version, enriched rows)

**Files:**
- Modify: `web/src/client/mock.ts` (fields ~L101–215, `sendCommand` commit case L255–260)
- Test: `web/src/client/mock.test.ts` (create if absent; check `ls web/src/client/*.test.ts` first and append if a mock test file exists)

**Interfaces:**
- Consumes: `CommandEx`, `RevisionEx` from Task 1.
- Produces: `sendCommand({type:"commit"})` (no message) → generates message, appends a `RevisionEx` to `vaultHistory` + per-note `history` fixtures, returns/emits `committed`; skip-no-op when nothing changed. `sendCommand({type:"name_version", commit, name})` → marks matching revisions `is_named:true, name`, returns `{type:"done"}`.

- [ ] **Step 1: Write the failing tests**

```ts
// web/src/client/mock.test.ts (new describe block)
import { describe, it, expect } from "vitest";
import { MockClient } from "./mock";
import { asCommand } from "./contractExt";
import type { RevisionEx } from "./contractExt";

describe("MockClient C0 versions seam", () => {
  it("seals without a message: generates a summary message and a vault_history row", async () => {
    const client = new MockClient({ "roadmap.md": "# Roadmap\n\nalpha beta" });
    await client.sendCommand({ type: "write_note", path: "roadmap.md", contents: "# Roadmap\n\nalpha beta gamma delta" });
    const res = await client.sendCommand(asCommand({ type: "commit" }));
    expect(res).toEqual({ type: "committed", commit: "c0001" });
    const hist = await client.runQuery({ type: "vault_history", limit: 1 });
    if (hist.type !== "history") throw new Error("bad response");
    const rev = hist.revisions[0] as RevisionEx;
    expect(rev.message).toBe('Edit "roadmap" (+2/−0 words)');
    expect(rev.words_added).toBe(2);
    expect(rev.is_named).toBe(false);
  });

  it("skip-no-op: sealing with no changes adds no new version", async () => {
    const client = new MockClient({ "a.md": "hi" });
    await client.sendCommand(asCommand({ type: "commit" })); // nothing changed since construction
    const hist = await client.runQuery({ type: "vault_history", limit: null });
    if (hist.type !== "history") throw new Error("bad response");
    expect(hist.revisions).toHaveLength(0);
  });

  it("multi-note seal rolls up into one version", async () => {
    const client = new MockClient({ "a.md": "x", "b.md": "y" });
    await client.sendCommand({ type: "write_note", path: "a.md", contents: "x x" });
    await client.sendCommand({ type: "write_note", path: "b.md", contents: "y y" });
    const res = await client.sendCommand(asCommand({ type: "commit" }));
    if (res.type !== "committed") throw new Error("bad response");
    const hist = await client.runQuery({ type: "vault_history", limit: 1 });
    if (hist.type !== "history") throw new Error("bad response");
    expect(hist.revisions[0].message).toBe("Update 2 notes: a, b");
  });

  it("an explicit message is used verbatim (back-compat)", async () => {
    const client = new MockClient({ "a.md": "x" });
    await client.sendCommand({ type: "write_note", path: "a.md", contents: "x y" });
    const res = await client.sendCommand({ type: "commit", message: "snapshot" });
    expect(res).toEqual({ type: "committed", commit: "c0001" });
  });

  it("name_version marks the revision named in vault and note history", async () => {
    const client = new MockClient({ "a.md": "x" });
    await client.sendCommand({ type: "write_note", path: "a.md", contents: "x y" });
    await client.sendCommand(asCommand({ type: "commit" }));
    const res = await client.sendCommand(asCommand({ type: "name_version", commit: "c0001", name: "Draft 1" }));
    expect(res).toEqual({ type: "done" });
    const vault = await client.runQuery({ type: "vault_history", limit: 1 });
    if (vault.type !== "history") throw new Error("bad response");
    expect((vault.revisions[0] as RevisionEx).name).toBe("Draft 1");
    const note = await client.runQuery({ type: "note_history", path: "a.md" });
    if (note.type !== "history") throw new Error("bad response");
    expect((note.revisions[0] as RevisionEx).is_named).toBe(true);
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `pnpm test -- --run src/client/mock.test.ts`
Expected: FAIL — messages/rows/name_version not implemented (first test fails on `rev.message`, last throws on unknown command).

- [ ] **Step 3: Implement in `mock.ts`**

Add imports and fields:

```ts
import type { CommandEx, RevisionEx } from "./contractExt";
```

In the class, next to `commitSeq` (L106): add

```ts
  // C0 seal baseline: the note contents as of the last sealed version.
  private sealedSnapshot: Map<string, string>;
```

In the constructor (after `this.notes = new Map(...)`): `this.sealedSnapshot = new Map(this.notes);`

Change `sendCommand`'s first line so the switch sees the extended union:

```ts
  async sendCommand(c: Command): Promise<CommandResponse> {
    const cx = c as CommandEx;
    switch (cx.type) {
```

(rename `c.` → `cx.` inside the switch cases — mechanical.)

Add a private helper + module-level message builder (near the existing `stem` helper):

```ts
type SealChange = { op: "add" | "edit" | "delete"; path: string; added: number; removed: number };

function countWords(s: string): number {
  return (s.match(/\S+/g) ?? []).length;
}

/** Reference implementation of the engine's deterministic message template
 *  (spec "Diff-derived version labels"). Mock-only: the real one is engine E2. */
function sealMessage(changes: SealChange[]): string {
  if (changes.length > 1)
    return `Update ${changes.length} notes: ${changes
      .slice(0, 3)
      .map((x) => stem(x.path))
      .join(", ")}`;
  const [ch] = changes;
  const title = stem(ch.path);
  if (ch.op === "add") return `Add note "${title}"`;
  if (ch.op === "delete") return `Delete note "${title}"`;
  return `Edit "${title}" (+${ch.added}/−${ch.removed} words)`;
}
```

Private method on the class:

```ts
  /** Diff notes vs the last sealed snapshot; advances the snapshot. */
  private sealChanges(): SealChange[] {
    const changes: SealChange[] = [];
    for (const [path, now] of this.notes) {
      const then = this.sealedSnapshot.get(path);
      if (then === now) continue;
      const d = countWords(now) - countWords(then ?? "");
      changes.push({
        op: then === undefined ? "add" : "edit",
        path,
        added: Math.max(d, 0),
        removed: Math.max(-d, 0),
      });
    }
    for (const [path, then] of this.sealedSnapshot) {
      if (!this.notes.has(path))
        changes.push({ op: "delete", path, added: 0, removed: countWords(then) });
    }
    this.sealedSnapshot = new Map(this.notes);
    return changes;
  }
```

Replace the `case "commit":` body (L255–260):

```ts
      case "commit": {
        // C0: message omitted ⇒ engine-side policy generates it (mocked here).
        let message = cx.message;
        let changes: SealChange[] = [];
        if (message === undefined) {
          changes = this.sealChanges();
          if (changes.length === 0) {
            // Skip-no-op: never create an empty version; report the last seal.
            return {
              type: "committed",
              commit: `c${String(this.commitSeq).padStart(4, "0")}`,
            };
          }
          message = sealMessage(changes);
        } else {
          this.sealChanges(); // explicit message still advances the baseline
        }
        this.commitSeq += 1;
        const commit = `c${String(this.commitSeq).padStart(4, "0")}`;
        const rev: RevisionEx = {
          id: commit,
          message,
          author: "mock",
          timestamp_secs: Math.floor(Date.now() / 1000),
          op: changes[0]?.op,
          words_added: changes.reduce((n, s) => n + s.added, 0),
          words_removed: changes.reduce((n, s) => n + s.removed, 0),
          is_named: false,
          name: null,
        };
        this.vaultHistory.unshift(rev);
        for (const s of changes) {
          const fix = this.history.get(s.path) ?? { revisions: [], contents: {} };
          fix.revisions.unshift(rev);
          const now = this.notes.get(s.path);
          if (now !== undefined) fix.contents[commit] = now;
          this.history.set(s.path, fix);
        }
        this.emit({ type: "committed", commit });
        return { type: "committed", commit };
      }
      case "name_version": {
        const mark = (r: Revision) => {
          const ex = r as RevisionEx;
          if (ex.id === cx.commit) {
            ex.is_named = true;
            ex.name = cx.name;
          }
        };
        this.vaultHistory.forEach(mark);
        for (const fix of this.history.values()) fix.revisions.forEach(mark);
        return { type: "done" };
      }
```

Note: `vaultHistory` field type stays `Revision[]` (RevisionEx is assignable). If TS complains about the switch exhaustiveness elsewhere, the `default` case (if any) keeps working because `CommandEx` is a superset.

- [ ] **Step 4: Run tests to verify they pass**

Run: `pnpm test -- --run src/client/mock.test.ts`
Expected: PASS (5 new tests). Also run `pnpm test -- --run` — no regressions (existing tests call `{type:"commit", message}` which is still honored).

- [ ] **Step 5: Commit**

```bash
git add web/src/client/mock.ts web/src/client/mock.test.ts
git commit -m "feat(versions): MockClient C0 stubs — seal-without-message, name_version, enriched rows"
```

---

### Task 3: Strip the Commit UI (U1-UI + U6)

**Files:**
- Delete: `web/src/components/CommitBar.tsx`, `CommitBar.test.tsx`, `CommitDialog.tsx`, `CommitDialog.test.tsx`
- Modify: `web/src/components/TopBar.tsx`, `DialogHost.tsx`, `Settings.tsx`, `Settings.test.tsx`, `web/src/components/shells/MoreMenu.tsx`, `MoreMenu.test.tsx`, `web/src/components/shortcuts/commands.ts`, `useGlobalKeys.ts`, `web/src/app/useCommands.ts`, `useCommands.test.tsx`

**Interfaces:**
- Consumes: nothing new. The store's `commitManual` and `ui.commitOpen` still exist after this task (removed in Task 4) — this task only removes every component/registry reference so Task 4's store surgery can't break the build.

- [ ] **Step 1: Delete the four component files**

```bash
git rm web/src/components/CommitBar.tsx web/src/components/CommitBar.test.tsx \
       web/src/components/CommitDialog.tsx web/src/components/CommitDialog.test.tsx
```

- [ ] **Step 2: Remove references (each edit is small and exact)**

- `TopBar.tsx`: remove the `CommitBar` import (L5), the selectors `saving`/`dirty`/`uncommitted`/`lastCommit`/`committing` (L16–20), and the whole `<CommitBar … />` JSX block (L67–74). The right cluster is temporarily just `SlotRenderer` + Settings icon (PresenceCluster arrives in Task 7).
- `DialogHost.tsx`: remove the `CommitDialog` import, the `committing` selector (L24), and the `<CommitDialog … />` block (L45–50).
- `useCommands.ts`: remove `case "commit": st.setUi({ commitOpen: true }); break;` (L72–74).
- `commands.ts`: remove `{ id: "commit", label: "Commit changes…", defaultBinding: "Mod+Enter" }` (L12).
- `useGlobalKeys.ts`: `ALLOW_IN_EDITABLE` (L7) becomes `new Set(["open-palette"])`.
- `MoreMenu.tsx`: remove the `"Commit changes"` item (L18–22) and the now-unused `GitCommit` import (L1).
- `Settings.tsx`: remove the whole "Auto-commit" section — the `SectionLabel` block (L16–18), the idle toggle (L19–26), interval toggle (L27–36), interval minutes input (L37–51), and the `intervalStr` `useState` (L11–13). Keep everything else (e.g. "Load remote images").

- [ ] **Step 3: Update the touched tests**

- `useCommands.test.tsx`: delete the `it("opens the commit dialog", …)` block (L18–22) and the `expect(ids).toContain("commit")` assertion (L48).
- `MoreMenu.test.tsx`: delete the `it("opens Commit", …)` block (L18–22) and drop `commitOpen: false` from the `setUi` reset (L9).
- `Settings.test.tsx`: delete `it("toggles idle auto-commit", …)` and `it("edits the interval minutes", …)`; keep `it("toggles loading remote images", …)`.

- [ ] **Step 4: Verify**

Run: `pnpm test -- --run && pnpm lint`
Expected: PASS. (`store.ts` still defines `commitManual`/`ui.commitOpen` — unused-by-components is fine; store tests still exercise them until Task 4.)

- [ ] **Step 5: Commit**

```bash
git add -A web/src
git commit -m "feat(versions)!: remove Commit button, commit dialog and palette/menu entries (U1, U6)"
```

---

### Task 4: Store — sealNow / nameVersion / lastVersion; kill frontend commit timers

**Files:**
- Modify: `web/src/store/store.ts` (Settings L73–76, UiState L102–126, editBuffer L699–706, loadCairn L558, onEvent committed ~L481, commit actions L1253–1285, setSettings ~L1290)
- Test: `web/src/store/store.test.ts`

**Interfaces:**
- Consumes: `asCommand`, `RevisionEx` (Task 1); MockClient seal/name behavior (Task 2).
- Produces (used by Tasks 5, 9, 10):
  - `sealNow(): Promise<void>` — no-op unless `uncommitted && !committing`; sends `{type:"commit"}` (no message); on success sets `lastCommit`, clears `uncommitted`, awaits `loadLastVersion()`. **Silent on failure** (console.warn, no toast).
  - `nameVersion(commit: string, name: string): Promise<void>` — sends `name_version`; on `done` reloads `loadHistory()` + `loadLastVersion()`; failures toast via `pushError("Name version", err)`.
  - `loadLastVersion(): Promise<void>` — `runQuery({type:"vault_history", limit: 1})` → `lastVersion`.
  - State: `lastVersion: RevisionEx | null` (default null). `uncommitted`, `lastCommit`, `committing` survive unchanged.
  - `UiState`: `commitOpen` removed; `nameVersionFor: string | null` added (default `null`).
  - `Settings`: `idleAutoCommit`, `idleAutoCommitMs`, `intervalAutoCommit`, `intervalAutoCommitMin` removed.

- [ ] **Step 1: Write the failing tests** (replace/extend in `store.test.ts`)

Replace `it("commitManual commits and records the id", …)` (L92–97) with:

```ts
  it("sealNow seals a version, records the id and refreshes lastVersion", async () => {
    const { client, store } = setup();
    await store.getState().init();
    await client.sendCommand({ type: "write_note", path: "a.md", contents: "hello world" });
    store.setState({ uncommitted: true });
    await store.getState().sealNow();
    expect(store.getState().lastCommit).toBe("c0001");
    expect(store.getState().uncommitted).toBe(false);
    expect(store.getState().lastVersion?.id).toBe("c0001");
  });

  it("sealNow is a no-op when nothing is uncommitted", async () => {
    const { client, store } = setup();
    await store.getState().init();
    const spy = vi.spyOn(client, "sendCommand");
    await store.getState().sealNow();
    expect(spy).not.toHaveBeenCalled();
  });

  it("sealNow failures are silent (no toast)", async () => {
    const { client, store } = setup();
    await store.getState().init();
    vi.spyOn(client, "sendCommand").mockRejectedValueOnce(new Error("boom"));
    vi.spyOn(console, "warn").mockImplementation(() => {});
    store.setState({ uncommitted: true });
    await store.getState().sealNow();
    expect(store.getState().errors).toHaveLength(0);
    expect(store.getState().uncommitted).toBe(true); // still sealable later
  });

  it("nameVersion sends the C0 command and surfaces errors as toasts", async () => {
    const { client, store } = setup();
    await store.getState().init();
    const spy = vi.spyOn(client, "sendCommand");
    await store.getState().nameVersion("c0001", "Draft 1");
    expect(spy).toHaveBeenCalledWith({ type: "name_version", commit: "c0001", name: "Draft 1" });
    vi.spyOn(client, "sendCommand").mockRejectedValueOnce(new Error("boom"));
    await store.getState().nameVersion("c0001", "x");
    expect(store.getState().errors[0].message).toContain("boom");
  });
```

Rewire the three error-channel tests that used `commitManual` to use `nameVersion` (which toasts):
- `it("surfaces errors from a failing command", …)` (L~115–121): `await store.getState().nameVersion("c1", "x");` instead of `commitManual("x")`.
- The two-errors test (L~226–232): two `nameVersion` calls.
- The auto-dismiss test (L~234–241): one `nameVersion` call.

Delete the interval auto-commit test (the `it` block around L280–296 that calls `setSettings({ idleAutoCommit: false })` and advances timers by `DEFAULT_SETTINGS.intervalAutoCommitMin * 60_000`). In the test around L877 remove the `store.getState().setSettings({ idleAutoCommit: false });` isolation line (the autosave-debounce test stands on its own). Grep `AutoCommit` in the file afterwards — zero matches must remain.

- [ ] **Step 2: Run tests to verify they fail**

Run: `pnpm test -- --run src/store/store.test.ts`
Expected: FAIL — `sealNow`/`nameVersion`/`lastVersion` don't exist.

- [ ] **Step 3: Implement in `store.ts`**

1. Import: `import { asCommand, type RevisionEx } from "../client/contractExt";`
2. `Settings` interface + `DEFAULT_SETTINGS`: delete the four auto-commit fields. (Persisted settings that still carry stale keys are harmlessly ignored on load.)
3. `UiState`/`DEFAULT_UI`: delete `commitOpen`; add `nameVersionFor: string | null` / `nameVersionFor: null`.
4. State: add `lastVersion: RevisionEx | null` (init `null` next to `lastCommit`).
5. `editBuffer`: delete the idle-commit debounce block (L699–706). Keep the autosave debounce (L691–697) exactly as is.
6. Delete the module/closure `intervalHandle` variable and `commitManual`/`autoCommit`/`rearmInterval` (L1253–1285). `setSettings` (~L1290): remove the `rearmInterval()` call.
7. `loadCairn` (L558): replace `rearmInterval();` with `void get().loadLastVersion();`
8. `onEvent` committed handler (~L481): after `set({ lastCommit: e.commit, uncommitted: false })`, add `void get().loadLastVersion();` (engine-driven seals keep the StatusBar fresh).
9. New actions (same spot the old ones lived, ~L1253):

```ts
      async sealNow() {
        if (!get().uncommitted || get().committing) return;
        set({ committing: true });
        try {
          const res = await client.sendCommand(asCommand({ type: "commit" }));
          if (res.type === "committed") {
            set({ lastCommit: res.commit, uncommitted: false });
            await get().loadLastVersion();
          } else unexpected("Seal version", res);
        } catch (err) {
          // Seal hints are background garnish — a failed hint must never toast.
          console.warn("sealNow failed", err);
        } finally {
          set({ committing: false });
        }
      },

      async nameVersion(commit, name) {
        try {
          const res = await client.sendCommand(
            asCommand({ type: "name_version", commit, name }),
          );
          if (res.type === "done") {
            await get().loadHistory();
            void get().loadLastVersion();
          } else unexpected("Name version", res);
        } catch (err) {
          pushError("Name version", err);
        }
      },

      async loadLastVersion() {
        try {
          const res = await client.runQuery({ type: "vault_history", limit: 1 });
          if (res.type === "history")
            set({ lastVersion: (res.revisions[0] as RevisionEx | undefined) ?? null });
        } catch {
          // Status-bar garnish — leave the previous value, never toast.
        }
      },
```

10. Update the `CairnState` interface: remove `commitManual`/`autoCommit`/`rearmInterval` signatures; add `sealNow(): Promise<void>`, `nameVersion(commit: string, name: string): Promise<void>`, `loadLastVersion(): Promise<void>`, `lastVersion: RevisionEx | null`.

- [ ] **Step 4: Run the store tests, then the full suite**

Run: `pnpm test -- --run src/store/store.test.ts && pnpm test -- --run`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/store/store.ts web/src/store/store.test.ts
git commit -m "feat(versions): store consumes engine sealing — sealNow/nameVersion/lastVersion, timers removed (U1)"
```

---

### Task 5: StatusBar (U2)

**Files:**
- Create: `web/src/components/StatusBar.tsx`
- Modify: `web/src/app/App.tsx`
- Test: `web/src/components/StatusBar.test.tsx`

**Interfaces:**
- Consumes: `RevisionEx` (Task 1), `lastVersion` + `showHistory()` (Task 4 / historySlice), `relativeTime` (`components/history/formatRevision.ts`).
- Produces: `StatusBar` props `{ saving: boolean; dirty: boolean; sync: "ok" | "reconnecting" | "down"; lastVersion: RevisionEx | null; onShowVersions: () => void }`. Task 9's `versionWordDelta` will be wired in Task 9 — v1 here shows only the relative time.

- [ ] **Step 1: Write the failing test**

```tsx
// web/src/components/StatusBar.test.tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { StatusBar } from "./StatusBar";

const NOW_SECS = Math.floor(Date.now() / 1000);

describe("StatusBar", () => {
  it("shows the three calm axes when healthy", () => {
    render(
      <StatusBar saving={false} dirty={false} sync="ok" lastVersion={null} onShowVersions={() => {}} />,
    );
    expect(screen.getByTestId("status-saved")).toHaveTextContent("Saved");
    expect(screen.getByTestId("status-sync")).toHaveTextContent("Synced");
    expect(screen.getByRole("button", { name: /versions/i })).toBeInTheDocument();
  });

  it("shows Saving… while a flush is in flight", () => {
    render(
      <StatusBar saving={true} dirty={true} sync="ok" lastVersion={null} onShowVersions={() => {}} />,
    );
    expect(screen.getByTestId("status-saved")).toHaveTextContent("Saving…");
  });

  it("is reassuring when offline", () => {
    render(
      <StatusBar saving={false} dirty={false} sync="down" lastVersion={null} onShowVersions={() => {}} />,
    );
    expect(screen.getByTestId("status-sync")).toHaveTextContent("Offline — changes saved locally");
  });

  it("summarizes the last version and opens the browser", async () => {
    const onShow = vi.fn();
    render(
      <StatusBar
        saving={false}
        dirty={false}
        sync="ok"
        lastVersion={{ id: "c9", message: "m", author: "a", timestamp_secs: NOW_SECS - 60 }}
        onShowVersions={onShow}
      />,
    );
    expect(screen.getByTestId("status-last-version")).toHaveTextContent(/Last version:/);
    await userEvent.click(screen.getByRole("button", { name: /versions/i }));
    expect(onShow).toHaveBeenCalled();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test -- --run src/components/StatusBar.test.tsx`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement**

```tsx
// web/src/components/StatusBar.tsx
import type { RevisionEx } from "../client/contractExt";
import { relativeTime } from "./history/formatRevision";

export type SyncStatus = "ok" | "reconnecting" | "down";

/** Persistent bottom strip: the calm vault-wide home for the save / sync /
 *  version axes (spec Part 2 "Status bar"). Hidden on mobile (bottom-nav tier).
 *  "Saved" is strictly the disk-flush axis; the git layer is "Versions". */
export function StatusBar(props: {
  saving: boolean;
  dirty: boolean;
  sync: SyncStatus;
  lastVersion: RevisionEx | null;
  onShowVersions: () => void;
}) {
  const saveLabel = props.saving
    ? "Saving…"
    : props.dirty
      ? "Unsaved changes"
      : "✓ Saved";
  const syncLabel =
    props.sync === "ok"
      ? "Synced"
      : props.sync === "reconnecting"
        ? "Syncing…"
        : "Offline — changes saved locally";
  const lv = props.lastVersion;
  return (
    <div
      data-testid="status-bar"
      className="hidden items-center gap-2 border-t border-border bg-surface px-3 py-1 text-xs text-muted md:flex"
    >
      <span data-testid="status-saved">{saveLabel}</span>
      <span aria-hidden>·</span>
      <span
        data-testid="status-sync"
        className={props.sync === "down" ? "font-medium text-text" : undefined}
      >
        {syncLabel}
      </span>
      <span aria-hidden>·</span>
      <button
        type="button"
        className="rounded px-1 hover:bg-surface-2 hover:text-text"
        onClick={props.onShowVersions}
      >
        🕘 Versions
      </button>
      <span className="grow" />
      {lv && (
        <span data-testid="status-last-version">
          Last version: {relativeTime(lv.timestamp_secs)}
        </span>
      )}
    </div>
  );
}
```

Mount in `App.tsx` — replace the `<AppShell … />` element (keep all its props) with a flex column wrapping shell + strip, and add the selectors:

```tsx
  const saving = useCairn((s) => s.saving);
  const dirty = useCairn((s) => s.dirty);
  const lastVersion = useCairn((s) => s.lastVersion);
```

```tsx
      <div className="flex h-full flex-col">
        <div className="min-h-0 flex-1">
          <AppShell
            topBar={<TopBar />}
            list={<Sidebar />}
            editor={<EditorPane />}
            backlinks={<RightAside />}
            ask={<AskPanelHost />}
            recovery={<RecoveryPanelHost />}
          />
        </div>
        <StatusBar
          saving={saving}
          dirty={dirty}
          sync={liveUpdates}
          lastVersion={lastVersion}
          onShowVersions={() => cairnStore.getState().showHistory()}
        />
      </div>
```

(import `StatusBar`; `liveUpdates` selector already exists in App.)

- [ ] **Step 4: Run tests**

Run: `pnpm test -- --run src/components/StatusBar.test.tsx && pnpm test -- --run`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/StatusBar.tsx web/src/components/StatusBar.test.tsx web/src/app/App.tsx
git commit -m "feat(versions): bottom StatusBar — saved/sync/versions axes (U2)"
```

---

### Task 6: collabSlice — peers[], conflict actions, "their version" state (U3 store half)

**Files:**
- Modify: `web/src/store/collabSlice.ts`, `web/src/store/store.ts` (UiState rename), `web/src/components/DialogHost.tsx` + `web/src/components/collab/CollabPresenceHost.tsx` (ui-key rename only, mechanical)
- Test: `web/src/store/collabSlice.test.ts`

**Interfaces:**
- Consumes: existing slice machinery (token, setBuffer, `reloadNoteBuffer`).
- Produces (used by Task 7):
  - `CollabPeer = { id: string; name?: string; editing?: boolean }` (exported).
  - `CollabPresence` gains `peers: CollabPeer[]` (default `[]`, empty until the engine ships a roster) and `theirs: { note: string; contents: string } | null` (default `null`).
  - Actions: `collabKeepMine()` (close dialog, `pendingCount: 0`, `theirs: null`, buffer untouched), `collabViewTheirs()` (fetch `get_note` → set `theirs`, buffer untouched), `collabExitTheirs()` (`theirs: null`). `collabReloadNow()` additionally clears `theirs`.
  - `UiState.collabReloadConfirmOpen` renamed to `collabConflictOpen` everywhere.

- [ ] **Step 1: Write the failing tests** (append inside the `describe("collab slice", …)` block of `collabSlice.test.ts`; the file already provides `make()` seeding `MockClient({ "n.md": "# N\n", "m.md": "# M\n" })`, the `wireOp` fixture, fake timers in `beforeEach`, and the `client.mockCollabHandlers!.onForeignOp!` injection seam — reuse them exactly)

```ts
  // Shared setup for the conflict-flow tests: follow n.md with a dirty buffer
  // and one pending foreign change (mirrors the existing dirty-buffer test).
  const makeConflict = async () => {
    const { client, store } = make();
    await store.getState().openNote("n.md");
    store.getState().editBuffer("# N\nmine");
    store.getState().collabFollow("n.md");
    client.mockCollabHandlers!.onForeignOp!("n.md", wireOp);
    expect(store.getState().collab.pendingCount).toBe(1);
    return { client, store };
  };

  it("collabViewTheirs fetches the remote contents without touching the buffer", async () => {
    const { store } = await makeConflict();
    store.getState().collabViewTheirs();
    vi.useRealTimers(); // flush the internal get_note microtask
    await vi.waitFor(() =>
      expect(store.getState().collab.theirs).toEqual({
        note: "n.md",
        contents: "# N\n",
      }),
    );
    expect(store.getState().openNotes["n.md"].contents).toBe("# N\nmine");
    expect(store.getState().openNotes["n.md"].dirty).toBe(true);
    vi.useFakeTimers();
  });

  it("collabKeepMine clears the conflict but keeps my buffer", async () => {
    const { store } = await makeConflict();
    store.getState().setUi({ collabConflictOpen: true });
    store.getState().collabKeepMine();
    expect(store.getState().collab.pendingCount).toBe(0);
    expect(store.getState().collab.theirs).toBeNull();
    expect(store.getState().ui.collabConflictOpen).toBe(false);
    expect(store.getState().openNotes["n.md"].contents).toBe("# N\nmine");
    expect(store.getState().openNotes["n.md"].dirty).toBe(true);
  });

  it("collabReloadNow (use their version) force-replaces and clears theirs", async () => {
    const { store } = await makeConflict();
    store.getState().collabViewTheirs();
    store.getState().collabReloadNow();
    vi.useRealTimers();
    await vi.waitFor(() =>
      expect(store.getState().openNotes["n.md"].dirty).toBe(false),
    );
    expect(store.getState().openNotes["n.md"].contents).toBe("# N\n");
    expect(store.getState().collab.pendingCount).toBe(0);
    expect(store.getState().collab.theirs).toBeNull();
    vi.useFakeTimers();
  });

  it("collabExitTheirs returns to my version without changes", async () => {
    const { store } = await makeConflict();
    store.getState().collabViewTheirs();
    vi.useRealTimers();
    await vi.waitFor(() =>
      expect(store.getState().collab.theirs).not.toBeNull(),
    );
    store.getState().collabExitTheirs();
    expect(store.getState().collab.theirs).toBeNull();
    expect(store.getState().openNotes["n.md"].contents).toBe("# N\nmine");
    vi.useFakeTimers();
  });
```

Also rename the ui key in the two existing tests `"switching the followed note closes an open reload-confirm dialog"` (L83–89) and `"collabStop closes an open reload-confirm dialog"` (L91–97): `collabReloadConfirmOpen` → `collabConflictOpen` (titles: "reload-confirm" → "conflict").

- [ ] **Step 2: Run tests to verify they fail**

Run: `pnpm test -- --run src/store/collabSlice.test.ts`
Expected: FAIL — new actions/fields missing.

- [ ] **Step 3: Implement**

In `collabSlice.ts`:

```ts
export interface CollabPeer {
  id: string;
  name?: string;
  editing?: boolean;
}

export interface CollabPresence {
  note: string | null;
  live: boolean;
  pendingCount: number;
  /** Roster from the engine's future awareness channel. Empty today (the wire
   *  is anonymous); the PresenceCluster degrades to "Someone is editing…". */
  peers: CollabPeer[];
  /** Non-destructive view of the incoming remote version (conflict flow). */
  theirs: { note: string; contents: string } | null;
}

export const DEFAULT_COLLAB: CollabPresence = {
  note: null,
  live: false,
  pendingCount: 0,
  peers: [],
  theirs: null,
};
```

- `CollabState` interface: add `collabKeepMine(): void; collabViewTheirs(): void; collabExitTheirs(): void;`
- `collabFollow`: the fresh-presence literal becomes `{ note: path, live: false, pendingCount: 0, peers: [], theirs: null }`; the `setUi` call becomes `{ collabConflictOpen: false }`.
- `collabStop`: `setUi({ collabConflictOpen: false })`.
- `collabReloadNow`: in the success branch set `{ collab: { ...s.collab, pendingCount: 0, theirs: null } }`.
- New actions:

```ts
    collabViewTheirs() {
      const note = get().collab.note;
      if (!note) return;
      const my = token;
      void (async () => {
        const res = await client.runQuery({ type: "get_note", path: note });
        if (my !== token) return; // superseded by a note switch / stop
        if (res.type === "note") {
          set((s) => ({
            collab: { ...s.collab, theirs: { note, contents: res.contents } },
          }));
        }
      })();
    },

    collabKeepMine() {
      get().setUi({ collabConflictOpen: false });
      set((s) => ({ collab: { ...s.collab, pendingCount: 0, theirs: null } }));
    },

    collabExitTheirs() {
      set((s) => ({ collab: { ...s.collab, theirs: null } }));
    },
```

In `store.ts`: rename `collabReloadConfirmOpen` → `collabConflictOpen` in `UiState` (L109) and `DEFAULT_UI` (L126). In `DialogHost.tsx` (L52–54) and `CollabPresenceHost.tsx` (onReload) update the key name — behavior unchanged in this task.

- [ ] **Step 4: Run tests**

Run: `pnpm test -- --run src/store/collabSlice.test.ts && pnpm test -- --run`
Expected: PASS (existing collab tests updated only where they reference the renamed ui key).

- [ ] **Step 5: Commit**

```bash
git add web/src/store web/src/components/DialogHost.tsx web/src/components/collab/CollabPresenceHost.tsx
git commit -m "feat(collab): peers[] + non-destructive conflict actions in collabSlice (U3)"
```

---

### Task 7: PresenceCluster + ConflictDialog + TheirVersionView (U3 UI half)

**Files:**
- Create: `web/src/components/collab/PresenceCluster.tsx`, `ConflictDialog.tsx`, `TheirVersionView.tsx`, `web/src/components/history/DiffTable.tsx`
- Modify: `web/src/components/TopBar.tsx`, `DialogHost.tsx`, `EditorPane.tsx`, `collab/CollabPresenceHost.tsx`, `history/RevisionView.tsx`, `app/App.tsx`
- Delete: `collab/CollabPresencePill.tsx` (+ test), `LiveUpdatesBanner.tsx` (+ test), `collab/CollabReloadDialog.tsx` (+ test)
- Test: `web/src/components/collab/PresenceCluster.test.tsx`, `ConflictDialog.test.tsx`, `TheirVersionView.test.tsx`

**Interfaces:**
- Consumes: `CollabPeer`, `collab.{live,peers,pendingCount,theirs}`, `collabKeepMine/collabViewTheirs/collabExitTheirs/collabReloadNow`, `ui.collabConflictOpen` (Task 6); `liveUpdates` store state; `lineDiff` (`history/lineDiff.ts`); `Modal`/`Button` ui primitives.
- Produces:
  - `PresenceCluster` props: `{ status: "ok" | "reconnecting" | "down"; live: boolean; peers: CollabPeer[]; conflictCount: number; onConflict: () => void; onReconnect: () => void }`.
  - `editingLabel(peers: CollabPeer[]): string` exported for tests.
  - `DiffTable` props: `{ rows: DiffRow[] }`.

- [ ] **Step 1: Write the failing tests**

```tsx
// web/src/components/collab/PresenceCluster.test.tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { PresenceCluster, editingLabel } from "./PresenceCluster";

const base = {
  status: "ok" as const,
  live: false,
  peers: [],
  conflictCount: 0,
  onConflict: () => {},
  onReconnect: () => {},
};

describe("PresenceCluster", () => {
  it("shows a calm Connected chip when healthy and alone", () => {
    render(<PresenceCluster {...base} />);
    expect(screen.getByText("Connected")).toBeInTheDocument();
  });

  it("degrades to 'Someone is editing…' on anonymous live activity", () => {
    render(<PresenceCluster {...base} live={true} />);
    expect(screen.getByText("Someone is editing…")).toBeInTheDocument();
  });

  it("names editors when the roster is present", () => {
    render(
      <PresenceCluster
        {...base}
        live={true}
        peers={[{ id: "1", name: "Maya", editing: true }]}
      />,
    );
    expect(screen.getByText("Maya is editing…")).toBeInTheDocument();
  });

  it("shows N here when peers are present but idle", () => {
    render(
      <PresenceCluster {...base} peers={[{ id: "1" }, { id: "2" }]} />,
    );
    expect(screen.getByText("2 here")).toBeInTheDocument();
  });

  it("pulses Reconnecting…", () => {
    render(<PresenceCluster {...base} status="reconnecting" />);
    expect(screen.getByText("Reconnecting…")).toBeInTheDocument();
  });

  it("offers Reconnect when offline", async () => {
    const onReconnect = vi.fn();
    render(<PresenceCluster {...base} status="down" onReconnect={onReconnect} />);
    expect(screen.getByText("Offline")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Reconnect" }));
    expect(onReconnect).toHaveBeenCalled();
  });

  it("conflict wins over everything and opens the dialog", async () => {
    const onConflict = vi.fn();
    render(
      <PresenceCluster {...base} status="down" live conflictCount={2} onConflict={onConflict} />,
    );
    await userEvent.click(
      screen.getByRole("button", { name: /also changed on another device/i }),
    );
    expect(onConflict).toHaveBeenCalled();
  });
});

describe("editingLabel", () => {
  it("rolls past two names", () => {
    expect(editingLabel([])).toBe("Someone is editing…");
    expect(editingLabel([{ id: "1", name: "Maya", editing: true }])).toBe("Maya is editing…");
    expect(
      editingLabel([
        { id: "1", name: "Maya", editing: true },
        { id: "2", name: "Sam", editing: true },
      ]),
    ).toBe("Maya, Sam editing…");
    expect(
      editingLabel([
        { id: "1", name: "Maya", editing: true },
        { id: "2", name: "Sam", editing: true },
        { id: "3", name: "Ada", editing: true },
      ]),
    ).toBe("Maya, Sam +1 editing…");
  });
});
```

```tsx
// web/src/components/collab/ConflictDialog.test.tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ConflictDialog } from "./ConflictDialog";

describe("ConflictDialog", () => {
  it("renders nothing when closed", () => {
    render(
      <ConflictDialog open={false} onOpenChange={() => {}} onKeepMine={() => {}} onSeeTheirs={() => {}} />,
    );
    expect(screen.queryByText(/also changed/i)).not.toBeInTheDocument();
  });

  it("keeps my version by default action, no danger styling", async () => {
    const onKeep = vi.fn();
    render(
      <ConflictDialog open onOpenChange={() => {}} onKeepMine={onKeep} onSeeTheirs={() => {}} />,
    );
    expect(
      screen.getByText("This note also changed on another device"),
    ).toBeInTheDocument();
    const keep = screen.getByRole("button", { name: "Keep my version" });
    expect(keep.className).not.toMatch(/danger/);
    await userEvent.click(keep);
    expect(onKeep).toHaveBeenCalled();
  });

  it("see their version is offered and non-destructive", async () => {
    const onSee = vi.fn();
    const onOpenChange = vi.fn();
    render(
      <ConflictDialog open onOpenChange={onOpenChange} onKeepMine={() => {}} onSeeTheirs={onSee} />,
    );
    await userEvent.click(screen.getByRole("button", { name: "See their version" }));
    expect(onSee).toHaveBeenCalled();
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });
});
```

```tsx
// web/src/components/collab/TheirVersionView.test.tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { TheirVersionView } from "./TheirVersionView";

describe("TheirVersionView", () => {
  it("shows a read-only diff of their version vs mine", () => {
    render(
      <TheirVersionView
        path="n.md"
        mine={"shared\nmine"}
        theirs={"shared\ntheirs"}
        onBack={() => {}}
        onUseTheirs={() => {}}
      />,
    );
    expect(screen.getByText(/their version/i)).toBeInTheDocument();
    expect(screen.getByText("theirs")).toBeInTheDocument();
    expect(screen.getByText("mine")).toBeInTheDocument();
  });

  it("back and use-their-version are explicit separate actions", async () => {
    const onBack = vi.fn();
    const onUse = vi.fn();
    render(
      <TheirVersionView path="n.md" mine="a" theirs="b" onBack={onBack} onUseTheirs={onUse} />,
    );
    await userEvent.click(screen.getByRole("button", { name: /back to my version/i }));
    expect(onBack).toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: /use their version/i }));
    expect(onUse).toHaveBeenCalled();
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `pnpm test -- --run src/components/collab`
Expected: FAIL — modules not found.

- [ ] **Step 3: Implement the four components**

`DiffTable.tsx`: MOVE the `ROW_STYLE` and `ROW_SIGN` constants and the `rows.map(...)` diff-row JSX block out of `RevisionView.tsx` (lines ~87–113) **verbatim** into:

```tsx
// web/src/components/history/DiffTable.tsx
import type { DiffRow } from "./lineDiff";

// ROW_STYLE / ROW_SIGN moved verbatim from RevisionView.tsx

export function DiffTable({ rows }: { rows: DiffRow[] }) {
  return (
    <div className="min-h-0 flex-1 overflow-auto p-3 font-mono text-sm">
      {/* the rows.map(...) block moved verbatim from RevisionView.tsx */}
    </div>
  );
}
```

then make `RevisionView.tsx` render `<DiffTable rows={rows} />` in the diff branch. Run the existing `RevisionView.test.tsx` — it must stay green.

```tsx
// web/src/components/collab/PresenceCluster.tsx
import { Button } from "../ui/Button";
import type { CollabPeer } from "../../store/collabSlice";

export function editingLabel(peers: CollabPeer[]): string {
  const names = peers
    .filter((p) => p.editing && p.name)
    .map((p) => p.name as string);
  if (names.length === 0) return "Someone is editing…";
  if (names.length === 1) return `${names[0]} is editing…`;
  if (names.length === 2) return `${names[0]}, ${names[1]} editing…`;
  return `${names[0]}, ${names[1]} +${names.length - 2} editing…`;
}

const CHIP =
  "flex items-center gap-2 rounded-full border border-border bg-surface-2 px-2.5 py-1 text-xs";

/** One persistent presence element in the top-right of the TopBar (spec Part 1).
 *  Calm by default, louder only when it matters; replaces the two corner cards.
 *  Priority: conflict > offline > reconnecting > live editing > baseline. */
export function PresenceCluster(props: {
  status: "ok" | "reconnecting" | "down";
  live: boolean;
  peers: CollabPeer[];
  conflictCount: number;
  onConflict: () => void;
  onReconnect: () => void;
}) {
  if (props.conflictCount > 0) {
    return (
      <button
        type="button"
        onClick={props.onConflict}
        className={`${CHIP} text-text hover:bg-surface`}
        data-testid="presence-conflict"
      >
        <span aria-hidden className="h-2 w-2 rounded-full bg-accent" />
        <span>Also changed on another device</span>
      </button>
    );
  }
  if (props.status === "down") {
    return (
      <div role="status" className={`${CHIP} text-text`} data-testid="presence-offline">
        <span aria-hidden className="h-2 w-2 rounded-full bg-danger" />
        <span>Offline</span>
        <Button variant="ghost" onClick={props.onReconnect}>
          Reconnect
        </Button>
      </div>
    );
  }
  if (props.status === "reconnecting") {
    return (
      <div role="status" className={`${CHIP} text-muted`} data-testid="presence-reconnecting">
        <span aria-hidden className="h-2 w-2 animate-pulse rounded-full bg-accent" />
        <span>Reconnecting…</span>
      </div>
    );
  }
  if (props.live) {
    const pips = Math.max(1, props.peers.filter((p) => p.editing).length);
    return (
      <div role="status" className={`${CHIP} text-muted`} data-testid="presence-editing">
        {Array.from({ length: Math.min(pips, 3) }, (_, i) => (
          <span key={i} aria-hidden className="h-2 w-2 animate-pulse rounded-full bg-success" />
        ))}
        <span>{editingLabel(props.peers)}</span>
      </div>
    );
  }
  return (
    <div role="status" className={`${CHIP} text-muted`} data-testid="presence-idle">
      <span aria-hidden className="h-2 w-2 rounded-full bg-success" />
      <span>{props.peers.length > 0 ? `${props.peers.length} here` : "Connected"}</span>
    </div>
  );
}
```

```tsx
// web/src/components/collab/ConflictDialog.tsx
import { Modal } from "../ui/Modal";
import { Button } from "../ui/Button";

/** Calm conflict choice (replaces CollabReloadDialog). Never a one-click way
 *  to lose work: "See their version" only opens a read-only view. */
export function ConflictDialog({
  open,
  onOpenChange,
  onKeepMine,
  onSeeTheirs,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onKeepMine: () => void;
  onSeeTheirs: () => void;
}) {
  const close = () => onOpenChange(false);
  return (
    <Modal
      open={open}
      onClose={close}
      title="This note also changed on another device"
      description="You can keep your version, or look at theirs first. Nothing is discarded until you choose."
    >
      <div className="flex justify-end gap-2">
        <Button
          variant="ghost"
          onClick={() => {
            onSeeTheirs();
            close();
          }}
        >
          See their version
        </Button>
        <Button variant="primary" onClick={onKeepMine}>
          Keep my version
        </Button>
      </div>
    </Modal>
  );
}
```

```tsx
// web/src/components/collab/TheirVersionView.tsx
import { Button } from "../ui/Button";
import { lineDiff } from "../history/lineDiff";
import { DiffTable } from "../history/DiffTable";

/** Read-only view of the incoming remote version, diffed against my buffer.
 *  Overwriting my buffer is the explicit "Use their version" action here —
 *  never one click from the conflict dialog. */
export function TheirVersionView(props: {
  path: string;
  mine: string;
  theirs: string;
  onBack: () => void;
  onUseTheirs: () => void;
}) {
  const rows = lineDiff(props.mine, props.theirs);
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex items-center gap-2 border-b border-border px-3 py-2 text-sm">
        <span className="text-muted">
          Their version — <span className="text-text">{props.path}</span>
        </span>
        <span className="grow" />
        <Button variant="ghost" onClick={props.onBack}>
          ← Back to my version
        </Button>
        <Button variant="primary" onClick={props.onUseTheirs}>
          Use their version
        </Button>
      </div>
      <DiffTable rows={rows} />
    </div>
  );
}
```

- [ ] **Step 4: Wire mounts and delete the old surfaces**

- `TopBar.tsx`: add selectors `const liveUpdates = useCairn((s) => s.liveUpdates);` and `const collab = useCairn((s) => s.collab);`; mount in the right cluster (between `<SlotRenderer …/>` and the Settings IconButton):

```tsx
      <PresenceCluster
        status={liveUpdates}
        live={collab.live}
        peers={collab.peers}
        conflictCount={collab.pendingCount}
        onConflict={() => actions.setUi({ collabConflictOpen: true })}
        onReconnect={() => void cairnStore.getState().refreshAll()}
      />
```

- `DialogHost.tsx`: replace the `CollabReloadDialog` block with:

```tsx
      <ConflictDialog
        open={ui.collabConflictOpen}
        onOpenChange={(o) => actions.setUi({ collabConflictOpen: o })}
        onKeepMine={actions.collabKeepMine}
        onSeeTheirs={actions.collabViewTheirs}
      />
```

- `EditorPane.tsx`: next to the existing `viewingRevision` overlay branch (~L132–141), add a `theirs` overlay for the focused pane's path:

```tsx
  const theirs = useCairn((s) => s.collab.theirs);
  // in render, before the viewingRevision branch:
  if (theirs && theirs.note === path) {
    return (
      <TheirVersionView
        path={theirs.note}
        mine={buffer?.contents ?? ""}
        theirs={theirs.contents}
        onBack={() => actions.collabExitTheirs()}
        onUseTheirs={() => actions.collabReloadNow()}
      />
    );
  }
```

(match the surrounding pane code's actual variable names for `path`/`buffer` — they exist in the pane render already.)

- `CollabPresenceHost.tsx`: remove the pill import and render; keep both `useEffect`s (follow/stop lifecycle). Return `null`. Remove the now-unused `collab`/`dirty` selectors.
- `App.tsx`: remove the `LiveUpdatesBanner` import and mount and the `liveUpdates` selector **only if** unused after Task 5 wiring (it is used by StatusBar — keep the selector, delete only the banner element).
- Delete files:

```bash
git rm web/src/components/collab/CollabPresencePill.tsx web/src/components/collab/CollabPresencePill.test.tsx \
       web/src/components/LiveUpdatesBanner.tsx web/src/components/LiveUpdatesBanner.test.tsx \
       web/src/components/collab/CollabReloadDialog.tsx web/src/components/collab/CollabReloadDialog.test.tsx
```

- [ ] **Step 5: Run the full suite**

Run: `pnpm test -- --run && pnpm lint`
Expected: PASS — new component tests green, `RevisionView.test.tsx` still green after the DiffTable extraction.

- [ ] **Step 6: Commit**

```bash
git add -A web/src
git commit -m "feat(collab): PresenceCluster + calm conflict flow replace corner cards (U3)"
```

---

### Task 8: Versions utilities — grouping + word delta (U4 pure logic)

**Files:**
- Create: `web/src/components/history/groupRevisions.ts`, `web/src/components/history/versionSummary.ts`
- Test: `web/src/components/history/groupRevisions.test.ts`, `versionSummary.test.ts`

**Interfaces:**
- Consumes: `RevisionEx` (Task 1).
- Produces (used by Task 9):
  - `SESSION_GAP_SECS = 1800`, `SessionGroup = { head: RevisionEx; rest: RevisionEx[] }`, `DayGroup = { label: string; sessions: SessionGroup[] }`
  - `groupRevisions(revs: RevisionEx[], nowSecs: number): DayGroup[]` (input newest-first, contract order)
  - `dayLabel(tsSecs: number, nowSecs: number): string` → "Today" | "Yesterday" | locale date
  - `versionWordDelta(r: RevisionEx): { added: number; removed: number } | null` — structured fields first, else parses `(+N/−M words)` from the message (accepts `−` U+2212 or ASCII `-`).

- [ ] **Step 1: Write the failing tests**

```ts
// web/src/components/history/groupRevisions.test.ts
import { describe, it, expect } from "vitest";
import { groupRevisions, dayLabel, SESSION_GAP_SECS } from "./groupRevisions";
import type { RevisionEx } from "../../client/contractExt";

// Fixed "now": 2026-08-21 12:00 local — tests build timestamps relative to it.
const NOW = new Date(2026, 7, 21, 12, 0, 0).getTime() / 1000;
const rev = (id: string, ts: number): RevisionEx => ({
  id,
  message: `m-${id}`,
  author: "a",
  timestamp_secs: ts,
});

describe("dayLabel", () => {
  it("labels today, yesterday and older dates", () => {
    expect(dayLabel(NOW - 60, NOW)).toBe("Today");
    expect(dayLabel(NOW - 86_400, NOW)).toBe("Yesterday");
    expect(dayLabel(NOW - 3 * 86_400, NOW)).not.toMatch(/Today|Yesterday/);
  });
});

describe("groupRevisions", () => {
  it("groups newest-first revisions into days and 30-min sessions", () => {
    const revs = [
      rev("c4", NOW - 60), // today, session A
      rev("c3", NOW - 60 - SESSION_GAP_SECS / 2), // today, session A (gap 15m)
      rev("c2", NOW - 60 - SESSION_GAP_SECS * 3), // today, session B (gap > 30m)
      rev("c1", NOW - 86_400), // yesterday
    ];
    const days = groupRevisions(revs, NOW);
    expect(days.map((d) => d.label)).toEqual(["Today", "Yesterday"]);
    expect(days[0].sessions).toHaveLength(2);
    expect(days[0].sessions[0].head.id).toBe("c4");
    expect(days[0].sessions[0].rest.map((r) => r.id)).toEqual(["c3"]);
    expect(days[0].sessions[1].head.id).toBe("c2");
    expect(days[1].sessions[0].head.id).toBe("c1");
  });

  it("returns [] for no revisions", () => {
    expect(groupRevisions([], NOW)).toEqual([]);
  });
});
```

```ts
// web/src/components/history/versionSummary.test.ts
import { describe, it, expect } from "vitest";
import { versionWordDelta } from "./versionSummary";

const base = { id: "c1", author: "a", timestamp_secs: 1 };

describe("versionWordDelta", () => {
  it("prefers structured C0 fields", () => {
    expect(
      versionWordDelta({ ...base, message: "x", words_added: 124, words_removed: 3 }),
    ).toEqual({ added: 124, removed: 3 });
  });

  it("falls back to parsing the deterministic message", () => {
    expect(
      versionWordDelta({ ...base, message: 'Edit "Q3 Roadmap" § Goals (+42/−3 words)' }),
    ).toEqual({ added: 42, removed: 3 });
    expect(
      versionWordDelta({ ...base, message: "Edit \"x\" (+1/-2 words)" }),
    ).toEqual({ added: 1, removed: 2 });
  });

  it("returns null when nothing is derivable", () => {
    expect(versionWordDelta({ ...base, message: "cairn: update note.md" })).toBeNull();
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `pnpm test -- --run src/components/history/groupRevisions.test.ts src/components/history/versionSummary.test.ts`
Expected: FAIL — modules not found.

- [ ] **Step 3: Implement**

```ts
// web/src/components/history/groupRevisions.ts
import type { RevisionEx } from "../../client/contractExt";

/** Two seals within this gap belong to the same editing session (spec: the
 *  browser tames the flat auto-stream by grouping, not deletion). */
export const SESSION_GAP_SECS = 30 * 60;

export interface SessionGroup {
  head: RevisionEx;
  rest: RevisionEx[];
}
export interface DayGroup {
  label: string;
  sessions: SessionGroup[];
}

export function dayLabel(tsSecs: number, nowSecs: number): string {
  const d = new Date(tsSecs * 1000);
  const n = new Date(nowSecs * 1000);
  const startOfDay = (x: Date) =>
    new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const diffDays = Math.round((startOfDay(n) - startOfDay(d)) / 86_400_000);
  if (diffDays <= 0) return "Today";
  if (diffDays === 1) return "Yesterday";
  return d.toLocaleDateString(undefined, {
    month: "short",
    day: "numeric",
    year: d.getFullYear() === n.getFullYear() ? undefined : "numeric",
  });
}

/** Input is newest-first (contract order); output preserves it. */
export function groupRevisions(revs: RevisionEx[], nowSecs: number): DayGroup[] {
  const days: DayGroup[] = [];
  for (const r of revs) {
    const label = dayLabel(r.timestamp_secs, nowSecs);
    let day = days[days.length - 1];
    if (!day || day.label !== label) {
      day = { label, sessions: [] };
      days.push(day);
    }
    const session = day.sessions[day.sessions.length - 1];
    const prev = session ? (session.rest[session.rest.length - 1] ?? session.head) : null;
    if (prev && prev.timestamp_secs - r.timestamp_secs <= SESSION_GAP_SECS) {
      session.rest.push(r);
    } else {
      day.sessions.push({ head: r, rest: [] });
    }
  }
  return days;
}
```

```ts
// web/src/components/history/versionSummary.ts
import type { RevisionEx } from "../../client/contractExt";

// Matches the engine's deterministic label template "(+N/−M words)".
// Accepts U+2212 minus (the template) and ASCII hyphen (defensive).
const DELTA_RE = /\(\+(\d+)\/[−-](\d+) words\)/;

export function versionWordDelta(
  r: RevisionEx,
): { added: number; removed: number } | null {
  if (typeof r.words_added === "number" && typeof r.words_removed === "number")
    return { added: r.words_added, removed: r.words_removed };
  const m = DELTA_RE.exec(r.message);
  return m ? { added: Number(m[1]), removed: Number(m[2]) } : null;
}
```

- [ ] **Step 4: Run tests**

Run: `pnpm test -- --run src/components/history`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/history/groupRevisions.ts web/src/components/history/groupRevisions.test.ts \
        web/src/components/history/versionSummary.ts web/src/components/history/versionSummary.test.ts
git commit -m "feat(versions): day/session grouping + word-delta summary helpers (U4)"
```

---

### Task 9: Versions browser (U4 UI)

**Files:**
- Modify: `web/src/components/history/HistoryList.tsx`, `HistoryPane.tsx`, `HistoryList.test.tsx`, `web/src/components/RightAside.tsx`, `web/src/components/DialogHost.tsx`, `web/src/components/shortcuts/commands.ts` (label only), `web/src/components/StatusBar.tsx` (word delta)
- Create: `web/src/components/history/NameVersionDialog.tsx` (+ test)

**Interfaces:**
- Consumes: `groupRevisions`/`dayLabel`/`versionWordDelta` (Task 8), `nameVersion` + `ui.nameVersionFor` (Task 4), `relativeTime`/`absoluteTime` (`formatRevision.ts`).
- Produces: `HistoryList` props become `{ revisions: RevisionEx[] | null; loading: boolean; onView(id: string): void; onRestore(id: string): void; onName(id: string): void }` (named-only filter + disclosure state internal). `NameVersionDialog` props `{ open: boolean; onOpenChange(open: boolean): void; onName(name: string): void }`.

- [ ] **Step 1: Write the failing tests**

Rewrite `HistoryList.test.tsx`:

```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { HistoryList } from "./HistoryList";
import type { RevisionEx } from "../../client/contractExt";

const NOW_SECS = Math.floor(Date.now() / 1000);
const noop = () => {};
const revs: RevisionEx[] = [
  {
    id: "c3",
    message: 'Edit "roadmap" (+4/−1 words)',
    author: "a",
    timestamp_secs: NOW_SECS - 60,
  },
  {
    id: "c2",
    message: "Draft done",
    author: "a",
    timestamp_secs: NOW_SECS - 120,
    is_named: true,
    name: "Draft 1",
  },
  // > 30 min older: separate session, same day
  { id: "c1", message: "start", author: "a", timestamp_secs: NOW_SECS - 4000 },
];

describe("HistoryList (Versions browser)", () => {
  it("groups by day with relative headers and collapses sessions", async () => {
    render(
      <HistoryList revisions={revs} loading={false} onView={noop} onRestore={noop} onName={noop} />,
    );
    expect(screen.getByText("Today")).toBeInTheDocument();
    // c3 and c2 share a session: head visible, rest behind a disclosure
    expect(screen.getByText('Edit "roadmap" (+4/−1 words)')).toBeInTheDocument();
    expect(screen.queryByText("Draft done")).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /1 more/i }));
    expect(screen.getByText("Draft done")).toBeInTheDocument();
    // c1 is its own session head, visible without disclosure
    expect(screen.getByText("start")).toBeInTheDocument();
  });

  it("filters to named versions only", async () => {
    render(
      <HistoryList revisions={revs} loading={false} onView={noop} onRestore={noop} onName={noop} />,
    );
    await userEvent.click(screen.getByLabelText(/named only/i));
    expect(screen.getByText("Draft done")).toBeInTheDocument();
    expect(screen.queryByText("start")).not.toBeInTheDocument();
    expect(screen.getByText("Draft 1")).toBeInTheDocument(); // the name badge
  });

  it("offers Name… on a row", async () => {
    const onName = vi.fn();
    render(
      <HistoryList revisions={[revs[2]]} loading={false} onView={noop} onRestore={noop} onName={onName} />,
    );
    await userEvent.click(screen.getByRole("button", { name: /name…/i }));
    expect(onName).toHaveBeenCalledWith("c1");
  });
});
```

```tsx
// web/src/components/history/NameVersionDialog.test.tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { NameVersionDialog } from "./NameVersionDialog";

describe("NameVersionDialog", () => {
  it("names the version and closes", async () => {
    const onName = vi.fn();
    const onOpenChange = vi.fn();
    render(<NameVersionDialog open onOpenChange={onOpenChange} onName={onName} />);
    await userEvent.type(screen.getByPlaceholderText(/e\.g\./i), "Draft 1");
    await userEvent.click(screen.getByRole("button", { name: "Name version" }));
    expect(onName).toHaveBeenCalledWith("Draft 1");
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("disables submit when empty", () => {
    render(<NameVersionDialog open onOpenChange={() => {}} onName={() => {}} />);
    expect(screen.getByRole("button", { name: "Name version" })).toBeDisabled();
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `pnpm test -- --run src/components/history`
Expected: FAIL.

- [ ] **Step 3: Implement**

`NameVersionDialog.tsx` (mirrors the deleted CommitDialog's Modal/Input/Button structure):

```tsx
import { useState } from "react";
import { Modal } from "../ui/Modal";
import { Button } from "../ui/Button";
import { Input } from "../ui/Input";

/** "Name this version" — the only manual act in the Versions model, optional
 *  and retroactive (Google-Docs named milestones). */
export function NameVersionDialog({
  open,
  onOpenChange,
  onName,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onName: (name: string) => void;
}) {
  const [name, setName] = useState("");
  const close = () => {
    setName("");
    onOpenChange(false);
  };
  const submit = () => {
    const n = name.trim();
    if (!n) return;
    onName(n);
    close();
  };
  return (
    <Modal open={open} onClose={close} title="Name this version" description="Named versions stand out in the Versions browser.">
      <Input
        autoFocus
        placeholder="e.g. Draft 1"
        value={name}
        onChange={(e) => setName(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") submit();
        }}
      />
      <div className="mt-3 flex justify-end gap-2">
        <Button variant="ghost" onClick={close}>
          Cancel
        </Button>
        <Button variant="primary" disabled={!name.trim()} onClick={submit}>
          Name version
        </Button>
      </div>
    </Modal>
  );
}
```

`HistoryList.tsx` rewrite:

```tsx
import { useState } from "react";
import type { RevisionEx } from "../../client/contractExt";
import { groupRevisions } from "./groupRevisions";
import { versionWordDelta } from "./versionSummary";
import { relativeTime, absoluteTime } from "./formatRevision";
import { Button } from "../ui/Button";

function Row(props: {
  r: RevisionEx;
  onView: (id: string) => void;
  onRestore: (id: string) => void;
  onName: (id: string) => void;
}) {
  const { r } = props;
  const delta = versionWordDelta(r);
  return (
    <div className="flex flex-col gap-0.5 rounded px-1.5 py-1 hover:bg-surface-2">
      <div className="flex items-center gap-1.5">
        <span className={"min-w-0 flex-1 truncate " + (r.is_named ? "font-semibold text-text" : "text-text")}>
          {r.message}
        </span>
      </div>
      <div className="flex items-center gap-1.5 text-xs text-muted">
        {r.is_named && r.name && (
          <span className="rounded bg-accent/15 px-1 text-accent">{r.name}</span>
        )}
        <span title={absoluteTime(r.timestamp_secs)}>{relativeTime(r.timestamp_secs)}</span>
        {delta && (
          <span>
            +{delta.added}/&minus;{delta.removed} words
          </span>
        )}
        <span className="grow" />
        <Button variant="ghost" onClick={() => props.onView(r.id)}>
          View
        </Button>
        <Button variant="ghost" onClick={() => props.onRestore(r.id)}>
          Restore
        </Button>
        <Button variant="ghost" onClick={() => props.onName(r.id)}>
          Name…
        </Button>
      </div>
    </div>
  );
}

export function HistoryList(props: {
  revisions: RevisionEx[] | null;
  loading: boolean;
  onView: (id: string) => void;
  onRestore: (id: string) => void;
  onName: (id: string) => void;
}) {
  const [namedOnly, setNamedOnly] = useState(false);
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  if (props.loading || props.revisions === null)
    return <div className="p-2 text-sm text-muted">Loading…</div>;
  if (props.revisions.length === 0)
    return <div className="p-2 text-sm text-muted">No versions yet.</div>;

  const revs = namedOnly ? props.revisions.filter((r) => r.is_named) : props.revisions;
  const days = groupRevisions(revs, Date.now() / 1000);
  const toggle = (id: string) =>
    setExpanded((s) => {
      const next = new Set(s);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  return (
    <div className="flex flex-col gap-1 text-sm">
      <label className="flex items-center gap-2 px-1.5 text-xs text-muted">
        <input
          type="checkbox"
          checked={namedOnly}
          onChange={(e) => setNamedOnly(e.target.checked)}
        />
        Named only
      </label>
      {days.map((day) => (
        <div key={day.label} className="flex flex-col gap-0.5">
          <div className="px-1.5 pt-1 text-xs font-medium uppercase tracking-wide text-faint">
            {day.label}
          </div>
          {day.sessions.map((session) => (
            <div key={session.head.id} className="flex flex-col gap-0.5">
              <Row r={session.head} onView={props.onView} onRestore={props.onRestore} onName={props.onName} />
              {session.rest.length > 0 && !expanded.has(session.head.id) && (
                <button
                  type="button"
                  className="self-start px-1.5 text-xs text-muted hover:text-text"
                  onClick={() => toggle(session.head.id)}
                >
                  {session.rest.length} more in this session…
                </button>
              )}
              {expanded.has(session.head.id) &&
                session.rest.map((r) => (
                  <Row key={r.id} r={r} onView={props.onView} onRestore={props.onRestore} onName={props.onName} />
                ))}
            </div>
          ))}
        </div>
      ))}
    </div>
  );
}
```

Wiring:
- `HistoryPane.tsx`: pass `onName={(id) => actions.setUi({ nameVersionFor: id })}` (add `useActions` if not present); adjust `onView`/`onRestore` to the id-based signatures (they already call `viewRevision(r.id)`-style handlers — adapt the mapping, behavior unchanged). Cast `history` to `RevisionEx[] | null` at the prop site (`s.history as RevisionEx[] | null` — fields are optional so this is safe pre-C0).
- `DialogHost.tsx`: mount

```tsx
      <NameVersionDialog
        open={ui.nameVersionFor !== null}
        onOpenChange={(o) => {
          if (!o) actions.setUi({ nameVersionFor: null });
        }}
        onName={(name) => {
          const id = ui.nameVersionFor;
          if (id) void actions.nameVersion(id, name);
        }}
      />
```

- `RightAside.tsx`: tab label `"History"` → `"Versions"` (keep the internal `"history"` tab id — persisted state and routes reference it). Update any test asserting the label (`grep -rn '"History"' web/src`).
- `commands.ts`: `show-history` label `"Show note history"` → `"Show versions"` (id unchanged — user keybinding overrides are keyed by id).
- `StatusBar.tsx`: extend the last-version span using Task 8's helper:

```tsx
import { versionWordDelta } from "./history/versionSummary";
// inside the lv && (...) span, after the relativeTime call:
{(() => {
  const d = versionWordDelta(lv);
  return d && d.added > 0 ? ` · +${d.added} words` : "";
})()}
```

- `HistoryPane.test.tsx`: keep; update if the pane's prop plumbing changed signatures.

- [ ] **Step 4: Run the full suite**

Run: `pnpm test -- --run && pnpm lint`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A web/src
git commit -m "feat(versions): grouped Versions browser with named milestones (U4)"
```

---

### Task 10: Seal-now hints (U5)

**Files:**
- Create: `web/src/app/useSealHints.ts`
- Modify: `web/src/app/App.tsx`
- Test: `web/src/app/useSealHints.test.tsx`

**Interfaces:**
- Consumes: `sealNow()` (Task 4), `cairnStore` vanilla `subscribe(listener(state, prev))`.
- Produces: `useSealHints(): void` React hook, mounted once in `App`.

- [ ] **Step 1: Write the failing test**

```tsx
// web/src/app/useSealHints.test.tsx
import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook } from "@testing-library/react";
import { cairnStore } from "./cairnStore";
import { useSealHints } from "./useSealHints";

describe("useSealHints", () => {
  beforeEach(() => {
    cairnStore.setState({ activePath: "a.md" });
  });

  it("seals on note switch", () => {
    const seal = vi.fn().mockResolvedValue(undefined);
    cairnStore.setState({ sealNow: seal });
    renderHook(() => useSealHints());
    cairnStore.setState({ activePath: "b.md" });
    expect(seal).toHaveBeenCalledTimes(1);
  });

  it("seals on window blur", () => {
    const seal = vi.fn().mockResolvedValue(undefined);
    cairnStore.setState({ sealNow: seal });
    renderHook(() => useSealHints());
    window.dispatchEvent(new Event("blur"));
    expect(seal).toHaveBeenCalledTimes(1);
  });

  it("does not seal when the path did not change", () => {
    const seal = vi.fn().mockResolvedValue(undefined);
    cairnStore.setState({ sealNow: seal });
    renderHook(() => useSealHints());
    cairnStore.setState({ query: "unrelated" });
    expect(seal).not.toHaveBeenCalled();
  });
});
```

(If the shared test store setup resets state between tests differently, follow the file-local convention used by `useCommands.test.tsx` for store priming.)

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test -- --run src/app/useSealHints.test.tsx`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement**

```ts
// web/src/app/useSealHints.ts
import { useEffect } from "react";
import { cairnStore } from "./cairnStore";

/** U5 seal-now hints: ask the engine to seal a version when attention moves —
 *  note switch or window blur. The engine owns the policy (idle gap, backstop,
 *  skip-no-op); these are only boundary hints and are safe to over-send. */
export function useSealHints() {
  useEffect(() => {
    const unsub = cairnStore.subscribe((s, prev) => {
      if (s.activePath !== prev.activePath && prev.activePath !== null)
        void cairnStore.getState().sealNow();
    });
    const onBlur = () => void cairnStore.getState().sealNow();
    window.addEventListener("blur", onBlur);
    return () => {
      unsub();
      window.removeEventListener("blur", onBlur);
    };
  }, []);
}
```

In `App.tsx`, after `useGlobalKeys(chordMap, runCommand);` add `useSealHints();` (+ import).

- [ ] **Step 4: Run tests**

Run: `pnpm test -- --run src/app && pnpm test -- --run`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/app
git commit -m "feat(versions): seal-now hints on note switch and window blur (U5)"
```

---

### Task 11: Full gate + live QA + PR

- [ ] **Step 1: Full local gate (CI parity — prettier check included)**

Run from `web/`: `pnpm lint && pnpm format:check && pnpm test -- --run && pnpm build`
(If the repo root `justfile` has a web gate target — check `just --list` — prefer it.)
Expected: all green. Fix anything before claiming done.

- [ ] **Step 2: Manual QA vs MockClient (`pnpm dev`, browser)**

- No Commit button / dialog anywhere; ⌘⏎ does nothing; palette has no "Commit changes…".
- Tab ● dot still appears while typing and clears after autosave.
- StatusBar: `✓ Saved · Synced · 🕘 Versions`; edit a note, switch notes → "Last version: just now" appears (mock seal fired).
- Versions tab (RightAside): grouped rows with summaries; "Name…" → named badge + "Named only" filter works.
- Settings: no Auto-commit section.

- [ ] **Step 3: Live daemon QA (presence axes — engine C0 not required)**

```sh
/path/to/cairn/target/debug/cairn-daemon --cairn /tmp/cairn-demo-live \
  --port 7777 --cors-origin http://localhost:5173
# browser console @ localhost:5173:
localStorage.setItem('cairn.daemon', JSON.stringify({url:'http://localhost:7777',
  token:'<from /tmp/cairn-demo-live/.cairn/token>'})); location.reload()
# concurrent edit from a terminal:
cairn --cairn /tmp/cairn-demo-live write welcome.md "..."
```

Verify: PresenceCluster shows "Connected" → "Someone is editing…" on the foreign write; kill the daemon → "Reconnecting…" then "Offline · Reconnect"; StatusBar sync axis mirrors it. With a dirty buffer + foreign write → conflict chip → calm dialog → "See their version" shows the diff read-only; "Keep my version" leaves the buffer.
**Known pre-C0 limitation (document in the PR):** seal-now hints against today's daemon fail silently (Commit still requires `message` server-side) — sealing e2e lands at Phase-3 integration when engine C0 ships. Use vanilla Chrome for browser automation (Arc hangs).

- [ ] **Step 4: Push + PR**

```bash
git push
gh pr create --base main --title "feat: presence cluster + human Versions (UI track)" \
  --body "<summary; note MockClient-first C0 seam + Phase-3 integration follow-up>"
```

Merge via merge queue ("Merge when ready").

---

## Self-review notes (spec → tasks)

- Spec Part 1 states table → Task 7 (all seven states incl. degradation); conflict flow → Tasks 6+7; peers[] growth seam → Task 6.
- Spec Part 2: commit UI removal → Tasks 3+4; two-layer save/version split → Layer 1 untouched (global constraint), Layer 2 = engine (out of repo) + hints Task 10; deterministic labels → engine-owned, mock reference impl Task 2; named versions → Tasks 2/4/9; Versions browser → Tasks 8+9; StatusBar → Tasks 5+9.
- Contract seam C0 → Tasks 1+2 (host-copy, never touching `web/src/contract`).
- Out of scope honored: no CRDT, no awareness channel, no engine changes, no retroactive fold.
- Type consistency: `RevisionEx`/`asCommand` (Task 1) used verbatim in Tasks 2/4/5/8/9; `CollabPeer` (Task 6) in Task 7; `sealNow`/`nameVersion` (Task 4) in Tasks 9/10; `SyncStatus` in Tasks 5/7 shares the store's `"ok" | "reconnecting" | "down"`.
