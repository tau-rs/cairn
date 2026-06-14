# Plugin Linux Sandbox — Design

> Follow-up to #40 / #61. macOS Seatbelt backend landed in #60
> (`docs/superpowers/specs/...` + `crates/cairn-infra/src/sandbox.rs`). This
> design adds the Linux backend.

## Goal

Jail trusted plugins in an OS sandbox on Linux so a spawned plugin's direct
filesystem-write, network access, and reads of the user's vault are denied by
the kernel — and refuse to spawn anywhere a working jail cannot be applied.
Behaviorally equivalent to the macOS backend, implemented through the existing
`Sandbox` port with **no port changes, no new Rust dependencies, and no
`unsafe`** (the workspace sets `unsafe_code = "forbid"`).

## Mechanism: bubblewrap (decision)

We confine via [`bubblewrap`](https://github.com/containers/bubblewrap)
(`bwrap`), an external sandbox launcher, rather than the `landlock` + `seccomp`
crates or `birdcage`/`extrasafe`. Rationale:

- **Mirrors the established pattern.** The macOS backend wraps the plugin
  command with an external launcher (`sandbox-exec -p <profile> -- <cmd>`).
  `bwrap … -- <cmd>` is the exact Linux analogue, so the `wrap() -> Command`
  port shape is preserved unchanged.
- **Honors `forbid(unsafe_code)`.** A hand-rolled `landlock`/`seccomp`
  `pre_exec` closure needs an `unsafe` carve-out overriding the workspace lint.
  `bwrap` needs none.
- **No new dependency surface.** Adding a security-critical crate to a security
  feature is the wrong trade; `bwrap` is a runtime dependency, not a linked
  one, and absence is handled by refusing (see below).

This is a deliberate deviation from issue #61's "Landlock + seccomp" title,
accepted during brainstorming.

### Trade-off: exec-denial

The macOS profile denies further `exec` (`deny process-exec*`). `bwrap` alone
cannot block `execve` — that needs a seccomp BPF filter, which would reintroduce
the crate/`unsafe` question. We **accept namespace inheritance instead**: any
process the plugin execs inherits the same `bwrap` namespace and is therefore
equally confined (no writes, no network, no vault). The guarantee is "anything
that runs is jailed," not "nothing else runs." This is documented as a
deliberate, justified difference from the macOS backend.

### Trade-off: read breadth

The jail exposes a **broad read-only root** (`--ro-bind / /`), matching the
macOS profile's "allow file-read* broadly, deny the vault" policy. Tightening
non-vault reads to a minimal allowlist is explicitly the scope of issue #63
(capability-derived profiles) and is **out of scope here** — doing it now would
make Linux stricter than macOS and duplicate #63's work.

## Architecture

All changes land in `crates/cairn-infra/src/sandbox.rs`, alongside
`MacSeatbeltSandbox`. The `Sandbox` port in `cairn-ports`, the
`ProcessPluginHost` in `plugin_host.rs` (already threads `&dyn Sandbox`), and
the daemon (already calls `platform_sandbox()`) are **untouched**.

### `LinuxBwrapSandbox`

```rust
pub struct LinuxBwrapSandbox {
    /// Path to the `bwrap` binary (overridable in tests). Default `/usr/bin/bwrap`.
    exec: PathBuf,
    /// Cached result of the one-time userns probe (see `wrap`).
    probe: std::sync::OnceLock<Result<(), String>>,
}
```

- `Default` → `exec = /usr/bin/bwrap`.
- `with_exec(PathBuf)` for tests (mirrors `MacSeatbeltSandbox::with_exec`).

### Pure argv builder

```rust
pub(crate) fn bwrap_args(vault_root: &Path, plugin_dir: &Path, cmd: &Path)
    -> Vec<OsString>
```

The testable analogue of `seatbelt_profile`. Produces, in order:

```
--ro-bind / /                          # broad read-only root  (≈ allow file-read*)
--tmpfs   <vault_root>                 # mask the vault        (≈ deny file-read* vault)
--ro-bind <plugin_dir> <plugin_dir>    # re-expose the plugin's own dir
--dev /dev                             # minimal device nodes (null/zero/random/...)
--proc /proc                           # minimal procfs
--unshare-all                          # new user/pid/ipc/uts/net ns → no network
--die-with-parent                      # jail dies with the host
--                                     # end of bwrap flags
<cmd> <args...>
```

Ordering is significant: `--ro-bind / /` first, then `--tmpfs` masks the vault
(including the plugin dir, which lives under it), then `--ro-bind <plugin_dir>`
re-exposes it on top. `bwrap` processes mount ops left-to-right and creates bind
destinations automatically.

Paths are emitted as distinct `OsString` argv entries, so — unlike the SBPL
profile — **no quoting and no `to_string_lossy` are needed**; non-UTF-8 paths
pass through intact. (The `sbpl_quote` doc comment's warning against reuse on
Linux is honored by not reusing it.)

### `wrap()`

Mirrors the macOS adapter's structure:

1. **Existence check** — `self.exec.exists()` is false → `SandboxError::Unavailable`.
2. **One-time userns probe** — via `OnceLock::get_or_init`, run
   `bwrap --ro-bind / / --unshare-all -- /bin/true`. A non-zero exit or spawn
   failure caches and returns `Unavailable("user namespaces unavailable: …")`.
   This makes a userns-disabled host produce the same clean refusal the macOS
   path gives, instead of a confusing later spawn error. The probe result is
   cached per instance (keyed implicitly to that instance's `exec`).
3. **Canonicalize** `vault_root`, `plugin_dir`, `cmd` (each
   `canonicalize()` error → `Unavailable`), as the macOS adapter does.
4. **Build** `Command::new(&self.exec).args(bwrap_args(...))`. Stdio is left
   unconfigured — the caller wires it (unchanged contract).

### Factory

```rust
pub fn platform_sandbox() -> Box<dyn Sandbox> {
    #[cfg(target_os = "macos")]   { Box::new(MacSeatbeltSandbox::default()) }
    #[cfg(target_os = "linux")]   { Box::new(LinuxBwrapSandbox::default()) }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
                                  { Box::new(RefusingSandbox) }
}
```

`RefusingSandbox` now covers only non-macOS, non-Linux targets (Windows is #62).

## Confinement guarantee

Equivalent — not byte-identical — to the macOS backend:

| Capability        | macOS (Seatbelt)        | Linux (bwrap)                         |
|-------------------|-------------------------|---------------------------------------|
| Read filesystem   | broad, vault denied     | broad (`--ro-bind / /`), vault tmpfs'd |
| Read plugin dir   | allowed                 | allowed (re-bound)                    |
| Write filesystem  | denied                  | denied (everything read-only)         |
| Network           | denied                  | denied (`--unshare-all`)              |
| Further `exec`    | **denied**              | **allowed but equally jailed** (inherits ns) |

## Error handling

Reuse `SandboxError::Unavailable(String)` — no new variants. `thiserror` already
defines it at the port boundary.

## Testing

Mirrors the macOS suite in the same module.

**Pure (any platform):**
- `bwrap_args` contains the expected flags and the three paths in the correct
  order (no spawn).

**`#[cfg(target_os = "linux")]`, skipped if the probe reports unavailable:**
- Deny write outside the jail: `touch <escaped>` fails, file not created.
- Allow exec + pipe stdout: `echo hi` → `b"hi\n"`.
- Deny vault read but allow plugin-dir read: `cat <vault>/secret.md` fails;
  `cat <plugin_dir>/own.txt` → `b"OWN"`.
- `Unavailable` when `bwrap` is missing (`with_exec` → nonexistent path).

**CI:** add a `sudo apt-get install -y bubblewrap` step guarded
`if: matrix.os == 'ubuntu-latest'` to the `test` job in
`.github/workflows/ci.yml`. GitHub's ubuntu runners permit unprivileged user
namespaces, so the behavioral tests execute rather than skip.

## Out of scope

- Windows backend (#62).
- Capability-derived / tightened-read profiles (#63).
- Blocking `execve` via seccomp (accepted as namespace-inherited confinement).

## Files

- **Modify** `crates/cairn-infra/src/sandbox.rs` — add `LinuxBwrapSandbox`,
  `bwrap_args`, the probe, extend `platform_sandbox()`, update the module doc.
- **Modify** `.github/workflows/ci.yml` — install `bubblewrap` on the Linux
  test job.
