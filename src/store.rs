//! The memory store: a folder of Markdown files.
//!
//! Reads take a path relative to the store root. Absolute paths, `..`, hidden
//! entries (so `.git` and `.lucid` stay private) and symlinks that leave the
//! root are refused. Writes are add-only and go to `inbox/` and `logs/` under
//! names the store chooses, so a caller can never overwrite memory.

use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

use crate::util::{sha256_hex, valid_name, Utc};

/// The index every agent loads at session start.
pub const INDEX: &str = "MEMORY.md";

const MAX_READ_BYTES: u64 = 256 * 1024;
pub const MAX_NOTE_BYTES: usize = 64 * 1024;
const MAX_LIST_ENTRIES: usize = 2000;
const MAX_HIT_CHARS: usize = 300;
const SEARCHABLE: [&str; 2] = ["md", "txt"];

#[derive(Debug)]
pub enum StoreError {
    BadPath(String),
    NotFound(String),
    TooLarge(String),
    Invalid(String),
    Io(io::Error),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::BadPath(p) => write!(f, "path not allowed: {p}"),
            StoreError::NotFound(p) => write!(f, "not found: {p}"),
            StoreError::TooLarge(p) => {
                write!(
                    f,
                    "file too large to read (limit {MAX_READ_BYTES} bytes): {p}"
                )
            }
            StoreError::Invalid(why) => write!(f, "{why}"),
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

/// A raw note for the inbox. `source` is the agent that recorded it; `said_by`
/// is whose statement it is: "user" for the user's own decision, otherwise the
/// agent's name for its own opinion.
pub struct Note<'a> {
    pub source: &'a str,
    pub said_by: &'a str,
    pub text: &'a str,
    pub topic: Option<&'a str>,
    pub origin: Option<&'a str>,
}

pub enum Remembered {
    New(String),
    Duplicate(String),
}

/// A session summary for `logs/`.
pub struct Log<'a> {
    pub source: &'a str,
    pub summary: &'a str,
    pub session: Option<&'a str>,
    pub device: Option<&'a str>,
    pub project: Option<&'a str>,
}

pub struct Store {
    root: PathBuf,
}

/// One line, bounded: safe as a frontmatter value.
fn field(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(200)
        .collect()
}

fn check_text(text: &str) -> Result<&str, StoreError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(StoreError::Invalid("text is empty".into()));
    }
    if text.len() > MAX_NOTE_BYTES {
        return Err(StoreError::Invalid(format!(
            "text is over {MAX_NOTE_BYTES} bytes; split it into smaller notes"
        )));
    }
    Ok(text)
}

fn check_name(what: &str, name: &str) -> Result<(), StoreError> {
    if valid_name(name) {
        Ok(())
    } else {
        Err(StoreError::Invalid(format!("bad {what}: {name:?}")))
    }
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

    /// Add a note to `inbox/`. The same statement by the same voice is stored
    /// once, even after dream has moved the first copy to `archive/`.
    pub fn remember(&self, note: &Note, now: &Utc) -> Result<Remembered, StoreError> {
        check_name("source", note.source)?;
        check_name("said_by", note.said_by)?;
        let text = check_text(note.text)?;
        let hash = &sha256_hex(format!("{}\n{text}", note.said_by).as_bytes())[..12];
        let suffix = format!("-{hash}.md");
        for dir in ["inbox", "archive"] {
            let dir = self.root.join(dir);
            if !dir.is_dir() {
                continue;
            }
            let mut files = Vec::new();
            self.walk(&dir, &mut files)?;
            let same = |f: &&PathBuf| {
                f.file_name()
                    .is_some_and(|n| n.to_string_lossy().ends_with(&suffix))
            };
            if let Some(found) = files.iter().find(same) {
                return Ok(Remembered::Duplicate(self.rel(found)));
            }
        }

        let mut body = format!(
            "---\nsource: {}\nsaid_by: {}\nat: {}\n",
            note.source,
            note.said_by,
            now.iso()
        );
        for (key, value) in [("topic", note.topic), ("origin", note.origin)] {
            if let Some(value) = value.map(field).filter(|v| !v.is_empty()) {
                body.push_str(&format!("{key}: {value}\n"));
            }
        }
        body.push_str(&format!("---\n\n{text}\n"));
        let rel = format!("inbox/{}-{}{suffix}", now.compact(), note.source);
        self.create(&rel, &body, false)?;
        Ok(Remembered::New(rel))
    }

    /// Add a session summary to `logs/<date>/`. A session that reports again
    /// replaces its own earlier summary and nothing else.
    pub fn log(&self, log: &Log, now: &Utc) -> Result<String, StoreError> {
        check_name("source", log.source)?;
        let text = check_text(log.summary)?;
        let session: Option<String> = log
            .session
            .map(|s| {
                s.chars()
                    .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
                    .take(64)
                    .collect()
            })
            .filter(|s: &String| !s.is_empty());
        let name = match &session {
            Some(session) => format!("{session}-{}.md", log.source),
            None => {
                let hash = &sha256_hex(text.as_bytes())[..8];
                format!("{}-{}-{hash}.md", now.compact(), log.source)
            }
        };
        let mut body = format!("---\nsource: {}\nat: {}\n", log.source, now.iso());
        let fields = [
            ("session", session.as_deref()),
            ("device", log.device),
            ("project", log.project),
        ];
        for (key, value) in fields {
            if let Some(value) = value.map(field).filter(|v| !v.is_empty()) {
                body.push_str(&format!("{key}: {value}\n"));
            }
        }
        body.push_str(&format!("---\n\n{text}\n"));
        let rel = format!("logs/{}/{name}", now.date());
        self.create(&rel, &body, true)?;
        Ok(rel)
    }

    /// Write a file at a path this module built from checked parts.
    fn create(&self, rel: &str, body: &str, replace: bool) -> Result<(), StoreError> {
        let path = self.root.join(rel);
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut opts = fs::OpenOptions::new();
        opts.write(true);
        if replace {
            opts.create(true).truncate(true);
        } else {
            opts.create_new(true);
        }
        opts.open(&path)?.write_all(body.as_bytes())?;
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

/// The recipient that means every agent.
pub const EVERYONE: &str = "all";

const MAX_MESSAGES: usize = 50;

/// A message one agent left for another.
pub struct Message {
    pub path: String,
    pub from: String,
    pub to: String,
    pub at: String,
    pub re: Option<String>,
    pub text: String,
}

fn parse_message(path: String, body: &str) -> Option<Message> {
    let (head, text) = body.strip_prefix("---\n")?.split_once("\n---\n")?;
    let get = |key: &str| {
        head.lines()
            .find_map(|line| line.strip_prefix(key)?.strip_prefix(": "))
            .map(str::to_string)
    };
    Some(Message {
        path,
        from: get("from")?,
        to: get("to")?,
        at: get("at").unwrap_or_default(),
        re: get("re"),
        text: text.trim().to_string(),
    })
}

/// The bridge: agents leave each other messages that wait in `messages/`
/// until the recipient's next session picks them up.
impl Store {
    pub fn send(
        &self,
        from: &str,
        to: &str,
        text: &str,
        re: Option<&str>,
        now: &Utc,
    ) -> Result<String, StoreError> {
        check_name("sender", from)?;
        check_name("recipient", to)?;
        if from == to {
            return Err(StoreError::Invalid(
                "cannot send a message to yourself".into(),
            ));
        }
        let text = check_text(text)?;
        let hash = &sha256_hex(text.as_bytes())[..8];
        let mut body = format!("---\nfrom: {from}\nto: {to}\nat: {}\n", now.iso());
        if let Some(re) = re.map(field).filter(|v| !v.is_empty()) {
            body.push_str(&format!("re: {re}\n"));
        }
        body.push_str(&format!("---\n\n{text}\n"));
        let rel = format!("messages/{}/{}-{from}-{hash}.md", now.date(), now.compact());
        self.create(&rel, &body, true)?;
        Ok(rel)
    }

    /// Messages addressed to `who` or to everyone, oldest first. By default
    /// only those `who` has not been given before; they are then marked seen.
    /// With `include_seen`, the most recent ones regardless.
    pub fn messages(&self, who: &str, include_seen: bool) -> Result<Vec<Message>, StoreError> {
        check_name("agent", who)?;
        let dir = self.root.join("messages");
        if !dir.is_dir() {
            return Ok(Vec::new());
        }
        let seen_file = self.root.join(".lucid").join("seen").join(who);
        let seen_text = fs::read_to_string(&seen_file).unwrap_or_default();
        let seen: std::collections::HashSet<&str> = seen_text.lines().collect();

        let mut files = Vec::new();
        self.walk(&dir, &mut files)?;
        files.sort();
        let mut found = Vec::new();
        for file in files {
            let path = self.rel(&file);
            let was_seen = seen.contains(path.as_str());
            if was_seen && !include_seen {
                continue;
            }
            let Some(message) = fs::read_to_string(&file)
                .ok()
                .and_then(|b| parse_message(path, &b))
            else {
                continue;
            };
            if message.from != who && (message.to == who || message.to == EVERYONE) {
                found.push((message, was_seen));
            }
        }
        let extra = found.len().saturating_sub(MAX_MESSAGES);
        let kept: Vec<(Message, bool)> = if include_seen {
            found.into_iter().skip(extra).collect()
        } else {
            found.into_iter().take(MAX_MESSAGES).collect()
        };

        let newly_seen: String = kept
            .iter()
            .filter(|(_, was_seen)| !was_seen)
            .map(|(m, _)| format!("{}\n", m.path))
            .collect();
        if !newly_seen.is_empty() {
            if let Some(dir) = seen_file.parent() {
                fs::create_dir_all(dir)?;
            }
            fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&seen_file)?
                .write_all(newly_seen.as_bytes())?;
        }
        Ok(kept.into_iter().map(|(m, _)| m).collect())
    }
}

#[cfg(test)]
mod message_tests {
    use super::tests::Temp;
    use super::*;

    #[test]
    fn a_message_waits_for_its_recipient_and_is_delivered_once() {
        let t = Temp::new();
        let s = t.store();
        let now = Utc::from_unix(1_700_000_000);
        let path = s
            .send(
                "claude",
                "codex",
                "The API changed; see notes/api.md.",
                None,
                &now,
            )
            .unwrap();
        assert!(path.starts_with("messages/2023-11-14/20231114T221320Z-claude-"));
        s.send(
            "grok",
            "all",
            "I am offline until Monday.",
            Some(&path),
            &Utc::from_unix(1_700_000_050),
        )
        .unwrap();

        assert!(s
            .messages("claude", false)
            .unwrap()
            .iter()
            .all(|m| m.from == "grok"));
        let for_codex = s.messages("codex", false).unwrap();
        assert_eq!(for_codex.len(), 2);
        assert_eq!(
            (for_codex[0].from.as_str(), for_codex[0].to.as_str()),
            ("claude", "codex")
        );
        assert_eq!(for_codex[0].text, "The API changed; see notes/api.md.");
        assert_eq!(for_codex[1].re.as_deref(), Some(path.as_str()));
        assert!(s.messages("codex", false).unwrap().is_empty());
        assert_eq!(s.messages("codex", true).unwrap().len(), 2);
        assert!(s.messages("grok", false).unwrap().is_empty());
    }

    #[test]
    fn refuses_bad_messages() {
        let t = Temp::new();
        let s = t.store();
        let now = Utc::from_unix(0);
        for (from, to, text) in [
            ("claude", "claude", "hi"),
            ("claude", "../x", "hi"),
            ("claude", "codex", " "),
        ] {
            assert!(matches!(
                s.send(from, to, text, None, &now),
                Err(StoreError::Invalid(_))
            ));
        }
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
            fs::write(
                dir.join("notes/rust.md"),
                "Default to Rust.\nsaid_by: user\n",
            )
            .unwrap();
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
        assert!(t
            .store()
            .read("notes/rust.md")
            .unwrap()
            .starts_with("Default to Rust."));
    }

    #[test]
    fn refuses_paths_outside_the_root() {
        let t = Temp::new();
        let s = t.store();
        for p in [
            "../x",
            "notes/../../x",
            "/etc/passwd",
            ".git/config",
            "notes/.hidden",
        ] {
            assert!(matches!(s.read(p), Err(StoreError::BadPath(_))), "{p}");
        }
        assert!(matches!(
            s.read("notes/none.md"),
            Err(StoreError::NotFound(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlinks_that_leave_the_root() {
        let t = Temp::new();
        let outside = t.0.with_extension("outside");
        fs::write(&outside, "outside").unwrap();
        std::os::unix::fs::symlink(&outside, t.0.join("notes/link.md")).unwrap();
        let s = t.store();
        assert!(matches!(
            s.read("notes/link.md"),
            Err(StoreError::BadPath(_))
        ));
        assert!(s
            .list("")
            .unwrap()
            .iter()
            .all(|e| e.path != "notes/link.md"));
        fs::remove_file(outside).unwrap();
    }

    #[test]
    fn lists_files_without_hidden_ones() {
        let t = Temp::new();
        let paths: Vec<String> = t
            .store()
            .list("")
            .unwrap()
            .into_iter()
            .map(|e| e.path)
            .collect();
        assert_eq!(paths, ["MEMORY.md", "notes/rust.md"]);
    }

    fn note<'a>(source: &'a str, said_by: &'a str, text: &'a str) -> Note<'a> {
        Note {
            source,
            said_by,
            text,
            topic: Some("lang\nx: y"),
            origin: None,
        }
    }

    #[test]
    fn remembers_into_the_inbox_with_its_voice() {
        let t = Temp::new();
        let s = t.store();
        let now = Utc::from_unix(1_700_000_000);
        let Remembered::New(path) = s
            .remember(&note("claude", "user", " Use Rust. "), &now)
            .unwrap()
        else {
            panic!("expected a new note");
        };
        assert!(path.starts_with("inbox/20231114T221320Z-claude-"));
        let body = s.read(&path).unwrap();
        assert!(body.starts_with("---\nsource: claude\nsaid_by: user\nat: 2023-11-14T22:13:20Z\n"));
        assert!(body.contains("topic: lang x: y\n---\n\nUse Rust.\n"));
    }

    #[test]
    fn the_same_statement_is_stored_once_even_after_archiving() {
        let t = Temp::new();
        let s = t.store();
        let now = Utc::from_unix(1_700_000_000);
        let Remembered::New(path) = s
            .remember(&note("claude", "user", "Use Rust."), &now)
            .unwrap()
        else {
            panic!()
        };
        let later = Utc::from_unix(1_700_000_999);
        assert!(matches!(
            s.remember(&note("codex", "user", "Use Rust."), &later)
                .unwrap(),
            Remembered::Duplicate(_)
        ));
        // A different voice saying the same thing is a different note.
        assert!(matches!(
            s.remember(&note("codex", "codex", "Use Rust."), &later)
                .unwrap(),
            Remembered::New(_)
        ));
        fs::create_dir_all(t.0.join("archive/inbox")).unwrap();
        let name = path.strip_prefix("inbox/").unwrap();
        fs::rename(t.0.join(&path), t.0.join("archive/inbox").join(name)).unwrap();
        assert!(matches!(
            s.remember(&note("claude", "user", "Use Rust."), &later)
                .unwrap(),
            Remembered::Duplicate(_)
        ));
    }

    #[test]
    fn refuses_bad_notes() {
        let t = Temp::new();
        let s = t.store();
        let now = Utc::from_unix(0);
        let big = "x".repeat(MAX_NOTE_BYTES + 1);
        for n in [
            note("claude", "user", "  "),
            note("claude", "user", &big),
            note("../x", "user", "a"),
            note("claude", "a/b", "a"),
        ] {
            assert!(matches!(s.remember(&n, &now), Err(StoreError::Invalid(_))));
        }
    }

    #[test]
    fn a_session_replaces_only_its_own_log() {
        let t = Temp::new();
        let s = t.store();
        let now = Utc::from_unix(1_700_000_000);
        let log = |summary| Log {
            source: "claude",
            summary,
            session: Some("ab/../12"),
            device: Some("mac"),
            project: None,
        };
        let first = s.log(&log("one"), &now).unwrap();
        let second = s.log(&log("two"), &now).unwrap();
        assert_eq!(first, "logs/2023-11-14/ab12-claude.md");
        assert_eq!(first, second);
        assert!(s
            .read(&first)
            .unwrap()
            .ends_with("device: mac\n---\n\ntwo\n"));
        let loose = Log {
            source: "codex",
            summary: "three",
            session: None,
            device: None,
            project: None,
        };
        assert_ne!(s.log(&loose, &now).unwrap(), first);
        assert_eq!(s.list("logs").unwrap().len(), 2);
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
