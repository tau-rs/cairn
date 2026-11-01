use std::path::{Path, PathBuf};

/// Why a request could not be served. Maps to HTTP status in `build_response`.
#[derive(Debug, PartialEq, Eq)]
pub enum ServeError {
    NotFound,   // 404: unknown file / missing / not a regular file
    Forbidden,  // 403: resolved path escapes the bundle root
    BadRequest, // 400: malformed path (NUL byte, bad percent-escape, etc.)
}

/// Resolve `rel` (the URL path after the host) against an already-canonicalised
/// `root`, returning the canonical file path IFF it is a regular file inside
/// `root`. This is the traversal wall: canonicalisation resolves `..` AND
/// symlinks, then the prefix test rejects anything that escaped.
pub fn resolve_in_root(root: &Path, rel: &str) -> Result<PathBuf, ServeError> {
    let decoded = percent_decode(rel)?;
    if decoded.as_bytes().contains(&0) {
        return Err(ServeError::BadRequest);
    }
    let trimmed = decoded.trim_start_matches('/');
    let candidate = root.join(trimmed);
    let canonical = std::fs::canonicalize(&candidate).map_err(|_| ServeError::NotFound)?;
    if !canonical.starts_with(root) {
        return Err(ServeError::Forbidden);
    }
    if !canonical.is_file() {
        return Err(ServeError::NotFound);
    }
    Ok(canonical)
}

/// Minimal percent-decode (no external dep). Invalid escapes → BadRequest.
fn percent_decode(s: &str) -> Result<String, ServeError> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                if i + 2 >= bytes.len() {
                    return Err(ServeError::BadRequest);
                }
                let hi = (bytes[i + 1] as char)
                    .to_digit(16)
                    .ok_or(ServeError::BadRequest)?;
                let lo = (bytes[i + 2] as char)
                    .to_digit(16)
                    .ok_or(ServeError::BadRequest)?;
                out.push((hi * 16 + lo) as u8);
                i += 3;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|_| ServeError::BadRequest)
}

use crate::plugin_roots::PluginRoots;

/// The locked-down per-frame CSP set on EVERY served response. `'self'` =
/// plugin-sandbox://<id>. No network (connect-src 'none'), no inline scripts
/// (script-src 'self'), only the host may embed the frame.
const PLUGIN_CSP: &str = "default-src 'none'; script-src 'self'; \
    style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; \
    font-src 'self'; connect-src 'none'; base-uri 'none'; form-action 'none'; \
    frame-ancestors tauri://localhost";

/// Expose the CSP constant for the handler in lib.rs.
pub fn plugin_csp() -> &'static str {
    PLUGIN_CSP
}

fn mime_for(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html",
        Some("js") | Some("mjs") => "text/javascript",
        Some("css") => "text/css",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("woff2") => "font/woff2",
        Some("wasm") => "application/wasm",
        _ => "application/octet-stream",
    }
}

fn status_for(e: &ServeError) -> u16 {
    match e {
        ServeError::NotFound => 404,
        ServeError::Forbidden => 403,
        ServeError::BadRequest => 400,
    }
}

/// Resolve `(id, rel)` against the registered roots and build the response
/// (bytes + content-type + status), or an error status with an empty body.
/// The caller (lib.rs) attaches the CSP header from `plugin_csp()`.
pub fn build_response(roots: &PluginRoots, id: &str, rel: &str) -> (u16, &'static str, Vec<u8>) {
    let Some(root) = roots.get(id) else {
        return (404, "text/plain", Vec::new());
    };
    match resolve_in_root(&root, rel) {
        Ok(file) => match std::fs::read(&file) {
            Ok(bytes) => (200, mime_for(&file), bytes),
            Err(_) => (404, "text/plain", Vec::new()),
        },
        Err(e) => (status_for(&e), "text/plain", Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin_roots::PluginRoots;
    use std::collections::HashMap;
    use std::fs;

    fn bundle() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("ui")).unwrap();
        fs::write(dir.path().join("ui/index.html"), b"<h1>hi</h1>").unwrap();
        fs::write(dir.path().join("secret.txt"), b"top secret").unwrap();
        dir
    }

    fn root(d: &tempfile::TempDir) -> PathBuf {
        std::fs::canonicalize(d.path().join("ui")).unwrap()
    }

    #[test]
    fn serves_a_real_file() {
        let d = bundle();
        let got = resolve_in_root(&root(&d), "index.html").unwrap();
        assert!(got.ends_with("index.html"));
    }

    #[test]
    fn dotdot_traversal_is_forbidden() {
        let d = bundle();
        // ../secret.txt escapes ui/ — must be rejected, never served.
        let res = resolve_in_root(&root(&d), "../secret.txt");
        assert!(matches!(
            res,
            Err(ServeError::Forbidden) | Err(ServeError::NotFound)
        ));
    }

    #[test]
    fn nul_byte_is_bad_request() {
        let d = bundle();
        assert_eq!(
            resolve_in_root(&root(&d), "a%00b.html"),
            Err(ServeError::BadRequest)
        );
    }

    #[test]
    fn missing_file_is_not_found() {
        let d = bundle();
        assert_eq!(
            resolve_in_root(&root(&d), "nope.html"),
            Err(ServeError::NotFound)
        );
    }

    #[test]
    fn directory_is_not_found() {
        let d = bundle();
        assert_eq!(resolve_in_root(&root(&d), ""), Err(ServeError::NotFound));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_is_rejected() {
        let d = bundle();
        std::os::unix::fs::symlink(d.path().join("secret.txt"), d.path().join("ui/link.txt"))
            .unwrap();
        let res = resolve_in_root(&root(&d), "link.txt");
        assert!(matches!(
            res,
            Err(ServeError::Forbidden) | Err(ServeError::NotFound)
        ));
    }

    fn roots_with(id: &str, d: &tempfile::TempDir) -> PluginRoots {
        let r = PluginRoots::default();
        r.replace(HashMap::from([(
            id.to_string(),
            d.path().join("ui").to_string_lossy().into_owned(),
        )]));
        r
    }

    #[test]
    fn build_serves_html_with_mime() {
        let d = bundle();
        let r = roots_with("wc", &d);
        let (status, mime, body) = build_response(&r, "wc", "index.html");
        assert_eq!(status, 200);
        assert_eq!(mime, "text/html");
        assert_eq!(body, b"<h1>hi</h1>");
    }

    #[test]
    fn build_unknown_id_is_404() {
        let d = bundle();
        let r = roots_with("wc", &d);
        let (status, _, _) = build_response(&r, "other", "index.html");
        assert_eq!(status, 404);
    }

    #[test]
    fn build_traversal_is_403_or_404() {
        let d = bundle();
        let r = roots_with("wc", &d);
        let (status, _, _) = build_response(&r, "wc", "../secret.txt");
        assert!(status == 403 || status == 404);
    }

    #[test]
    fn csp_blocks_network_and_inline_script() {
        let csp = plugin_csp();
        assert!(csp.contains("connect-src 'none'"));
        assert!(csp.contains("script-src 'self'"));
        assert!(!csp.contains("script-src 'self' 'unsafe-inline'"));
    }
}
