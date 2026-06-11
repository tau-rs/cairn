//! Local bearer-token authentication for the daemon (audit S5). The token is a
//! file under `<cairn>/.cairn/token` (mode `0600`); holding it is equivalent to
//! having read access to that file, i.e. being the cairn's owner.

use std::fs;
use std::io::{self, Write};
use std::path::Path;

/// Generate a fresh 64-char lowercase-hex bearer token, write it to
/// `<cairn_root>/.cairn/token` with mode `0600` (truncating any prior token),
/// and return it. Creates the `.cairn` directory if absent.
///
/// # Errors
/// Returns an error if the OS RNG is unavailable or the file cannot be written.
pub fn generate_token_file(cairn_root: &Path) -> io::Result<String> {
    let token = random_hex_32()?;
    let dir = cairn_root.join(".cairn");
    fs::create_dir_all(&dir)?;
    write_secret_file(&dir.join("token"), &token)?;
    Ok(token)
}

/// 32 cryptographically-random bytes, lowercase-hex encoded (64 chars).
fn random_hex_32() -> io::Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(io::Error::other)?;
    let mut hex = String::with_capacity(64);
    for b in bytes {
        use std::fmt::Write as _;
        // Writing to a String is infallible.
        let _ = write!(hex, "{b:02x}");
    }
    Ok(hex)
}

/// Write `contents` to `path`, owner-read/write only.
#[cfg(unix)]
fn write_secret_file(path: &Path, contents: &str) -> io::Result<()> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    // Enforce 0600 even if the file pre-existed with looser permissions
    // (`.mode()` only applies when the file is newly created).
    f.set_permissions(fs::Permissions::from_mode(0o600))?;
    f.write_all(contents.as_bytes())
}

/// Non-Unix fallback: best-effort write with no permission guarantee (noted in
/// the trust-model docs).
#[cfg(not(unix))]
fn write_secret_file(path: &Path, contents: &str) -> io::Result<()> {
    fs::write(path, contents)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn token_file_is_0600_and_64_hex() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let tok = generate_token_file(tmp.path()).unwrap();
        assert_eq!(tok.len(), 64);
        assert!(tok.bytes().all(|b| b.is_ascii_hexdigit()));
        let meta = std::fs::metadata(tmp.path().join(".cairn").join("token")).unwrap();
        assert_eq!(meta.permissions().mode() & 0o777, 0o600);
    }
}
