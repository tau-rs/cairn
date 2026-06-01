//! A `VaultStore` backed by a local directory of `.md` files.

use std::fs;
use std::path::{Path, PathBuf};

use cairn_domain::NotePath;
use cairn_ports::{PortError, VaultStore};

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

    fn collect_md(&self, dir: &Path, out: &mut Vec<NotePath>) -> Result<(), PortError> {
        for entry in fs::read_dir(dir).map_err(|e| PortError::Adapter(e.to_string()))? {
            let entry = entry.map_err(|e| PortError::Adapter(e.to_string()))?;
            let path = entry.path();
            if entry
                .file_type()
                .map_err(|e| PortError::Adapter(e.to_string()))?
                .is_dir()
            {
                if path.file_name().is_some_and(|n| n == ".git") {
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
        let full = self.full(path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).map_err(|e| PortError::Adapter(e.to_string()))?;
        }
        fs::write(full, contents).map_err(|e| PortError::Adapter(e.to_string()))
    }

    fn delete(&mut self, path: &NotePath) -> Result<(), PortError> {
        fs::remove_file(self.full(path)).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                PortError::NotFound(path.as_str().to_string())
            } else {
                PortError::Adapter(e.to_string())
            }
        })
    }

    fn rename(&mut self, from: &NotePath, to: &NotePath) -> Result<(), PortError> {
        let src = self.full(from);
        let dst = self.full(to);
        if !src.exists() {
            return Err(PortError::NotFound(from.as_str().to_string()));
        }
        if dst.exists() {
            return Err(PortError::AlreadyExists(to.as_str().to_string()));
        }
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent).map_err(|e| PortError::Adapter(e.to_string()))?;
        }
        std::fs::rename(&src, &dst).map_err(|e| PortError::Adapter(e.to_string()))
    }

    fn list(&self) -> Result<Vec<NotePath>, PortError> {
        let mut out = Vec::new();
        if self.root.exists() {
            self.collect_md(&self.root, &mut out)?;
        }
        out.sort();
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
