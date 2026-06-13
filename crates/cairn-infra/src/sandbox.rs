//! OS-level sandboxing for spawned plugins. macOS uses Seatbelt via
//! `sandbox-exec`; other platforms refuse (no backend yet — see issue #40).

use std::path::Path;

/// Quote a path as an SBPL string literal, escaping the characters that would
/// otherwise break the quoted token: `\`, `"`, and the line terminators `\n`
/// and `\r` (a raw newline inside the `-p` profile could truncate or malform a
/// rule). `to_string_lossy` means a non-UTF-8 path has invalid bytes replaced
/// with U+FFFD; the quoted path then won't match on disk, so the plugin is
/// over-restricted (fails to exec) rather than under-restricted — safe, but a
/// reason not to reuse this helper for a future Linux backend without revisiting.
#[allow(dead_code)]
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
#[allow(dead_code)]
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

#[cfg(test)]
mod tests {
    use super::*;
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
}
