//! A `VaultStore` backed by a local directory of `.md` files.

use std::fs;
use std::path::{Path, PathBuf};

use cairn_domain::NotePath;
use cairn_ports::{AdapterError, FileStamp, PortError, VaultStore};

/// Wrap an adapter error (typically an [`std::io::Error`]) as a
/// [`PortError::Adapter`], preserving it as the typed `#[source]` so callers can
/// match on the cause (e.g. an [`std::io::ErrorKind`]).
fn adapt<E: std::error::Error + Send + Sync + 'static>(e: E) -> PortError {
    PortError::Adapter(AdapterError::new(e))
}

/// Create `<root>/.cairn/` and a `.gitignore` (`*`) so the cache never enters
/// the user's notes repo. Idempotent. Returns the `.cairn` directory path.
///
/// # Errors
/// `Adapter` if the directory or `.gitignore` cannot be created.
pub fn ensure_cairn_dir(root: &Path) -> Result<PathBuf, PortError> {
    let dir = root.join(".cairn");
    fs::create_dir_all(&dir).map_err(adapt)?;
    let ignore = dir.join(".gitignore");
    if !ignore.exists() {
        fs::write(&ignore, "*\n").map_err(adapt)?;
    }
    Ok(dir)
}

/// Whether `rel` (a relative path under the vault root) is safe to resolve:
/// non-empty, not absolute, and with no `..` or dot-leading segment. Defense
/// in depth behind [`NotePath::new`] — a crafted path that bypassed domain
/// validation still cannot escape the root or name a control directory
/// (`.cairn`, `.git`) or dotfile. Splits on both separators so a stray
/// backslash cannot smuggle a segment past the check.
fn is_safe_rel(rel: &str) -> bool {
    !rel.is_empty()
        && !rel.starts_with('/')
        && !rel.starts_with('\\')
        && rel
            .split(['/', '\\'])
            .all(|seg| seg != ".." && !seg.starts_with('.'))
}

/// Stores notes as files under `root`.
#[derive(Debug, Clone)]
pub struct LocalFsStore {
    root: PathBuf,
    /// `root` with all symlinks resolved, used to confirm that a resolved
    /// target stays inside the cairn.
    canonical_root: PathBuf,
}

impl LocalFsStore {
    /// Open a store rooted at `root`, creating the directory if needed.
    ///
    /// # Errors
    /// Returns [`PortError`] if the root directory cannot be created or its
    /// canonical path cannot be resolved.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, PortError> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root).map_err(adapt)?;
        let canonical_root = fs::canonicalize(&root).map_err(adapt)?;
        Ok(Self {
            root,
            canonical_root,
        })
    }

    fn full(&self, path: &NotePath) -> PathBuf {
        self.root.join(path.as_str())
    }

    /// Resolve `path` under the root, following and validating symlinks so a
    /// symlink planted inside the cairn cannot redirect an operation outside
    /// the root (closing the TOCTOU gap left by purely lexical validation).
    /// Used by every read/write/rename/delete/stamp. (`list` does not need it:
    /// `collect_md` uses `read_dir`'s lstat-based `file_type`, so a directory
    /// symlink reports as a symlink and is never recursed into.)
    ///
    /// 1. Lexical guard ([`is_safe_rel`]) — defense in depth behind `NotePath`.
    /// 2. The final component must not itself be a symlink (`O_NOFOLLOW`-style):
    ///    a leaf symlink could redirect a write/delete/rename through to
    ///    another file.
    /// 3. The deepest ancestor that exists on disk is canonicalized — resolving
    ///    every symlink in that prefix — and must stay under the canonical
    ///    root. Not-yet-existing tail components cannot be symlinks, so the
    ///    original target is returned for the caller to create.
    fn resolve(&self, path: &NotePath) -> Result<PathBuf, PortError> {
        if !is_safe_rel(path.as_str()) {
            return Err(PortError::Adapter(
                format!("unsafe note path: {}", path.as_str()).into(),
            ));
        }
        let full = self.full(path);

        if full
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink())
        {
            return Err(PortError::Adapter(
                format!("note path is a symlink: {}", path.as_str()).into(),
            ));
        }

        // Walk up to the deepest entry that exists on disk. `symlink_metadata`
        // (lstat) treats a broken symlink as existing, so it is caught by the
        // `canonicalize` below rather than silently skipped.
        let mut ancestor = full.as_path();
        while ancestor.symlink_metadata().is_err() {
            ancestor = ancestor.parent().ok_or_else(|| {
                PortError::Adapter(format!("cannot resolve note path: {}", path.as_str()).into())
            })?;
        }
        let canon = fs::canonicalize(ancestor).map_err(adapt)?;
        if !canon.starts_with(&self.canonical_root) {
            return Err(PortError::Adapter(
                format!("note path escapes cairn root: {}", path.as_str()).into(),
            ));
        }
        Ok(full)
    }

    fn collect_md(&self, dir: &Path, out: &mut Vec<NotePath>) -> Result<(), PortError> {
        for entry in fs::read_dir(dir).map_err(adapt)? {
            let entry = entry.map_err(adapt)?;
            let path = entry.path();
            if entry.file_type().map_err(adapt)?.is_dir() {
                // Skip VCS and the cairn cache (`.cairn/` holds the persisted
                // index + state, never notes).
                if path
                    .file_name()
                    .is_some_and(|n| n == ".git" || n == ".cairn")
                {
                    continue;
                }
                self.collect_md(&path, out)?;
            } else if path.extension().is_some_and(|e| e == "md") {
                let rel = path.strip_prefix(&self.root).map_err(adapt)?;
                let rel = rel.to_str().ok_or_else(|| {
                    PortError::Adapter(format!("non-UTF-8 path: {}", rel.display()).into())
                })?;
                // A dotfile `.md` (e.g. `.draft.md`) is not a valid note path;
                // skip it rather than failing the entire listing.
                if let Ok(np) = NotePath::new(rel) {
                    out.push(np);
                }
            }
        }
        Ok(())
    }
}

impl VaultStore for LocalFsStore {
    fn read(&self, path: &NotePath) -> Result<String, PortError> {
        let full = self.resolve(path)?;
        fs::read_to_string(full).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                PortError::NotFound(path.as_str().to_string())
            } else {
                adapt(e)
            }
        })
    }

    fn write(&mut self, path: &NotePath, contents: &str) -> Result<(), PortError> {
        let full = self.resolve(path)?;
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).map_err(adapt)?;
        }
        fs::write(full, contents).map_err(adapt)
    }

    fn delete(&mut self, path: &NotePath) -> Result<(), PortError> {
        fs::remove_file(self.resolve(path)?).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                PortError::NotFound(path.as_str().to_string())
            } else {
                adapt(e)
            }
        })
    }

    fn rename(&mut self, from: &NotePath, to: &NotePath) -> Result<(), PortError> {
        let src = self.resolve(from)?;
        let dst = self.resolve(to)?;
        if !src.exists() {
            return Err(PortError::NotFound(from.as_str().to_string()));
        }
        if dst.exists() {
            return Err(PortError::AlreadyExists(to.as_str().to_string()));
        }
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent).map_err(adapt)?;
        }
        fs::rename(&src, &dst).map_err(adapt)
    }

    fn list(&self) -> Result<Vec<NotePath>, PortError> {
        let mut out = Vec::new();
        if self.root.exists() {
            self.collect_md(&self.root, &mut out)?;
        }
        out.sort();
        Ok(out)
    }

    fn stamp(&self, path: &NotePath) -> Result<FileStamp, PortError> {
        let full = self.resolve(path)?;
        let meta = match fs::metadata(&full) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(PortError::NotFound(path.as_str().to_string()));
            }
            Err(e) => return Err(adapt(e)),
        };
        let modified = meta.modified().map_err(adapt)?;
        Ok(FileStamp {
            modified,
            len: meta.len(),
        })
    }

    fn read_meta(&self) -> Result<Option<String>, PortError> {
        let path = self.root.join(".cairn").join("state.json");
        match fs::read_to_string(&path) {
            Ok(s) => Ok(Some(s)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(adapt(e)),
        }
    }

    fn write_meta(&self, data: &str) -> Result<(), PortError> {
        let dir = ensure_cairn_dir(&self.root)?;
        fs::write(dir.join("state.json"), data).map_err(adapt)
    }

    fn quarantine_meta(&self) -> Result<Option<String>, PortError> {
        let dir = self.root.join(".cairn");
        let src = dir.join("state.json");
        if !src.exists() {
            return Ok(None);
        }
        // Choose a destination that does not clobber a prior quarantine: a
        // recurring corruption loop would otherwise lose all but the latest
        // blob, and on platforms where `rename` refuses an existing target the
        // preservation would fail outright.
        let mut dst = dir.join("state.json.corrupt");
        let mut n = 1u32;
        while dst.exists() {
            dst = dir.join(format!("state.json.corrupt.{n}"));
            n += 1;
        }
        fs::rename(&src, &dst).map_err(adapt)?;
        Ok(Some(dst.display().to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_safe_rel_rejects_control_and_escaping_paths() {
        // The RCE vectors (S1) — even if a caller bypasses NotePath::new.
        assert!(!is_safe_rel(".cairn/plugins/evil/manifest.toml"));
        assert!(!is_safe_rel(".git/config"));
        assert!(!is_safe_rel("notes/.git/config"));
        assert!(!is_safe_rel("a/../../etc/passwd"));
        assert!(!is_safe_rel("../escape"));
        assert!(!is_safe_rel("/absolute"));
        assert!(!is_safe_rel(""));
        // Backslash separators are treated as separators too.
        assert!(!is_safe_rel(r".cairn\x"));
        assert!(!is_safe_rel(r"a\..\..\x"));
        // Ordinary note paths pass.
        assert!(is_safe_rel("dir/a.md"));
        assert!(is_safe_rel("a.md"));
    }

    #[test]
    fn meta_roundtrips_and_creates_gitignored_cairn_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let store = LocalFsStore::open(tmp.path()).unwrap();
        assert!(store.read_meta().unwrap().is_none());

        store.write_meta("{\"x\":1}").unwrap();
        assert_eq!(store.read_meta().unwrap().as_deref(), Some("{\"x\":1}"));

        let ignore = tmp.path().join(".cairn").join(".gitignore");
        assert_eq!(std::fs::read_to_string(ignore).unwrap(), "*\n");
    }

    #[test]
    fn quarantine_meta_moves_state_aside_and_is_noop_when_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let store = LocalFsStore::open(tmp.path()).unwrap();

        // Nothing to move yet.
        assert_eq!(store.quarantine_meta().unwrap(), None);

        store.write_meta("corrupt{").unwrap();
        let moved = store.quarantine_meta().unwrap().expect("a path");
        assert!(moved.ends_with("state.json.corrupt"), "got {moved}");

        // Original bytes preserved at the new path; state.json no longer present.
        let corrupt = tmp.path().join(".cairn").join("state.json.corrupt");
        assert_eq!(std::fs::read_to_string(&corrupt).unwrap(), "corrupt{");
        assert!(store.read_meta().unwrap().is_none());
    }

    #[test]
    fn quarantine_meta_does_not_clobber_a_prior_quarantine() {
        // A recurring corrupt-state loop (G3's "every startup" impact) must not
        // lose earlier corruption: each quarantine gets a distinct destination.
        let tmp = tempfile::tempdir().unwrap();
        let store = LocalFsStore::open(tmp.path()).unwrap();

        store.write_meta("first-corruption").unwrap();
        let first = store.quarantine_meta().unwrap().expect("first path");
        store.write_meta("second-corruption").unwrap();
        let second = store.quarantine_meta().unwrap().expect("second path");

        assert_ne!(
            first, second,
            "second quarantine must not reuse the first path"
        );
        assert_eq!(std::fs::read_to_string(&first).unwrap(), "first-corruption");
        assert_eq!(
            std::fs::read_to_string(&second).unwrap(),
            "second-corruption"
        );
    }

    #[test]
    fn stamp_reflects_writes_and_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = LocalFsStore::open(tmp.path()).unwrap();
        let a = NotePath::new("a.md").unwrap();
        assert!(matches!(store.stamp(&a), Err(PortError::NotFound(_))));

        store.write(&a, "hello").unwrap();
        let s1 = store.stamp(&a).unwrap();
        assert_eq!(s1.len, 5);

        // Different length guarantees a different stamp regardless of mtime resolution.
        store.write(&a, "hello world!!").unwrap();
        let s2 = store.stamp(&a).unwrap();
        assert_ne!(s1, s2);
    }

    #[test]
    fn list_skips_dotfile_md_without_failing() {
        // A stray dotfile `.md` on disk (not creatable via NotePath::new) must
        // not abort listing the whole vault — it is simply not a note.
        let tmp = tempfile::tempdir().unwrap();
        let mut store = LocalFsStore::open(tmp.path()).unwrap();
        let good = NotePath::new("notes/a.md").unwrap();
        store.write(&good, "hi").unwrap();

        std::fs::create_dir_all(tmp.path().join("notes")).unwrap();
        std::fs::write(tmp.path().join("notes/.draft.md"), "secret").unwrap();

        assert_eq!(store.list().unwrap(), vec![good]);
    }

    #[test]
    fn write_read_list_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = LocalFsStore::open(tmp.path()).unwrap();
        let p = NotePath::new("dir/a.md").unwrap();
        store.write(&p, "hello").unwrap();
        assert_eq!(store.read(&p).unwrap(), "hello");
        assert_eq!(store.list().unwrap(), vec![p]);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escaping_root_cannot_be_read_or_written() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.md"), "TOPSECRET").unwrap();

        let mut store = LocalFsStore::open(tmp.path()).unwrap();
        std::fs::create_dir_all(tmp.path().join("notes")).unwrap();
        // A symlink planted inside the cairn pointing outside the root.
        symlink(outside.path(), tmp.path().join("notes/escape")).unwrap();

        // Reading through the symlink must not escape the root.
        let r = NotePath::new("notes/escape/secret.md").unwrap();
        assert!(
            store.read(&r).is_err(),
            "read escaped the cairn via symlink"
        );

        // Neither may stamp leak metadata of a file outside the cairn.
        assert!(
            store.stamp(&r).is_err(),
            "stamp escaped the cairn via symlink"
        );

        // Writing through the symlink must not escape the root, and must not
        // land a file outside the cairn.
        let w = NotePath::new("notes/escape/pwned.md").unwrap();
        assert!(
            store.write(&w, "x").is_err(),
            "write escaped the cairn via symlink"
        );
        assert!(
            !outside.path().join("pwned.md").exists(),
            "write landed outside the cairn"
        );

        // Deleting/renaming through the symlink must not touch the file outside.
        assert!(
            store.delete(&r).is_err(),
            "delete escaped the cairn via symlink"
        );
        assert!(
            store
                .rename(&r, &NotePath::new("notes/escape/moved.md").unwrap())
                .is_err(),
            "rename escaped the cairn via symlink"
        );
        assert!(
            outside.path().join("secret.md").exists(),
            "delete/rename mutated a file outside the cairn"
        );
    }

    #[cfg(unix)]
    #[test]
    fn leaf_symlink_is_rejected_even_when_target_is_in_root() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let mut store = LocalFsStore::open(tmp.path()).unwrap();
        store
            .write(&NotePath::new("real.md").unwrap(), "hi")
            .unwrap();
        // A note whose final component is itself a symlink (even to an in-root
        // file) is refused, so a mutation cannot be redirected through it.
        symlink(tmp.path().join("real.md"), tmp.path().join("link.md")).unwrap();

        let link = NotePath::new("link.md").unwrap();
        assert!(store.write(&link, "x").is_err());
        assert!(store.delete(&link).is_err());
        assert_eq!(
            store.read(&NotePath::new("real.md").unwrap()).unwrap(),
            "hi",
            "the real note was modified through a leaf symlink"
        );
    }

    #[cfg(unix)]
    #[test]
    fn in_root_symlink_resolves_correctly() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let mut store = LocalFsStore::open(tmp.path()).unwrap();
        let real = NotePath::new("real/a.md").unwrap();
        store.write(&real, "hello").unwrap();
        // A directory symlink that stays inside the root resolves normally.
        symlink(tmp.path().join("real"), tmp.path().join("link")).unwrap();
        let via = NotePath::new("link/a.md").unwrap();
        assert_eq!(store.read(&via).unwrap(), "hello");
    }

    #[test]
    fn read_unreadable_path_is_not_not_found() {
        // A path that exists but cannot be read as a note (here: a directory
        // sitting where a note file would be) must surface as a real adapter
        // error, never masquerade as a missing note.
        let tmp = tempfile::tempdir().unwrap();
        let store = LocalFsStore::open(tmp.path()).unwrap();
        let p = NotePath::new("a.md").unwrap();
        std::fs::create_dir(tmp.path().join("a.md")).unwrap();

        let err = store.read(&p).unwrap_err();
        assert!(
            matches!(err, PortError::Adapter(_)),
            "expected Adapter, got {err:?}"
        );
    }

    #[test]
    fn read_missing_is_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let store = LocalFsStore::open(tmp.path()).unwrap();
        let p = NotePath::new("nope.md").unwrap();
        assert!(matches!(store.read(&p), Err(PortError::NotFound(_))));
    }

    #[test]
    fn rename_moves_into_subdir_and_refuses_clobber() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = LocalFsStore::open(tmp.path()).unwrap();
        let a = NotePath::new("a.md").unwrap();
        let b = NotePath::new("dir/b.md").unwrap();
        store.write(&a, "hello").unwrap();

        store.rename(&a, &b).unwrap();
        assert!(matches!(store.read(&a), Err(PortError::NotFound(_))));
        assert_eq!(store.read(&b).unwrap(), "hello");

        let c = NotePath::new("c.md").unwrap();
        store.write(&c, "x").unwrap();
        assert!(matches!(
            store.rename(&c, &b),
            Err(PortError::AlreadyExists(_))
        ));
        let gone = NotePath::new("gone.md").unwrap();
        assert!(matches!(
            store.rename(&gone, &NotePath::new("z.md").unwrap()),
            Err(PortError::NotFound(_))
        ));
    }
}
