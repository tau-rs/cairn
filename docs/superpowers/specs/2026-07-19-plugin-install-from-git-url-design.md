# Plugin install-from-git-URL — `cairn plugin add <git-url>`

Roadmap slice 7. Adds a CLI to fetch a plugin from a git URL into
`<cairn>/.cairn/plugins/<id>/`, routed through the **existing** default-deny
trust gate and pinned-contents-on-drift model (#39/#40/#57/#58) rather than a
parallel mechanism.

Depends on / does not change: the daemon load gate in
`crates/cairn-infra/src/plugin_host.rs` (`TrustedPlugins` + `PinnedHash`) and the
`cairn.toml [plugins].trusted` config. Read
`docs/superpowers/specs/2026-06-11-cairn-plugin-trust-design.md` for that model.

## Goal

`cairn plugin add <url> [--ref <ref>]` clones a plugin, strips its `.git`, places
the content-only tree under `.cairn/plugins/<id>/`, records provenance in a
sidecar file, and prints a paste-ready approval snippet. A freshly added plugin
lands **untrusted** and does not run until the user approves it by hand in
`cairn.toml`. Re-adding an existing plugin at a new ref changes its bytes and so
trips the existing drift check, forcing re-approval. `list` and `remove` round
out the lifecycle.

## Decisions

- **D1 — `add` never writes `cairn.toml`.** Approval stays a manual paste into
  `[plugins].trusted`. This keeps the audited default-deny gate byte-for-byte
  unchanged and makes drift-forces-reapproval a free consequence rather than new
  logic. No TOML writer is introduced.
- **D2 — Provenance in a sibling sidecar, `list` reads it + checks remote.**
  `.cairn/plugins/<id>.source.toml` (outside the hashed tree) records source URL,
  requested ref, resolved commit, and the install-time content hash. `list`
  reads it and best-effort fetches the remote to show "update available".
- **D3 — Strip `.git`; hash covers content only; update = fresh re-clone.** The
  `PinnedHash::of_dir` construction is left untouched (its module doc marks it a
  stability contract). Keeping `.git` would either fold unstable pack files into
  the hash (`git gc` → phantom drift) or force special-casing the hasher.
- **D4 — `id` = `manifest.id`, read from the clone.** Guarantees the daemon's
  `manifest.id == <dir>` consistency check passes by construction. `--ref`
  defaults to the remote's default-branch HEAD, resolved to a commit. A
  same-source re-add is an update; a same-id/different-source add is a hard error.
- **D5 — Verbs: `add` / `list` / `remove`.** Update = re-run `add` (the sidecar
  already stores the source; a distinct `update` verb would be pure sugar).
  `remove` deletes the dir + sidecar and reminds the user to remove the trust
  line (cairn cannot revoke trust — that lives in the user-owned `cairn.toml`).
- **D6 — Clone over public HTTPS + SSH (ssh-agent → default keys).** Both libs
  are already compiled into `git2 = "0.21"` default features. Private-HTTPS is
  unsupported and yields a clear error pointing at the ssh URL. Token /
  credential-helper support is a fast-follow.

## Section 1 — Architecture & boundaries

```
cairn-cli  Command::Plugin { Add | List | Remove }
   │  (handled BEFORE build_engine — pure fs/git, no engine, no reindex)
   ▼
cairn-infra::plugin_install            ◄── new module, beside plugin_host + plugin_hash
   ├─ install(root, url, ref?) -> Installed
   ├─ list(root, fetch: bool)  -> Vec<InstalledInfo>
   └─ remove(root, id)         -> ()
        │ reuses
        ├─ git2                              (clone @ ref, ls-remote for list)
        ├─ cairn_plugin_protocol::Manifest   (parse manifest.toml, read .id)
        └─ cairn_infra::PinnedHash           (hash the content tree)
   error boundary: thiserror `PluginInstallError`  (CLI maps to String, as today)
```

**Why `cairn-infra`, not a new port + service:** plugin install is pure IO
orchestration (git + fs + hash) with no domain entities and exactly one real
implementation. A port trait would be a one-adapter abstraction with nothing
meaningful to fake — tests exercise real `git2` against **local fixture repos**
(fast, truthful, no network). This matches the existing placement of
`plugin_host`/`plugin_hash` in infra, which the CLI already consumes directly.

The three `plugin_install` functions are handled in `cairn-cli`'s `run()`
**before** `build_engine`: they need only the cairn root, not the engine or the
startup reindex. `needs_startup_reindex` stays `false` for the plugin arm.

## Section 2 — Data model & the three sources of truth

The sidecar sits **beside** the plugin dir (sibling → outside the hashed tree),
so writing it never perturbs the content hash:

```
.cairn/plugins/
├─ bar/                     ← content the daemon hashes live at every load
│  ├─ manifest.toml
│  └─ bar-bin
└─ bar.source.toml          ← cairn-owned provenance  (deny_unknown_fields)
```
```toml
# bar.source.toml
source = "https://github.com/foo/cairn-plugin-bar"
ref    = "v1.2.0"           # ref requested at add time (branch/tag/rev)
commit = "a1b2c3d..."       # exact resolved commit (40 hex)
hash   = "sha256:9f86d0..." # PinnedHash of the content tree at install
```

`deny_unknown_fields` on the sidecar struct for the same typo-safety reason as
`PinnedEntry` in the daemon config: a typo must fail loudly, not silently drop a
field.

**Critical separation — who the daemon trusts:**
```
  install writes ─► bar.source.toml.hash = H   (advisory: powers `list`, display)
  user pastes ────► cairn.toml [[plugins.trusted]] hash = H   (AUTHORITATIVE pin)
  daemon load ────► recompute hash(bar/), compare to CAIRN.TOML's H (never sidecar)
```
The daemon's integrity check stays exactly as audited: it compares live content
against the **user-edited** `cairn.toml` pin. The sidecar is cairn-writable and
therefore only advisory — it seeds the snippet the user pastes and drives
`list`, but is never a trust input. This is why `add` staying out of `cairn.toml`
(D1) is what makes drift → re-approval fall out for free.

The sidecar lives under `.cairn/plugins/` alongside plugin binaries and the
index — local runtime state, handled like the rest of `.cairn`.

## Section 3 — Command behaviors & error taxonomy

**`add`**
```
cairn plugin add <url> [--ref <ref>]

 1. clone url @ ref (default: remote HEAD) → tmpdir     [ssh-agent for ssh URLs]
 2. checkout ref, resolve exact commit
 3. parse tmp/manifest.toml → id = manifest.id           (missing/invalid → error)
 4. dest = .cairn/plugins/<id>/
      dest exists?
        └ sidecar.source == url  → UPDATE (proceed, replace)
        └ else                   → ERROR "id already installed from <other>"
 5. rm -rf tmp/.git             (content-only tree)
 6. move tmp → dest (atomic rename; on failure nothing is left half-written)
 7. hash = PinnedHash::of_dir(dest)      (symlink in tree → error, dest rolled back)
 8. write <id>.source.toml
 9. print: id, ref, commit, hash, UNTRUSTED banner + paste-ready snippet
```

Example:
```
$ cairn plugin add https://github.com/foo/cairn-plugin-bar --ref v1.2.0
cloned bar @ v1.2.0 (a1b2c3d) → .cairn/plugins/bar/
UNTRUSTED — will not run until you approve. Add to cairn.toml:

  [[plugins.trusted]]
  dir  = "bar"
  hash = "sha256:9f86d0..."
```

**`list`** (best-effort network per row; `--offline` skips the remote check)
```
cairn plugin list [--offline]

for each *.source.toml:  live = git ls-remote <source> <ref>
ID   SOURCE                           PINNED   TRUSTED  UPDATE
bar  github.com/foo/cairn-plugin-bar  v1.2.0   yes      v1.3.0 available
baz  github.com/x/cairn-plugin-baz    main     no       up to date
qux  github.com/y/qux                 v0.1.0   yes      unreachable
```
- `PINNED` = ref/commit recorded in the sidecar.
- `TRUSTED` = does `cairn.toml [plugins].trusted` list this id? (read-only peek —
  parsed, never written.)
- `UPDATE` = remote ref's commit vs recorded commit; `unreachable` on network
  failure (never fatal), `up to date`/`<ref> available` otherwise.

**`remove`**
```
cairn plugin remove <id>
  rm -rf .cairn/plugins/<id>/  and  <id>.source.toml
  note: still in cairn.toml [plugins].trusted? remove that line to revoke trust.
```

**Error taxonomy (`PluginInstallError`, thiserror):**
```
CloneFailed{url, source}          network/auth/no-such-ref
                                  (private-HTTPS lands here → message points to ssh URL)
ManifestMissing / ManifestInvalid  no / bad manifest.toml in repo
IdConflict{id, existing_source}   dest exists, different source → hard error
SymlinkInTree{path}               PinnedHash refused a symlink  → dest rolled back
NotInstalled{id}                  remove target has no dir
Io(..)                            fs failure
```
The CLI maps `PluginInstallError` to a `String` error as the existing arms do.

## Section 4 — Update / drift lifecycle

```
t0  add v1.2.0        → dest hash H1, sidecar.hash=H1, print snippet(H1)
t1  user approves     → cairn.toml: dir="bar" hash=H1
t2  daemon load       → hash(bar)=H1 == cairn.toml H1  → SPAWN

t3  add v1.3.0 (re-run, same source)
      → replace dest, new hash H2, sidecar.hash=H2, print snippet(H2)
      → cairn.toml STILL pins H1  (cairn never edits it — D1)
t4  daemon load       → hash(bar)=H2 != cairn.toml H1
      → REFUSE: "contents changed (pinned H1, found H2); re-approve by
                 updating hash in cairn.toml"
t5  user updates cairn.toml H1→H2  → next load spawns v1.3.0
```
No new drift logic — the existing daemon gate reacts to `add` having changed the
bytes while leaving the user-owned pin alone.

## Section 5 — Testing (TDD)

All git tests clone from **local fixture repos** (`git2` clones a `file://` /
path repo — no network):

```
unit (plugin_install.rs)
  ├─ id_comes_from_manifest_not_url
  ├─ sidecar_roundtrip / deny_unknown_fields_rejects_typo
  ├─ id_conflict_different_source_errors
  ├─ same_source_readd_is_update
  ├─ dot_git_is_stripped_from_dest
  ├─ symlink_in_repo_errors_and_rolls_back_dest
  └─ manifest_missing_errors

integration (fixture git repo → install → assert)
  ├─ install_lands_untrusted: after add, load with trusted={} → host.plugins() empty
  ├─ approve_then_spawn: load with trusted={id: sidecar.hash} → spawned
  │     (fixture repo wraps the cairn-plugin-example binary, reusing host harness)
  └─ readd_new_commit_forces_reapproval:
        install H1, load pin=H1 → spawn;
        re-add (new commit) → H2; load pin=H1 → REFUSE (drift)

cli
  └─ Plugin arm parses; remove reminds about cairn.toml; list renders columns
     (network update-check behind --offline so the test is deterministic)
```

**Definition of done:** `cargo test --workspace` + `cargo clippy --locked` +
`cargo fmt --check` all green. No new dependencies (`git2`, `sha2`, `toml`,
`serde` all present); `git add Cargo.lock` if it changes.

## Out of scope (fast-follows)

- Token / credential-helper auth for private HTTPS.
- A distinct `update` verb or `--all` bulk update.
- Cairn writing/rewriting `cairn.toml` (an interactive `plugin trust` command).
- Incremental `git fetch` updates (kept as fresh re-clone per D3).
