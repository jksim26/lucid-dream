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

use crate::util::{label, sha256_hex, valid_name, Utc};

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
    /// Set when the message is for one session of the recipient, not all of them.
    pub session: Option<String>,
    pub text: String,
}

/// A session that has announced itself: which agent, what it calls itself,
/// where it runs, and whether it is listening for messages right now.
pub struct Presence {
    pub agent: String,
    /// What tells one session from another, such as Claude Code's session id.
    /// Empty when the client has none; such a session is told apart by its
    /// name and device alone.
    pub id: String,
    /// When listed: the session's number, which is its address. At check-in
    /// it only matters for a session with no `id`, as the folder it is in.
    pub session: String,
    /// What the session is about, for people reading the list.
    pub title: String,
    pub device: String,
    pub project: String,
    pub live: bool,
    pub at: i64,
}

/// A session counts as listening if it checked in this recently.
pub const LISTENING_SECS: i64 = 90;
const PRESENCE_KEPT_SECS: i64 = 24 * 3600;

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
        session: get("session"),
        text: text.trim().to_string(),
    })
}

/// The bridge: agents leave each other messages that wait in `messages/`
/// until the recipient's next session picks them up.
impl Store {
    #[cfg(test)]
    pub fn send(
        &self,
        from: &str,
        to: &str,
        text: &str,
        re: Option<&str>,
        now: &Utc,
    ) -> Result<String, StoreError> {
        self.send_to(from, to, None, text, re, now)
    }

    /// Like `send`, optionally for one named session of the recipient.
    pub fn send_to(
        &self,
        from: &str,
        to: &str,
        session: Option<&str>,
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
        if let Some(session) = session.map(label).filter(|v| !v.is_empty()) {
            body.push_str(&format!("session: {session}\n"));
        }
        body.push_str(&format!("---\n\n{text}\n"));
        let rel = format!("messages/{}/{}-{from}-{hash}.md", now.date(), now.compact());
        self.create(&rel, &body, true)?;
        // Every listener, in this process or in a door's, watches this file
        // to learn at once that there is something to fetch.
        let _ = fs::write(self.sent_marker(), &rel);
        Ok(rel)
    }

    /// The file that changes whenever a message is sent.
    pub fn sent_marker(&self) -> PathBuf {
        self.root.join(".lucid").join("sent")
    }

    /// Messages addressed to `who` or to everyone, oldest first. By default
    /// only those `who` has not marked seen; with `include_seen`, the most
    /// recent ones regardless. `mark_seen` records the returned ones as seen.
    /// An agent runs many sessions under one name, so a session-start hook
    /// only peeks: a message keeps being shown until a session that actually
    /// read it asks for it with `mark_seen`.
    #[cfg(test)]
    pub fn messages(
        &self,
        who: &str,
        include_seen: bool,
        mark_seen: bool,
    ) -> Result<Vec<Message>, StoreError> {
        self.messages_for(who, None, include_seen, mark_seen)
    }

    /// Like `messages`, for one session. A message sent to a named session is
    /// returned only to that session; a caller with no session name sees such
    /// messages only when it asks for everything (`include_seen`).
    pub fn messages_for(
        &self,
        who: &str,
        session: Option<&str>,
        include_seen: bool,
        mark_seen: bool,
    ) -> Result<Vec<Message>, StoreError> {
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
            let addressed = message.from != who && (message.to == who || message.to == EVERYONE);
            let for_this_session = match (&message.session, session) {
                (None, _) => true,
                (Some(wanted), Some(mine)) => wanted == mine,
                (Some(_), None) => include_seen,
            };
            if addressed && for_this_session {
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
        if mark_seen && !newly_seen.is_empty() {
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

/// Presence: sessions announce themselves so other agents can see who is
/// around and address one of them. Kept in `.lucid/presence/`, outside git.
impl Store {
    /// Records that a session is around and returns its number, which is how
    /// it is addressed. A session keeps its number while it keeps checking
    /// in; a new one takes the lowest number no recent session holds, across
    /// all agents, so the numbers stay small and each means one session.
    pub fn check_in(&self, presence: &Presence) -> Result<String, StoreError> {
        static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());

        check_name("agent", &presence.agent)?;
        let (id, device) = (label(&presence.id), label(&presence.device));
        // Without an id, a session is told apart by its folder and device.
        let file = if id.is_empty() {
            format!("{}--{device}.json", label(&presence.session))
        } else {
            format!("id-{id}.json")
        };
        let root = self.root.join(".lucid").join("presence");
        let dir = root.join(&presence.agent);
        fs::create_dir_all(&dir)?;
        let mine = dir.join(file);

        let read = |path: &Path| {
            fs::read_to_string(path)
                .ok()
                .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        };
        let before = read(&mine).unwrap_or_default();
        let mut taken = Vec::new();
        for agent_dir in fs::read_dir(&root)?.flatten() {
            for other in fs::read_dir(agent_dir.path())
                .into_iter()
                .flatten()
                .flatten()
            {
                if other.path() == mine {
                    continue;
                }
                let Some(entry) = read(&other.path()) else {
                    continue;
                };
                if presence.at - entry["at"].as_i64().unwrap_or(0) <= PRESENCE_KEPT_SECS {
                    taken.extend(entry["number"].as_u64());
                }
            }
        }
        let number = before["number"]
            .as_u64()
            .filter(|n| *n > 0 && !taken.contains(n))
            .or_else(|| (1..).find(|n| !taken.contains(n)))
            .unwrap_or(1);
        // What the session is about, for whoever reads the list. A check-in
        // that has no title (the channel helper's) keeps the one given before.
        let title = match field(&presence.title) {
            new if !new.is_empty() => new,
            _ => before["title"].as_str().unwrap_or("").to_string(),
        };
        // A hook checking in for a session whose helper is listening does not
        // mark it as gone quiet.
        let live = presence.live
            || (before["live"] == true
                && presence.at - before["at"].as_i64().unwrap_or(0) <= LISTENING_SECS);
        let entry = serde_json::json!({
            "number": number, "title": title, "device": device,
            "project": field(&presence.project), "live": live, "at": presence.at,
        });
        fs::write(mine, entry.to_string())?;
        Ok(number.to_string())
    }

    /// Sessions seen in the last day, listening ones first, then most recent.
    pub fn who(&self, now: i64) -> Vec<Presence> {
        let mut found = Vec::new();
        let root = self.root.join(".lucid").join("presence");
        for agent_dir in fs::read_dir(root).into_iter().flatten().flatten() {
            let agent = agent_dir.file_name().to_string_lossy().into_owned();
            for file in fs::read_dir(agent_dir.path())
                .into_iter()
                .flatten()
                .flatten()
            {
                let Some(entry) = fs::read_to_string(file.path())
                    .ok()
                    .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
                else {
                    continue;
                };
                let text = |key: &str| entry[key].as_str().unwrap_or("").to_string();
                let at = entry["at"].as_i64().unwrap_or(0);
                if now - at > PRESENCE_KEPT_SECS {
                    continue;
                }
                found.push(Presence {
                    agent: agent.clone(),
                    id: String::new(),
                    session: entry["number"].as_u64().unwrap_or(0).to_string(),
                    title: text("title"),
                    device: text("device"),
                    project: text("project"),
                    // A listener that stopped checking in is no longer listening.
                    live: entry["live"] == true && now - at <= LISTENING_SECS,
                    at,
                });
            }
        }
        let number = |p: &Presence| p.session.parse::<u64>().unwrap_or(0);
        found.sort_by_key(|p| (p.agent.clone(), number(p)));
        found
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
            .messages("claude", false, true)
            .unwrap()
            .iter()
            .all(|m| m.from == "grok"));
        let for_codex = s.messages("codex", false, true).unwrap();
        assert_eq!(for_codex.len(), 2);
        assert_eq!(
            (for_codex[0].from.as_str(), for_codex[0].to.as_str()),
            ("claude", "codex")
        );
        assert_eq!(for_codex[0].text, "The API changed; see notes/api.md.");
        assert_eq!(for_codex[1].re.as_deref(), Some(path.as_str()));
        assert!(s.messages("codex", false, true).unwrap().is_empty());
        assert_eq!(s.messages("codex", true, true).unwrap().len(), 2);
        assert!(s.messages("grok", false, true).unwrap().is_empty());
    }

    #[test]
    fn peeking_leaves_a_message_unread_for_the_next_session() {
        let t = Temp::new();
        let s = t.store();
        s.send(
            "grok",
            "claude",
            "Hello from Grok.",
            None,
            &Utc::from_unix(1_700_000_000),
        )
        .unwrap();
        // Two sessions start and peek; both see it.
        assert_eq!(s.messages("claude", false, false).unwrap().len(), 1);
        assert_eq!(s.messages("claude", false, false).unwrap().len(), 1);
        // One of them reads it for real; later sessions are not shown it again.
        assert_eq!(s.messages("claude", false, true).unwrap().len(), 1);
        assert!(s.messages("claude", false, false).unwrap().is_empty());
    }

    #[test]
    fn a_message_for_one_session_reaches_only_that_session() {
        let t = Temp::new();
        let s = t.store();
        let now = Utc::from_unix(1_700_000_000);
        s.send_to(
            "grok",
            "claude",
            Some("Trade"),
            "Check the stop loss.",
            None,
            &now,
        )
        .unwrap();
        s.send(
            "grok",
            "claude",
            "For any Claude.",
            None,
            &Utc::from_unix(1_700_000_010),
        )
        .unwrap();

        let texts = |session: Option<&str>, all: bool| -> Vec<String> {
            s.messages_for("claude", session, all, false)
                .unwrap()
                .into_iter()
                .map(|m| m.text)
                .collect()
        };
        assert_eq!(
            texts(Some("trade"), false),
            ["Check the stop loss.", "For any Claude."]
        );
        assert_eq!(texts(Some("dev"), false), ["For any Claude."]);
        assert_eq!(texts(None, false), ["For any Claude."]);
        // Asking for everything, without a session name, shows targeted ones too.
        assert_eq!(texts(None, true).len(), 2);
        assert_eq!(
            s.messages_for("claude", Some("trade"), false, false)
                .unwrap()[0]
                .session
                .as_deref(),
            Some("trade")
        );
    }

    #[test]
    fn sessions_are_numbered_and_stop_counting_as_listening() {
        let t = Temp::new();
        let s = t.store();
        let p = |agent: &str, id: &str, live: bool, at: i64| Presence {
            agent: agent.into(),
            id: id.into(),
            session: "lucid-dream".into(),
            title: String::new(),
            device: "Mac Neo".into(),
            project: "/Users/x/lucid-dream".into(),
            live,
            at,
        };
        // Two sessions in one folder, and one of another agent: a number each.
        assert_eq!(s.check_in(&p("claude", "a", true, 1000)).unwrap(), "1");
        assert_eq!(s.check_in(&p("claude", "b", false, 1001)).unwrap(), "2");
        assert_eq!(s.check_in(&p("codex", "c", false, 1002)).unwrap(), "3");
        // Each keeps its number as it checks in again, whatever the order.
        assert_eq!(s.check_in(&p("claude", "b", false, 1003)).unwrap(), "2");
        assert_eq!(s.check_in(&p("claude", "a", true, 1004)).unwrap(), "1");

        let seen: Vec<(String, String, bool)> = s
            .who(1010)
            .into_iter()
            .map(|p| (p.agent, p.session, p.live))
            .collect();
        let row = |agent: &str, n: &str, live| (agent.to_string(), n.to_string(), live);
        assert_eq!(
            seen,
            [
                row("claude", "1", true),
                row("claude", "2", false),
                row("codex", "3", false)
            ]
        );
        // With no further check-in, the listener has gone quiet.
        assert!(s.who(1004 + LISTENING_SECS + 1).iter().all(|p| !p.live));

        // A title is kept when a later check-in has none, and a hook's
        // check-in does not make a listening session look quiet.
        let titled = Presence {
            title: "Agent memory repo".into(),
            live: false,
            ..p("claude", "a", false, 1020)
        };
        s.check_in(&titled).unwrap();
        s.check_in(&p("claude", "a", true, 1030)).unwrap();
        let first = &s.who(1031)[0];
        assert_eq!(
            (first.title.as_str(), first.live),
            ("Agent memory repo", true)
        );

        // A day after a session was last seen, it is off the list and its
        // number goes to the next new session.
        let later = 1003 + PRESENCE_KEPT_SECS + 10;
        s.check_in(&p("claude", "a", true, later)).unwrap();
        assert_eq!(s.who(later).len(), 1);
        assert_eq!(s.check_in(&p("claude", "d", true, later)).unwrap(), "2");
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
