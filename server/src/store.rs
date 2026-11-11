//! Read-only access to the memory store: a folder of Markdown files.
//!
//! Every path a caller supplies is relative to the store root. Absolute paths,
//! `..`, hidden entries (so `.git` stays private) and symlinks that leave the
//! root are refused.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

/// The index every agent loads at session start.
pub const INDEX: &str = "MEMORY.md";

const MAX_READ_BYTES: u64 = 256 * 1024;
const MAX_LIST_ENTRIES: usize = 2000;
const MAX_HIT_CHARS: usize = 300;
const SEARCHABLE: [&str; 2] = ["md", "txt"];

#[derive(Debug)]
pub enum StoreError {
    BadPath(String),
    NotFound(String),
    TooLarge(String),
    Io(io::Error),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::BadPath(p) => write!(f, "path not allowed: {p}"),
            StoreError::NotFound(p) => write!(f, "not found: {p}"),
            StoreError::TooLarge(p) => {
                write!(f, "file too large to read (limit {MAX_READ_BYTES} bytes): {p}")
            }
            StoreError::Io(e) => write!(f, "io error: {e}"),
        }
    }
}

impl From<io::Error> for StoreError {
    fn from(e: io::Error) -> Self {
        StoreError::Io(e)
    }
}

pub struct Entry {
    pub path: String,
    pub bytes: u64,
}

pub struct Hit {
    pub path: String,
    pub line: usize,
    pub text: String,
}

pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn open(root: &Path) -> io::Result<Store> {
        let root = fs::canonicalize(root)?;
        if !root.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotADirectory,
                "memory root is not a directory",
            ));
        }
        Ok(Store { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Map a caller-supplied relative path to a real path inside the root.
    fn resolve(&self, rel: &str) -> Result<PathBuf, StoreError> {
        let bad = || StoreError::BadPath(rel.to_string());
        let mut path = self.root.clone();
        for part in Path::new(rel).components() {
            match part {
                Component::CurDir => {}
                Component::Normal(name) => {
                    if name.to_str().is_none_or(|n| n.starts_with('.')) {
                        return Err(bad());
                    }
                    path.push(name);
                }
                _ => return Err(bad()),
            }
        }
        let real = fs::canonicalize(&path).map_err(|e| match e.kind() {
            io::ErrorKind::NotFound => StoreError::NotFound(rel.to_string()),
            _ => StoreError::Io(e),
        })?;
        if !real.starts_with(&self.root) {
            return Err(bad());
        }
        Ok(real)
    }

    /// Path relative to the root, always with forward slashes.
    fn rel(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .unwrap_or(path)
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/")
    }

    pub fn read(&self, rel: &str) -> Result<String, StoreError> {
        let path = self.resolve(rel)?;
        let meta = fs::metadata(&path)?;
        if !meta.is_file() {
            return Err(StoreError::BadPath(format!("{rel} is not a file")));
        }
        if meta.len() > MAX_READ_BYTES {
            return Err(StoreError::TooLarge(rel.to_string()));
        }
        Ok(fs::read_to_string(&path)?)
    }

    /// Every file under `dir` (the whole store when empty), sorted by path.
    pub fn list(&self, dir: &str) -> Result<Vec<Entry>, StoreError> {
        let start = self.resolve(dir)?;
        let mut files = Vec::new();
        self.walk(&start, &mut files)?;
        files.sort();
        files.truncate(MAX_LIST_ENTRIES);
        Ok(files
            .into_iter()
            .map(|p| Entry {
                bytes: fs::metadata(&p).map(|m| m.len()).unwrap_or(0),
                path: self.rel(&p),
            })
            .collect())
    }

    fn walk(&self, dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), StoreError> {
        if dir.is_file() {
            out.push(dir.to_path_buf());
            return Ok(());
        }
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            // Symlinks are not followed, so a walk cannot leave the root.
            let kind = entry.file_type()?;
            if kind.is_dir() {
                self.walk(&entry.path(), out)?;
            } else if kind.is_file() {
                out.push(entry.path());
            }
        }
        Ok(())
    }

    /// Case-insensitive substring search over text files under `dir`.
    pub fn search(&self, query: &str, dir: &str, limit: usize) -> Result<Vec<Hit>, StoreError> {
        let needle = query.to_lowercase();
        let mut hits = Vec::new();
        if needle.trim().is_empty() {
            return Ok(hits);
        }
        let start = self.resolve(dir)?;
        let mut files = Vec::new();
        self.walk(&start, &mut files)?;
        files.sort();
        for file in files {
            let searchable = file
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| SEARCHABLE.contains(&e.to_ascii_lowercase().as_str()));
            if !searchable {
                continue;
            }
            // Unreadable or non-UTF-8 files are skipped rather than failing the search.
            let Ok(text) = fs::read_to_string(&file) else {
                continue;
            };
            for (i, line) in text.lines().enumerate() {
                if line.to_lowercase().contains(&needle) {
                    hits.push(Hit {
                        path: self.rel(&file),
                        line: i + 1,
                        text: line.trim().chars().take(MAX_HIT_CHARS).collect(),
                    });
                    if hits.len() >= limit {
                        return Ok(hits);
                    }
                }
            }
        }
        Ok(hits)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static NEXT: AtomicU32 = AtomicU32::new(0);

    /// A throwaway store with a few files; removed when dropped.
    pub struct Temp(pub PathBuf);

    impl Temp {
        pub fn new() -> Temp {
            let dir = std::env::temp_dir().join(format!(
                "lucid-dream-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(dir.join("notes")).unwrap();
            fs::create_dir_all(dir.join(".git")).unwrap();
            fs::write(dir.join("MEMORY.md"), "# MEMORY\n- [Rust](notes/rust.md)\n").unwrap();
            fs::write(dir.join("notes/rust.md"), "Default to Rust.\nsaid_by: user\n").unwrap();
            fs::write(dir.join(".git/config"), "secret").unwrap();
            Temp(dir)
        }

        pub fn store(&self) -> Store {
            Store::open(&self.0).unwrap()
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn reads_a_file() {
        let t = Temp::new();
        assert!(t.store().read("notes/rust.md").unwrap().starts_with("Default to Rust."));
    }

    #[test]
    fn refuses_paths_outside_the_root() {
        let t = Temp::new();
        let s = t.store();
        for p in ["../x", "notes/../../x", "/etc/passwd", ".git/config", "notes/.hidden"] {
            assert!(matches!(s.read(p), Err(StoreError::BadPath(_))), "{p}");
        }
        assert!(matches!(s.read("notes/none.md"), Err(StoreError::NotFound(_))));
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlinks_that_leave_the_root() {
        let t = Temp::new();
        let outside = t.0.with_extension("outside");
        fs::write(&outside, "outside").unwrap();
        std::os::unix::fs::symlink(&outside, t.0.join("notes/link.md")).unwrap();
        let s = t.store();
        assert!(matches!(s.read("notes/link.md"), Err(StoreError::BadPath(_))));
        assert!(s.list("").unwrap().iter().all(|e| e.path != "notes/link.md"));
        fs::remove_file(outside).unwrap();
    }

    #[test]
    fn lists_files_without_hidden_ones() {
        let t = Temp::new();
        let paths: Vec<String> = t.store().list("").unwrap().into_iter().map(|e| e.path).collect();
        assert_eq!(paths, ["MEMORY.md", "notes/rust.md"]);
    }

    #[test]
    fn searches_case_insensitively_with_a_limit() {
        let t = Temp::new();
        let s = t.store();
        let hits = s.search("RUST", "", 10).unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!((hits[1].path.as_str(), hits[1].line), ("notes/rust.md", 1));
        assert_eq!(s.search("rust", "", 1).unwrap().len(), 1);
        assert!(s.search("  ", "", 10).unwrap().is_empty());
    }
}
