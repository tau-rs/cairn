//! Content hash of a plugin directory tree, pinned in the trust list.
//!
//! A pin is the string `sha256:<64 lowercase hex>`. The explicit algorithm
//! prefix is part of the stored value so a future construction change is a new
//! prefix (surfaced as a mismatch the user can act on), never a silent
//! wrong-compare. The hashing construction under the `sha256:` prefix is a
//! stability contract and must not change once pins exist in the wild.

use cairn_ports::PortError;
use sha2::{Digest, Sha256};

const PREFIX: &str = "sha256:";
const HEX_LEN: usize = 64; // 32 bytes of SHA-256, lowercase hex

/// A pinned content hash, `sha256:<64 hex>`. Compare by value to detect drift.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedHash(String);

impl PinnedHash {
    /// Parse a stored pin. Rejects unknown prefixes, wrong length, non-hex.
    ///
    /// # Errors
    /// [`PortError::Adapter`] on any malformed value (caller surfaces it as a
    /// fail-fast config error).
    pub fn parse(s: &str) -> Result<Self, PortError> {
        let hex = s.strip_prefix(PREFIX).ok_or_else(|| {
            PortError::Adapter(format!("plugin hash {s:?} missing \"{PREFIX}\" prefix").into())
        })?;
        if hex.len() != HEX_LEN
            || !hex
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err(PortError::Adapter(
                format!("plugin hash {s:?} must be \"{PREFIX}\" + {HEX_LEN} lowercase hex chars")
                    .into(),
            ));
        }
        Ok(Self(s.to_string()))
    }
}

impl std::fmt::Display for PinnedHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Hash a list of `(relative_path, bytes)` into a [`PinnedHash`].
///
/// Canonical construction (a stability contract — see module docs): sort by
/// relative path (byte order), then for each file feed `path`, a `0x00`
/// separator (cannot appear in a path), the byte length as little-endian u64,
/// and the bytes. The separator + length framing makes the serialization
/// unambiguous, so no two distinct trees share a hash.
#[allow(dead_code)] // called by the forthcoming `PinnedHash::of_dir`
fn hash_files(mut files: Vec<(String, Vec<u8>)>) -> PinnedHash {
    files.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    let mut hasher = Sha256::new();
    for (path, bytes) in &files {
        hasher.update(path.as_bytes());
        hasher.update([0x00]);
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(PREFIX.len() + HEX_LEN);
    hex.push_str(PREFIX);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(hex, "{byte:02x}");
    }
    PinnedHash(hex)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_canonical_pin() {
        let s = format!("sha256:{}", "a".repeat(64));
        assert_eq!(PinnedHash::parse(&s).unwrap().to_string(), s);
    }

    #[test]
    fn parse_rejects_bad_pins() {
        assert!(PinnedHash::parse(&"a".repeat(64)).is_err()); // no prefix
        assert!(PinnedHash::parse("sha256:abc").is_err()); // too short
        assert!(PinnedHash::parse(&format!("sha256:{}", "a".repeat(63))).is_err()); // 63
        assert!(PinnedHash::parse(&format!("sha256:{}", "a".repeat(65))).is_err()); // 65
        assert!(PinnedHash::parse(&format!("sha256:{}", "A".repeat(64))).is_err()); // uppercase
        assert!(PinnedHash::parse(&format!("sha256:{}", "g".repeat(64))).is_err()); // non-hex
        assert!(PinnedHash::parse(&format!("blake3:{}", "a".repeat(64))).is_err());
        // wrong algo
    }

    #[test]
    fn hash_is_deterministic() {
        let files = vec![("a.txt".to_string(), b"x".to_vec())];
        assert_eq!(hash_files(files.clone()), hash_files(files));
    }

    #[test]
    fn hash_is_order_independent() {
        let asc = vec![
            ("a.txt".to_string(), b"1".to_vec()),
            ("b.txt".to_string(), b"2".to_vec()),
        ];
        let desc = vec![
            ("b.txt".to_string(), b"2".to_vec()),
            ("a.txt".to_string(), b"1".to_vec()),
        ];
        assert_eq!(hash_files(asc), hash_files(desc));
    }

    #[test]
    fn framing_prevents_boundary_collision() {
        // Same concatenated bytes, different split between path and contents.
        // Without the separator + length framing these would collide.
        let a = vec![("ab".to_string(), b"c".to_vec())];
        let b = vec![("a".to_string(), b"bc".to_vec())];
        assert_ne!(hash_files(a), hash_files(b));
    }

    #[test]
    fn distinct_contents_distinct_hash() {
        let a = vec![("f".to_string(), b"one".to_vec())];
        let b = vec![("f".to_string(), b"two".to_vec())];
        assert_ne!(hash_files(a), hash_files(b));
    }
}
