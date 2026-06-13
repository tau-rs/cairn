# Plugin content hashing — pin a trusted dir's bytes, refuse on drift

**Issue:** #40, item A — *Manifest signing / content hashing pinned in the
trusted list (detect a trusted dir whose contents changed)*. Follow-up to #39
(audit finding S3), the default-deny plugin trust gate.

**Builds on:** `docs/superpowers/specs/2026-06-11-cairn-plugin-trust-design.md`
(the trust gate this extends).

## Problem

#39 added a default-deny allowlist: a plugin under `<cairn>/.cairn/plugins/<dir>`
is spawned only if `<dir>` is in the `[plugins] trusted` set, and only if its
`manifest.id` equals the directory name. But trust is anchored on the directory
*name* alone. Once a name is trusted, **any** bytes that later appear under that
directory are spawned with the user's full privileges. An attacker (or an
errant sync, a compromised update channel, a careless `git pull`) who can write
into an already-trusted plugin dir can swap the binary the manifest points at,
or any file that binary reads, with no further approval.

This session pins a **content hash** per trusted directory. A trusted dir whose
bytes drift from the pinned hash is **refused** — trust now means "these
specific bytes," not "this name."

## Scope

Add, per trusted directory, an optional pinned hash of the directory's full
contents. At load time, after the existing trust-gate check and before spawn,
hash the directory tree and compare:

- **pinned + matches** → spawn (as today)
- **pinned + differs** → refuse, distinct log (drift detected)
- **unpinned** → spawn, but warn with the exact `hash = "..."` line to paste
- **tree contains a symlink / is unreadable** → refuse, distinct log

Out of scope (named, not built here):

- An **absolute** `command` path escapes the hashed directory; pinning it is a
  different problem. See *Known limitations*. Addressed by the sandbox
  follow-up or a separate "reject absolute commands" rule.
- Cryptographic *signing* (a third party vouching for bytes) — this is bare
  content integrity, not authenticity. The issue title says "signing /
  hashing"; we build hashing.
- Auto-writing pins back into `cairn.toml` (trust-on-first-use). The host reads
  policy; it does not author it. See *Migration*.

## Design

### Hash scope — whole directory tree

The pin covers **every regular file** under `<cairn>/.cairn/plugins/<dir>/`,
recursively. Rationale: it is the scope a user can reason about without knowing
our internals ("did any file in the plugin change? then it is not the plugin I
approved"), and it has no gaps *within* the managed directory — manifest,
binary, bundled scripts, data files all drift the hash.

Non-regular entries are a refusal, not a follow:

- A **symlink** anywhere in the tree → refuse to spawn. Following symlinks
  re-opens the directory-escape hole this feature exists to close (a symlink
  could point the hash — or the spawned binary — at bytes outside the dir).
- Other non-regular files (fifos, sockets, devices) → refuse, same reasoning.
- Empty subdirectories contribute nothing (only files are hashed).

### Algorithm — SHA-256

SHA-256, via the `sha2` crate. Chosen over BLAKE3 because throughput is
irrelevant for a handful of small files, and SHA-256 is the more recognizable
value for a human eyeballing `cairn.toml`. The stored pin carries an explicit
algorithm prefix (below) so the choice is not baked in irreversibly.

### Construction — canonical, framed, platform-stable

Compute one SHA-256 over a canonical serialization of the tree:

```
collect every regular file as (relative_path, bytes)
normalize each relative_path to use `/` separators
sort by relative_path, byte order
for each file in sorted order:
    hash.update(relative_path_bytes)
    hash.update([0x00])                  // domain separator; 0x00 cannot appear in a path
    hash.update((bytes.len() as u64).to_le_bytes())
    hash.update(bytes)
pin = "sha256:" + lower_hex(hash.finalize())   // 64 hex chars after the prefix
```

Three properties this guarantees, each load-bearing:

1. **Framing (separator + length prefix) is a correctness requirement, not
   style.** Without it, two different trees can produce the same byte stream —
   e.g. moving bytes between a path and a file's contents. The `0x00` separator
   and `u64` length prefix make the serialization unambiguous.
2. **`/`-normalized, sorted paths** make the hash platform-stable: a plugin
   approved on macOS and the same bytes on Windows produce the same pin.
   Consistent with the recent Windows-path hardening (commit `bb2028f`).
3. **The `sha256:` prefix is part of the stored value.** A future change to the
   algorithm or construction becomes a new prefix (`sha256v2:`, `blake3:`),
   surfaced as a mismatch the user can act on — never a silent wrong-compare.
   The construction is therefore a **stability contract**: once a pin exists in
   a user's config, the framing above cannot change under the `sha256:` prefix.

### Schema — array of tables, optional hash, back-compat with bare strings

`cairn.toml` `[plugins]` today:

```toml
[plugins]
trusted = ["my-plugin", "another-plugin"]
```

becomes:

```toml
# Pinned: spawned only if the directory's contents hash to exactly this value.
[[plugins.trusted]]
dir  = "my-plugin"
hash = "sha256:1f3a…"

# Unpinned: trusted by name, no pin yet — spawns with a warning telling you the
# line to paste to pin it.
[[plugins.trusted]]
dir  = "another-plugin"

# Legacy form still parses → trusted, unpinned. Non-breaking upgrade.
# trusted = ["legacy-plugin"]
```

Each element of `trusted` is parsed as an **untagged** `String | Table`:

- a **string** `"name"` → `{ dir: "name", hash: None }` (legacy + ergonomic
  shorthand for "trust by name, pin later");
- a **table** `{ dir, hash? }` → as written.

This makes the schema migration and the "no pin yet" state the *same*
mechanism: an unpinned entry is either a bare string or a table with `hash`
omitted, and both mean exactly "trusted, unpinned."

Array of tables (not an inline `dir = hash` map) because:

- the "unpinned" state is a naturally-absent optional field, not an ugly
  sentinel like `""`;
- the entry is already a table, so the other #40 follow-ups (surface declared
  capabilities, interactive approval) can add fields without a third schema
  migration — a config schema is a stability contract and re-breaking it is
  expensive;
- weird directory names (dots, spaces) are always a quoted `dir = "..."`, never
  a fragile bare key.

### API shape

```rust
// crates/cairn-infra/src/plugin_host.rs

/// "sha256:<64 lowercase hex>". Newtype so a pin can't be confused with an
/// arbitrary string and so parse/format live in one place.
pub struct PinnedHash(String);

impl PinnedHash {
    /// Parse a stored pin; rejects unknown prefixes and malformed hex.
    pub fn parse(s: &str) -> Result<Self, ...>;
    /// Hash a plugin directory tree per the canonical construction.
    /// Errors on symlink / non-regular file / IO error.
    pub fn of_dir(plugin_dir: &Path) -> Result<Self, ...>;
}

/// dir_name -> optional pinned hash. `None` = trusted-but-unpinned.
pub struct TrustedPlugins(HashMap<String, Option<PinnedHash>>);

impl TrustedPlugins {
    pub fn none() -> Self;                 // default-deny (unchanged semantics)
    /// Look up trust + pin for a directory name. `None` outer = not trusted.
    pub fn get(&self, dir_name: &str) -> Option<&Option<PinnedHash>>;
}
```

`TrustedPlugins` changes from `HashSet<String>` to a map carrying the optional
pin. `load` / `load_with_timeout` signatures are unchanged (they already take
`&TrustedPlugins`). The directory-name trust gate and the `manifest.id ==
dir_name` check are unchanged and still run first.

### Spawn-time flow

In `load_with_timeout`, per directory entry (changes in **bold**):

1. Resolve `dir_name`; if non-UTF-8/empty → skip (unchanged).
2. Look up `dir_name` in `trusted`. Not present → skip + existing log, **no
   read** (unchanged — untrusted bytes never reach our code).
3. **Hash the directory tree (`PinnedHash::of_dir`). On symlink / non-regular /
   IO error → refuse + distinct log, continue.**
4. **Match the pin:**
   - **pinned, computed == pin** → proceed to spawn.
   - **pinned, computed != pin** → refuse + distinct drift log, continue.
   - **unpinned** → proceed to spawn, but `eprintln!` the computed
     `hash = "sha256:…"` line to paste.
5. `spawn_plugin` (unchanged): read manifest, assert `manifest.id == dir_name`,
   spawn.

The hash is computed **after** the trust-gate check, so untrusted directories
are still never read. It is computed **before** `spawn_plugin`, so a tampered
trusted dir never spawns.

## Data flow

```
cairn.toml [[plugins.trusted]] (untagged String | {dir, hash?})
        │  (cairn-daemon/config.rs: Vec<TrustedEntry>)
        ▼
TrustedPlugins (dir_name -> Option<PinnedHash>)
        │  (cairn-daemon/main.rs)
        ▼
ProcessPluginHost::load_with_timeout(dir, timeout, &trusted)
        │  per directory entry
   trusted? ──no──► skip + log (no read)
        │ yes
   hash tree ──symlink/non-regular/IO──► refuse + log
        │ ok
   pinned? ──yes──► computed == pin? ──no──► refuse + drift log
        │ no                  │ yes
   spawn + warn          spawn
   (show computed pin)
```

## Error handling / logs

All non-fatal; load continues to the next directory (matches the existing
gate's per-dir tolerance):

- **Drift:** `plugin: refusing <dir>: contents changed (pinned <pin>, found
  <computed>); re-approve by updating hash in cairn.toml`.
- **Symlink / non-regular:** `plugin: refusing <dir>: contains a symlink or
  non-regular file (<path>); not hashing or spawning`.
- **Unpinned trusted:** `plugin: <dir> is trusted but unpinned; pin it by
  setting hash = "<computed>" in cairn.toml`.
- **Malformed stored pin** (unknown prefix / bad hex in config): surfaced at
  config-load as a parse error (fail fast — a typo'd pin must not silently
  degrade to "unpinned").

## Migration — "no pin yet"

Three populations collapse to one **trusted-but-unpinned** state, handled
uniformly by step 4's *unpinned* arm (spawn + warn, never auto-write):

1. **Legacy `trusted = ["x"]` string configs** — parse via the untagged String
   arm to `{ dir: "x", hash: None }`. Upgrade is non-breaking.
2. **New table entries with `hash` omitted** — `hash: None`.
3. **A freshly-added plugin the user hasn't pinned** — same.

Drift enforcement applies *only* once a hash is present. Rejected alternatives:

- **Trust-on-first-use (auto-pin):** records whatever is on disk at first run
  as canonical — including an attacker's tampered version if they struck first.
  A TOFU pin means "verified against whatever happened to be there," a weaker
  guarantee wearing a strong one's clothes. Also mutates the user's config
  behind their back. Rejected.
- **Default-deny on unpinned:** breaks every legacy config on upgrade and forces
  a two-step dance (run → see rejection + computed hash → paste → run) identical
  to the spawn+warn flow minus the convenience of the first run working.
  Rejected.

## Testing (TDD)

Host-level, against a real plugin directory (extend
`crates/cairn-plugin-example/tests/host.rs` and unit tests in
`crates/cairn-infra`):

1. **Pinned + matching hash spawns.** Compute the example dir's hash, pin it,
   load → plugin present.
2. **Pinned + drifted hash refuses.** Pin a value, mutate a file under the dir,
   load → plugin absent.
3. **Unpinned trusted spawns.** No hash, valid dir → plugin present (warning
   path; assert presence, optionally assert log).
4. **Symlink in tree refuses.** Add a symlink under a trusted dir → plugin
   absent.
5. **Untrusted dir still skipped pre-read.** Existing test stays green; a
   malformed manifest in an untrusted dir is still never parsed.

Construction unit tests (`cairn-infra`):

6. **Determinism:** same tree hashed twice → identical pin.
7. **Path-order independence:** files created in different order → identical pin
   (sorting works).
8. **Framing prevents collision:** two trees that differ only in where a
   boundary falls between a filename and file content → different pins.
9. **Platform-stable separators:** a path with backslashes normalizes to `/`
   before hashing (unit-testable without Windows).
10. **`PinnedHash::parse`:** accepts `sha256:<64 hex>`; rejects unknown prefix,
    wrong length, non-hex.

Config tests (`cairn-daemon/config.rs`):

11. Table form `[[plugins.trusted]] dir=… hash=…` parses with the pin.
12. Table form with `hash` omitted → `hash: None`.
13. Legacy `trusted = ["a","b"]` parses → two unpinned entries.
14. Malformed `hash` value → config parse error.

## Known limitations (documented, not solved here)

- **Absolute `command` path.** `spawn_plugin` allows an absolute
  `manifest.engine.command`, which lives outside the hashed directory; its bytes
  are not pinned. Pinning arbitrary absolute paths is a separate problem;
  closed by the sandbox follow-up or a future "reject absolute commands" rule.
  Noted here so the gap is explicit, not assumed-covered.
- **TOCTOU.** The hash is computed, then the binary is spawned, as two steps; a
  sufficiently privileged local attacker could swap bytes in between. Out of
  scope for a content pin (the sandbox follow-up is the real mitigation); noted
  for honesty.

## Follow-ups (remaining #40 items, not this PR)

- OS-level sandbox for the spawned child (seccomp / landlock / sandbox-exec).
- Interactive first-run approval + surface declared capabilities to the user.
- Docs: state explicitly that an approved, pinned plugin is still fully trusted
  code — the pin guarantees integrity, not safety.
