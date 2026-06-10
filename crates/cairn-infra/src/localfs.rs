//! A `VaultStore` backed by a local directory of `.md` files.

use std::fs;
use std::path::{Path, PathBuf};

use cairn_domain::NotePath;
use cairn_ports::{FileStamp, PortError, VaultStore};

/// Create `<root>/.cairn/` and a `.gitignore` (`*`) so the cache never enters
/// the user's notes repo. Idempotent. Returns the `.cairn` directory path.
///
/// # Errors
/// `Adapter` if the directory or `.gitignore` cannot be created.
pub fn ensure_cairn_dir(root: &Path) -> Result<PathBuf, PortError> {
    let dir = root.join(".cairn");
    fs::create_dir_all(&dir).map_err(|e| PortError::Adapter(e.to_string()))?;
    let ignore = dir.join(".gitignore");
    if !ignore.exists() {
        fs::write(&ignore, "*\n").map_err(|e| PortError::Adapter(e.to_string()))?;
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
}

impl LocalFsStore {
    /// Open a store rooted at `root`, creating the directory if needed.
    ///
    /// # Errors
    /// Returns [`PortError`] if the root directory cannot be created.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, PortError> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root).map_err(|e| PortError::Adapter(e.to_string()))?;
        Ok(Self { root })
    }

    fn full(&self, path: &NotePath) -> PathBuf {
        self.root.join(path.as_str())
    }

    /// Resolve `path` under the root, refusing anything [`is_safe_rel`]
    /// rejects. Used by every mutating operation; read-only paths use `full`.
    fn safe_full(&self, path: &NotePath) -> Result<PathBuf, PortError> {
        if !is_safe_rel(path.as_str()) {
            return Err(PortError::Adapter(format!(
                "unsafe note path: {}",
                path.as_str()
            )));
        }
        Ok(self.full(path))
    }

    fn collect_md(&self, dir: &Path, out: &mut Vec<NotePath>) -> Result<(), PortError> {
        for entry in fs::read_dir(dir).map_err(|e| PortError::Adapter(e.to_string()))? {
            let entry = entry.map_err(|e| PortError::Adapter(e.to_string()))?;
            let path = entry.path();
            if entry
                .file_type()
                .map_err(|e| PortError::Adapter(e.to_string()))?
                .is_dir()
            {
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
                let rel = path
                    .strip_prefix(&self.root)
                    .map_err(|e| PortError::Adapter(e.to_string()))?;
                let rel = rel.to_str().ok_or_else(|| {
                    PortError::Adapter(format!("non-UTF-8 path: {}", rel.display()))
                })?;
                out.push(NotePath::new(rel).map_err(|e| PortError::Adapter(e.to_string()))?);
            }
        }
        Ok(())
    }
}

impl VaultStore for LocalFsStore {
    fn read(&self, path: &NotePath) -> Result<String, PortError> {
        fs::read_to_string(self.full(path))
            .map_err(|_| PortError::NotFound(path.as_str().to_string()))
    }

    fn write(&mut self, path: &NotePath, contents: &str) -> Result<(), PortError> {
        let full = self.safe_full(path)?;
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).map_err(|e| PortError::Adapter(e.to_string()))?;
        }
        fs::write(full, contents).map_err(|e| PortError::Adapter(e.to_string()))
    }

    fn delete(&mut self, path: &NotePath) -> Result<(), PortError> {
        fs::remove_file(self.safe_full(path)?).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                PortError::NotFound(path.as_str().to_string())
            } else {
                PortError::Adapter(e.to_string())
            }
        })
    }

    fn rename(&mut self, from: &NotePath, to: &NotePath) -> Result<(), PortError> {
        let src = self.safe_full(from)?;
        let dst = self.safe_full(to)?;
        if !src.exists() {
            return Err(PortError::NotFound(from.as_str().to_string()));
        }
        if dst.exists() {
            return Err(PortError::AlreadyExists(to.as_str().to_string()));
        }
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent).map_err(|e| PortError::Adapter(e.to_string()))?;
        }
        fs::rename(&src, &dst).map_err(|e| PortError::Adapter(e.to_string()))
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
        let full = self.full(path);
        let meta = match fs::metadata(&full) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(PortError::NotFound(path.as_str().to_string()));
            }
            Err(e) => return Err(PortError::Adapter(e.to_string())),
        };
        let modified = meta
            .modified()
            .map_err(|e| PortError::Adapter(e.to_string()))?;
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
            Err(e) => Err(PortError::Adapter(e.to_string())),
        }
    }

    fn write_meta(&self, data: &str) -> Result<(), PortError> {
        let dir = ensure_cairn_dir(&self.root)?;
        fs::write(dir.join("state.json"), data).map_err(|e| PortError::Adapter(e.to_string()))
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
    fn write_read_list_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = LocalFsStore::open(tmp.path()).unwrap();
        let p = NotePath::new("dir/a.md").unwrap();
        store.write(&p, "hello").unwrap();
        assert_eq!(store.read(&p).unwrap(), "hello");
        assert_eq!(store.list().unwrap(), vec![p]);
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
