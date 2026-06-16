//! Local bearer-token authentication for the daemon (audit S5). The token is a
//! file under `<cairn>/.cairn/token` (mode `0600`); holding it is equivalent to
//! having read access to that file, i.e. being the cairn's owner.

use std::fs;
use std::io::{self, Write};
use std::path::Path;

use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::AppState;

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

/// axum middleware: when the daemon was configured with a token, reject any
/// request that lacks a matching `Authorization: Bearer <token>` header with
/// `401`. With no token configured, every request passes through.
pub(crate) async fn require_token(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    if let Some(expected) = &state.token {
        if !bearer_matches(req.headers(), expected) {
            return (
                StatusCode::UNAUTHORIZED,
                [(header::WWW_AUTHENTICATE, "Bearer")],
            )
                .into_response();
        }
    }
    next.run(req).await
}

/// axum middleware for `/mcp`: like [`require_token`], but also accepts the token
/// as a `?token=<token>` query parameter. MCP clients connecting over Streamable
/// HTTP can send `Authorization: Bearer`, but tau's β.3 MCP config carries only a
/// bare URL with no header — the query param is that headerless channel. The
/// secret is the same `.cairn/token`; on a loopback bind, holding it still means
/// read access to that file. (The `/mcp` request span records a fixed path, never
/// the raw URI, so the query token is not logged.)
pub(crate) async fn mcp_require_token(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    if let Some(expected) = &state.token {
        let header_ok = bearer_matches(req.headers(), expected);
        let query_ok = req
            .uri()
            .query()
            .and_then(token_from_query)
            .is_some_and(|tok| ct_eq(tok.as_bytes(), expected.as_bytes()));
        if !header_ok && !query_ok {
            return (
                StatusCode::UNAUTHORIZED,
                [(header::WWW_AUTHENTICATE, "Bearer")],
            )
                .into_response();
        }
    }
    next.run(req).await
}

/// Extract the `token` value from a URL query string. Tokens are 64-char hex, so
/// no percent-decoding is needed; this avoids a new dependency.
fn token_from_query(query: &str) -> Option<&str> {
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix("token="))
}

/// True if `headers` carry `Authorization: Bearer <token>` whose token equals
/// `expected`. Missing, non-UTF-8, or non-`Bearer` headers are rejected
/// (deny-by-default, mirroring the CORS/Origin gates).
pub(crate) fn bearer_matches(headers: &HeaderMap, expected: &str) -> bool {
    let Some(value) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    else {
        return false;
    };
    let Some(token) = value.strip_prefix("Bearer ") else {
        return false;
    };
    ct_eq(token.as_bytes(), expected.as_bytes())
}

/// Constant-time byte comparison. The length check leaks only the token length,
/// which is fixed and public; the value comparison itself is timing-independent.
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{header, HeaderMap, HeaderValue};

    fn headers_with(auth: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(header::AUTHORIZATION, HeaderValue::from_str(auth).unwrap());
        h
    }

    #[test]
    fn ct_eq_matches_and_rejects() {
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"ab")); // differing length
    }

    #[test]
    fn bearer_matches_accepts_correct_token() {
        assert!(bearer_matches(&headers_with("Bearer secret"), "secret"));
    }

    #[test]
    fn bearer_matches_rejects_wrong_scheme_value_and_missing() {
        assert!(!bearer_matches(&headers_with("Bearer nope"), "secret"));
        assert!(!bearer_matches(&headers_with("Basic secret"), "secret"));
        assert!(!bearer_matches(&HeaderMap::new(), "secret"));
    }

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
