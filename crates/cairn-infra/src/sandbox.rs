//! OS-level sandboxing for spawned plugins. macOS uses Seatbelt via
//! `sandbox-exec`; Linux uses bubblewrap via `bwrap`; other platforms refuse
//! (no backend yet — see issues #40, #62).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use cairn_ports::{Sandbox, SandboxError};

/// Quote a path as an SBPL string literal, escaping the characters that would
/// otherwise break the quoted token: `\`, `"`, and the line terminators `\n`
/// and `\r` (a raw newline inside the `-p` profile could truncate or malform a
/// rule). `to_string_lossy` means a non-UTF-8 path has invalid bytes replaced
/// with U+FFFD; the quoted path then won't match on disk, so the plugin is
/// over-restricted (fails to exec) rather than under-restricted — safe, but a
/// reason not to reuse this helper for a future Linux backend without revisiting.
fn sbpl_quote(p: &Path) -> String {
    let s = p.to_string_lossy();
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' | '"' => {
                out.push('\\');
                out.push(c);
            }
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Fix-C SBPL profile: reads are allowed broadly (required on macOS 26+ where
/// the dyld shared cache lives behind the Preboot cryptex and cannot be
/// explicitly listed), but the vault root is denied so plugins cannot directly
/// read the user's notes — they must use the gated host-RPC channel instead.
/// The plugin's own directory is re-allowed after the vault deny so plugins can
/// still read their own bundled files. Write, network, and exec (other than the
/// plugin command itself) remain denied. Rule ordering is last-match-wins.
pub(crate) fn seatbelt_profile(vault_root: &Path, plugin_dir: &Path, cmd: &Path) -> String {
    let vault = sbpl_quote(vault_root);
    let dir = sbpl_quote(plugin_dir);
    let cmd = sbpl_quote(cmd);
    format!(
        "(version 1)\n\
         (deny default)\n\
         (allow process-fork)\n\
         (allow file-read*)\n\
         (deny file-read* (subpath {vault}))\n\
         (allow file-read* (subpath {dir}))\n\
         (deny file-write*)\n\
         (deny network*)\n\
         (deny process-exec*)\n\
         (allow process-exec (literal {cmd}))\n"
    )
}

/// Build the bubblewrap argument vector that jails a plugin command.
///
/// Mounts a broad read-only root (`--ro-bind / /`), masks the vault with an
/// empty tmpfs, re-exposes the plugin's own directory read-only on top, drops
/// the network and other namespaces (`--unshare-all`), and ties the jail's
/// lifetime to the host (`--die-with-parent`).
/// A minimal `/dev` and `/proc` are mounted (`--dev /dev`, `--proc /proc`) so
/// plugin runtimes have the standard device nodes and a process tree.
/// The vector ends with `--` and
/// `cmd`; the caller appends the plugin's own arguments after it.
///
/// Ordering is significant: the vault `--tmpfs` must precede the plugin-dir
/// `--ro-bind` so the re-exposed plugin dir is layered on top of the mask.
///
/// All three paths are expected to be canonical absolute paths. They are
/// emitted as distinct `OsString` argv entries, so no quoting is required and a
/// non-UTF-8 path survives intact.
pub(crate) fn bwrap_args(vault_root: &Path, plugin_dir: &Path, cmd: &Path) -> Vec<OsString> {
    let vault = vault_root.as_os_str().to_os_string();
    let dir = plugin_dir.as_os_str().to_os_string();
    let cmd = cmd.as_os_str().to_os_string();
    vec![
        OsString::from("--ro-bind"),
        OsString::from("/"),
        OsString::from("/"),
        OsString::from("--tmpfs"),
        vault,
        OsString::from("--ro-bind"),
        dir.clone(),
        dir,
        OsString::from("--dev"),
        OsString::from("/dev"),
        OsString::from("--proc"),
        OsString::from("/proc"),
        OsString::from("--unshare-all"),
        OsString::from("--die-with-parent"),
        OsString::from("--"),
        cmd,
    ]
}

/// macOS Seatbelt backend: runs the plugin under `sandbox-exec -p <profile>`.
pub struct MacSeatbeltSandbox {
    /// Path to the `sandbox-exec` binary (overridable in tests).
    exec: PathBuf,
}

impl Default for MacSeatbeltSandbox {
    fn default() -> Self {
        Self {
            exec: PathBuf::from("/usr/bin/sandbox-exec"),
        }
    }
}

impl MacSeatbeltSandbox {
    /// Construct with an explicit `sandbox-exec` path (tests).
    pub fn with_exec(exec: PathBuf) -> Self {
        Self { exec }
    }
}

impl Sandbox for MacSeatbeltSandbox {
    fn wrap(
        &self,
        vault_root: &Path,
        plugin_dir: &Path,
        cmd: &Path,
        args: &[String],
    ) -> Result<Command, SandboxError> {
        if !self.exec.exists() {
            return Err(SandboxError::Unavailable(format!(
                "{} not found",
                self.exec.display()
            )));
        }
        // Seatbelt `subpath`/`literal` match canonical absolute paths.
        let vault_root_abs = vault_root
            .canonicalize()
            .map_err(|e| SandboxError::Unavailable(format!("canonicalize vault root: {e}")))?;
        let dir = plugin_dir
            .canonicalize()
            .map_err(|e| SandboxError::Unavailable(format!("canonicalize plugin dir: {e}")))?;
        let cmd_abs = cmd
            .canonicalize()
            .map_err(|e| SandboxError::Unavailable(format!("canonicalize command: {e}")))?;
        let profile = seatbelt_profile(&vault_root_abs, &dir, &cmd_abs);
        let mut c = Command::new(&self.exec);
        c.arg("-p").arg(profile).arg("--").arg(&cmd_abs).args(args);
        Ok(c)
    }
}

/// One-time probe that confirms unprivileged user namespaces actually work on
/// this host. `bwrap` exists on many systems where userns is disabled by policy
/// (some hardened/older distros); without this probe such a host would surface a
/// confusing spawn-time error instead of a clean refusal. Runs a trivial jail
/// over `/bin/true` and reports the bwrap stderr on failure.
fn bwrap_probe(exec: &Path) -> Result<(), String> {
    // Use the already-existence-checked `bwrap` binary itself as the trivial
    // inner command (`bwrap … -- <bwrap> --version`): a userns-disabled host
    // fails during namespace setup, before the inner exec, while a working host
    // runs `--version` and exits 0. This avoids assuming a fixed path such as
    // /bin/true exists (it does not on e.g. NixOS).
    let out = Command::new(exec)
        .arg("--ro-bind")
        .arg("/")
        .arg("/")
        .arg("--unshare-all")
        .arg("--")
        .arg(exec)
        .arg("--version")
        .output()
        .map_err(|e| format!("spawn {}: {e}", exec.display()))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "user namespaces unavailable: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// Linux bubblewrap backend: runs the plugin under `bwrap <flags> -- <cmd>`.
/// The external launcher applies the jail, mirroring how `MacSeatbeltSandbox`
/// uses `sandbox-exec`.
pub struct LinuxBwrapSandbox {
    /// Path to the `bwrap` binary (overridable in tests).
    exec: PathBuf,
    /// Cached result of the one-time userns probe (see [`bwrap_probe`]).
    probe: OnceLock<Result<(), String>>,
}

impl Default for LinuxBwrapSandbox {
    fn default() -> Self {
        Self {
            exec: PathBuf::from("/usr/bin/bwrap"),
            probe: OnceLock::new(),
        }
    }
}

impl LinuxBwrapSandbox {
    /// Construct with an explicit `bwrap` path (tests).
    pub fn with_exec(exec: PathBuf) -> Self {
        Self {
            exec,
            probe: OnceLock::new(),
        }
    }
}

impl Sandbox for LinuxBwrapSandbox {
    fn wrap(
        &self,
        vault_root: &Path,
        plugin_dir: &Path,
        cmd: &Path,
        args: &[String],
    ) -> Result<Command, SandboxError> {
        if !self.exec.exists() {
            return Err(SandboxError::Unavailable(format!(
                "{} not found",
                self.exec.display()
            )));
        }
        if let Err(e) = self.probe.get_or_init(|| bwrap_probe(&self.exec)) {
            return Err(SandboxError::Unavailable(e.clone()));
        }
        // bwrap binds/masks match canonical absolute paths.
        let vault_root_abs = vault_root
            .canonicalize()
            .map_err(|e| SandboxError::Unavailable(format!("canonicalize vault root: {e}")))?;
        let dir = plugin_dir
            .canonicalize()
            .map_err(|e| SandboxError::Unavailable(format!("canonicalize plugin dir: {e}")))?;
        let cmd_abs = cmd
            .canonicalize()
            .map_err(|e| SandboxError::Unavailable(format!("canonicalize command: {e}")))?;
        let mut c = Command::new(&self.exec);
        c.args(bwrap_args(&vault_root_abs, &dir, &cmd_abs))
            .args(args);
        Ok(c)
    }
}

/// Backend for platforms with no OS sandbox yet: always refuses, so the host
/// never spawns an unjailed plugin (the Windows backend is issue #62).
pub struct RefusingSandbox;

impl Sandbox for RefusingSandbox {
    fn wrap(
        &self,
        _vault_root: &Path,
        _plugin_dir: &Path,
        _cmd: &Path,
        _args: &[String],
    ) -> Result<Command, SandboxError> {
        Err(SandboxError::Unavailable(format!(
            "no sandbox backend for target_os={}",
            std::env::consts::OS
        )))
    }
}

/// The sandbox for the current platform: Seatbelt on macOS, bubblewrap on
/// Linux, refusing elsewhere.
pub fn platform_sandbox() -> Box<dyn Sandbox> {
    #[cfg(target_os = "macos")]
    {
        Box::new(MacSeatbeltSandbox::default())
    }
    #[cfg(target_os = "linux")]
    {
        Box::new(LinuxBwrapSandbox::default())
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Box::new(RefusingSandbox)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cairn_ports::{Sandbox, SandboxError};
    use std::path::PathBuf;

    #[test]
    fn profile_denies_write_network_and_interpolates_paths() {
        let p = seatbelt_profile(
            &PathBuf::from("/cairn"),
            &PathBuf::from("/cairn/.cairn/plugins/p"),
            &PathBuf::from("/cairn/.cairn/plugins/p/bin"),
        );
        assert!(p.contains("(deny default)"));
        assert!(p.contains("(allow file-read*)"));
        assert!(p.contains("(deny file-read* (subpath \"/cairn\"))"));
        assert!(p.contains("(allow file-read* (subpath \"/cairn/.cairn/plugins/p\"))"));
        assert!(p.contains("(deny file-write*)"));
        assert!(p.contains("(deny network*)"));
        assert!(p.contains("(deny process-exec*)"));
        assert!(p.contains("(literal \"/cairn/.cairn/plugins/p/bin\")"));
        assert!(p.contains("(allow process-exec (literal \"/cairn/.cairn/plugins/p/bin\"))"));
    }

    #[test]
    fn sbpl_quote_escapes_quotes_and_backslashes() {
        assert_eq!(sbpl_quote(Path::new(r#"/a/"b"\c"#)), r#""/a/\"b\"\\c""#);
    }

    #[test]
    fn space_in_path_is_quoted_not_escaped() {
        let dir = PathBuf::from("/Library/Application Support/cairn/p");
        assert_eq!(sbpl_quote(&dir), "\"/Library/Application Support/cairn/p\"");
        let p = seatbelt_profile(&PathBuf::from("/vault"), &dir, &dir.join("bin"));
        assert!(p.contains("(subpath \"/Library/Application Support/cairn/p\")"));
    }

    #[test]
    fn newline_in_path_is_escaped() {
        // A real newline byte in the path must not produce a multi-line SBPL token.
        assert_eq!(sbpl_quote(Path::new("/a/b\nc")), r#""/a/b\nc""#);
        assert_eq!(sbpl_quote(Path::new("/a/b\rc")), r#""/a/b\rc""#);
    }

    #[test]
    fn refusing_sandbox_is_always_unavailable() {
        let err = RefusingSandbox
            .wrap(Path::new("/"), Path::new("/"), Path::new("/bin/echo"), &[])
            .unwrap_err();
        assert!(matches!(err, SandboxError::Unavailable(_)));
    }

    #[test]
    fn bwrap_args_binds_root_masks_vault_reexposes_plugin_dir_and_disables_net() {
        let a = bwrap_args(
            Path::new("/cairn"),
            Path::new("/cairn/.cairn/plugins/p"),
            Path::new("/cairn/.cairn/plugins/p/bin"),
        );
        let s: Vec<String> = a.iter().map(|o| o.to_string_lossy().into_owned()).collect();
        assert_eq!(
            s,
            vec![
                "--ro-bind",
                "/",
                "/",
                "--tmpfs",
                "/cairn",
                "--ro-bind",
                "/cairn/.cairn/plugins/p",
                "/cairn/.cairn/plugins/p",
                "--dev",
                "/dev",
                "--proc",
                "/proc",
                "--unshare-all",
                "--die-with-parent",
                "--",
                "/cairn/.cairn/plugins/p/bin",
            ]
        );
    }

    #[test]
    fn linux_sandbox_missing_bwrap_is_unavailable() {
        let s = LinuxBwrapSandbox::with_exec(PathBuf::from("/nonexistent/bwrap"));
        let err = s
            .wrap(Path::new("/"), Path::new("/"), Path::new("/bin/true"), &[])
            .unwrap_err();
        assert!(matches!(err, SandboxError::Unavailable(_)));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn wrap_builds_sandbox_exec_argv_in_order() {
        use std::ffi::OsStr;

        let tmp = tempfile::tempdir().unwrap();
        let plugin_dir = tmp.path().join("p");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        // Real, canonicalizable paths so wrap() does not early-return.
        // `with_exec` points at an existing file so the exists() check passes.
        let sandbox = MacSeatbeltSandbox::with_exec(PathBuf::from("/bin/sh"));
        let command = sandbox
            .wrap(
                tmp.path(),
                &plugin_dir,
                Path::new("/bin/echo"),
                &["a".to_string(), "b".to_string()],
            )
            .expect("wrap should succeed for existing canonicalizable paths");

        assert_eq!(command.get_program(), OsStr::new("/bin/sh"));
        let args: Vec<&OsStr> = command.get_args().collect();
        // Expected: -p <profile> -- <canonical cmd> a b
        assert_eq!(args[0], OsStr::new("-p"));
        assert!(
            args[1].to_string_lossy().contains("(deny default)"),
            "arg after -p must be the SBPL profile"
        );
        assert_eq!(args[2], OsStr::new("--"));
        assert!(
            Path::new(args[3]).ends_with("echo"),
            "the jailed command must follow `--`"
        );
        assert_eq!(args[4], OsStr::new("a"));
        assert_eq!(args[5], OsStr::new("b"));
        assert_eq!(args.len(), 6, "no unexpected trailing args");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn missing_sandbox_exec_is_unavailable() {
        let tmp = tempfile::tempdir().unwrap();
        let s = MacSeatbeltSandbox::with_exec(PathBuf::from("/nonexistent/sandbox-exec"));
        let err = s
            .wrap(tmp.path(), tmp.path(), Path::new("/bin/echo"), &[])
            .unwrap_err();
        assert!(matches!(err, SandboxError::Unavailable(_)));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn seatbelt_denies_write_outside_plugin_dir() {
        let vault = tempfile::tempdir().unwrap();
        let plugin_dir = vault.path().join(".cairn/plugins/p");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        let escaped = vault.path().join("escaped.txt");

        let mut cmd = MacSeatbeltSandbox::default()
            .wrap(
                vault.path(),
                &plugin_dir,
                Path::new("/usr/bin/touch"),
                &[escaped.to_string_lossy().into_owned()],
            )
            .expect("sandbox-exec present on macOS");
        let status = cmd.status().expect("spawn under sandbox");

        // Real EPERM after the binary loads — exits non-zero WITHOUT a signal
        // (a signal would mean it crashed at link time, not that the write was denied).
        assert!(!status.success(), "write outside the jail must be denied");
        assert!(
            status.code().is_some(),
            "touch must exit via an error code (EPERM), not be killed by a signal \
             (a signal would mean a link-time crash, not a write denial)"
        );
        assert!(!escaped.exists(), "the file must not have been created");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn seatbelt_allows_plugin_to_exec_and_pipe_stdout() {
        let vault = tempfile::tempdir().unwrap();
        let plugin_dir = vault.path().join(".cairn/plugins/p");
        std::fs::create_dir_all(&plugin_dir).unwrap();

        let output = MacSeatbeltSandbox::default()
            .wrap(
                vault.path(),
                &plugin_dir,
                Path::new("/bin/echo"),
                &["hi".to_string()],
            )
            .expect("sandbox-exec present on macOS")
            .output()
            .expect("spawn under sandbox");

        assert!(
            output.status.success(),
            "the plugin command must be allowed to exec"
        );
        assert_eq!(output.stdout, b"hi\n", "stdout must pipe through the jail");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn seatbelt_denies_reading_vault_but_allows_plugin_dir() {
        let vault = tempfile::tempdir().unwrap();
        let plugin_dir = vault.path().join(".cairn/plugins/p");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        std::fs::write(vault.path().join("secret.md"), b"SECRET").unwrap();
        std::fs::write(plugin_dir.join("own.txt"), b"OWN").unwrap();

        // Reading a sibling vault note via /bin/cat -> denied.
        let secret = vault.path().join("secret.md");
        let denied = MacSeatbeltSandbox::default()
            .wrap(
                vault.path(),
                &plugin_dir,
                Path::new("/bin/cat"),
                &[secret.to_string_lossy().into_owned()],
            )
            .expect("sandbox-exec present")
            .output()
            .expect("spawn");
        assert!(
            !denied.status.success(),
            "reading another vault file must be denied"
        );

        // Reading the plugin's own dir file -> allowed.
        let own = plugin_dir.join("own.txt");
        let allowed = MacSeatbeltSandbox::default()
            .wrap(
                vault.path(),
                &plugin_dir,
                Path::new("/bin/cat"),
                &[own.to_string_lossy().into_owned()],
            )
            .expect("sandbox-exec present")
            .output()
            .expect("spawn");
        assert!(
            allowed.status.success(),
            "reading the plugin's own dir must be allowed"
        );
        assert_eq!(allowed.stdout, b"OWN");
    }

    #[cfg(target_os = "linux")]
    fn linux_bwrap_usable() -> bool {
        let exec = Path::new("/usr/bin/bwrap");
        exec.exists() && bwrap_probe(exec).is_ok()
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn bwrap_denies_write_to_host_filesystem() {
        if !linux_bwrap_usable() {
            eprintln!("skipping: bwrap/userns unavailable");
            return;
        }
        let vault = tempfile::tempdir().unwrap();
        let plugin_dir = vault.path().join(".cairn/plugins/p");
        std::fs::create_dir_all(&plugin_dir).unwrap();

        // A non-vault path: it lives under the read-only `--ro-bind / /` root, so a
        // write must fail with EROFS. (A path under the vault would land in the
        // writable-but-isolated tmpfs and succeed in-namespace — see plan notes.)
        let outside = tempfile::tempdir().unwrap();
        let escaped = outside.path().join("escaped.txt");

        let mut cmd = LinuxBwrapSandbox::default()
            .wrap(
                vault.path(),
                &plugin_dir,
                Path::new("/usr/bin/touch"),
                &[escaped.to_string_lossy().into_owned()],
            )
            .expect("bwrap present and userns usable");
        let status = cmd.status().expect("spawn under bwrap");

        assert!(
            !status.success(),
            "write to the read-only host fs must fail"
        );
        assert!(
            status.code().is_some(),
            "touch must exit via an error code (EROFS), not be killed by a signal"
        );
        assert!(
            !escaped.exists(),
            "the file must not have been created on the host"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn bwrap_allows_plugin_to_exec_and_pipe_stdout() {
        if !linux_bwrap_usable() {
            eprintln!("skipping: bwrap/userns unavailable");
            return;
        }
        let vault = tempfile::tempdir().unwrap();
        let plugin_dir = vault.path().join(".cairn/plugins/p");
        std::fs::create_dir_all(&plugin_dir).unwrap();

        let output = LinuxBwrapSandbox::default()
            .wrap(
                vault.path(),
                &plugin_dir,
                Path::new("/bin/echo"),
                &["hi".to_string()],
            )
            .expect("bwrap present and userns usable")
            .output()
            .expect("spawn under bwrap");

        assert!(
            output.status.success(),
            "the plugin command must be allowed to exec"
        );
        assert_eq!(output.stdout, b"hi\n", "stdout must pipe through the jail");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn bwrap_denies_reading_vault_but_allows_plugin_dir() {
        if !linux_bwrap_usable() {
            eprintln!("skipping: bwrap/userns unavailable");
            return;
        }
        let vault = tempfile::tempdir().unwrap();
        let plugin_dir = vault.path().join(".cairn/plugins/p");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        std::fs::write(vault.path().join("secret.md"), b"SECRET").unwrap();
        std::fs::write(plugin_dir.join("own.txt"), b"OWN").unwrap();

        // A sibling vault note: masked by the empty tmpfs, so the read fails.
        let secret = vault.path().join("secret.md");
        let denied = LinuxBwrapSandbox::default()
            .wrap(
                vault.path(),
                &plugin_dir,
                Path::new("/bin/cat"),
                &[secret.to_string_lossy().into_owned()],
            )
            .expect("bwrap present")
            .output()
            .expect("spawn");
        assert!(
            !denied.status.success(),
            "reading a vault file must be denied"
        );

        // The plugin's own dir is re-bound on top of the mask: read succeeds.
        let own = plugin_dir.join("own.txt");
        let allowed = LinuxBwrapSandbox::default()
            .wrap(
                vault.path(),
                &plugin_dir,
                Path::new("/bin/cat"),
                &[own.to_string_lossy().into_owned()],
            )
            .expect("bwrap present")
            .output()
            .expect("spawn");
        assert!(
            allowed.status.success(),
            "reading the plugin's own dir must be allowed"
        );
        assert_eq!(allowed.stdout, b"OWN");
    }
}
