//! OS-level sandboxing for spawned plugins. macOS uses Seatbelt via
//! `sandbox-exec`; other platforms refuse (no backend yet — see issue #40).

use std::path::{Path, PathBuf};
use std::process::Command;

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

/// The static "fixed jail" (model A) SBPL profile: read-only access to the
/// plugin's own dir + the runtime libraries needed to exec; deny all direct
/// file-write, network, and any `exec` other than the plugin command itself.
/// The vault is reachable only through the gated host-callback channel.
pub(crate) fn seatbelt_profile(plugin_dir: &Path, cmd: &Path) -> String {
    let dir = sbpl_quote(plugin_dir);
    let cmd = sbpl_quote(cmd);
    // The command binary appears in both file-read* and process-exec: it must be
    // readable to load and exec-able to run — neither use is redundant.
    format!(
        "(version 1)\n\
         (deny default)\n\
         (allow process-fork)\n\
         (allow file-read*\n\
         \t(subpath \"/usr/lib\")\n\
         \t(subpath \"/System\")\n\
         \t(subpath \"/Library/Frameworks\")\n\
         \t(literal {cmd})\n\
         \t(subpath {dir}))\n\
         (deny file-write*)\n\
         (deny network*)\n\
         (deny process-exec*)\n\
         (allow process-exec (literal {cmd}))\n"
    )
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
        let dir = plugin_dir
            .canonicalize()
            .map_err(|e| SandboxError::Unavailable(format!("canonicalize plugin dir: {e}")))?;
        let cmd_abs = cmd
            .canonicalize()
            .map_err(|e| SandboxError::Unavailable(format!("canonicalize command: {e}")))?;
        let profile = seatbelt_profile(&dir, &cmd_abs);
        let mut c = Command::new(&self.exec);
        c.arg("-p").arg(profile).arg("--").arg(&cmd_abs).args(args);
        Ok(c)
    }
}

/// Backend for platforms with no OS sandbox yet: always refuses, so the host
/// never spawns an unjailed plugin (Linux/Windows backends are issue-#40
/// follow-ups).
pub struct RefusingSandbox;

impl Sandbox for RefusingSandbox {
    fn wrap(
        &self,
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

/// The sandbox for the current platform: Seatbelt on macOS, refusing elsewhere.
pub fn platform_sandbox() -> Box<dyn Sandbox> {
    #[cfg(target_os = "macos")]
    {
        Box::new(MacSeatbeltSandbox::default())
    }
    #[cfg(not(target_os = "macos"))]
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
            &PathBuf::from("/cairn/.cairn/plugins/p"),
            &PathBuf::from("/cairn/.cairn/plugins/p/bin"),
        );
        assert!(p.contains("(deny default)"));
        assert!(p.contains("(deny file-write*)"));
        assert!(p.contains("(deny network*)"));
        assert!(p.contains("(deny process-exec*)"));
        assert!(p.contains("(subpath \"/cairn/.cairn/plugins/p\")"));
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
        let p = seatbelt_profile(&dir, &dir.join("bin"));
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
            .wrap(Path::new("/"), Path::new("/bin/echo"), &[])
            .unwrap_err();
        assert!(matches!(err, SandboxError::Unavailable(_)));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn wrap_builds_sandbox_exec_argv_in_order() {
        use std::ffi::OsStr;

        let tmp = tempfile::tempdir().unwrap();
        // Real, canonicalizable paths so wrap() does not early-return.
        // `with_exec` points at an existing file so the exists() check passes.
        let sandbox = MacSeatbeltSandbox::with_exec(PathBuf::from("/bin/sh"));
        let command = sandbox
            .wrap(
                tmp.path(),
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
        let err = s.wrap(tmp.path(), Path::new("/bin/echo"), &[]).unwrap_err();
        assert!(matches!(err, SandboxError::Unavailable(_)));
    }
}
