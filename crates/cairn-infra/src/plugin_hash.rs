//! Content hash of a plugin directory tree, pinned in the trust list.
//!
//! A pin is the string `sha256:<64 lowercase hex>`. The explicit algorithm
//! prefix is part of the stored value so a future construction change is a new
//! prefix (surfaced as a mismatch the user can act on), never a silent
//! wrong-compare. The hashing construction under the `sha256:` prefix is a
//! stability contract and must not change once pins exist in the wild.

use cairn_ports::PortError;

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
        assert!(PinnedHash::parse(&format!("sha256:{}", "A".repeat(64))).is_err()); // uppercase
        assert!(PinnedHash::parse(&format!("sha256:{}", "g".repeat(64))).is_err()); // non-hex
        assert!(PinnedHash::parse(&format!("blake3:{}", "a".repeat(64))).is_err());
        // wrong algo
    }
}
