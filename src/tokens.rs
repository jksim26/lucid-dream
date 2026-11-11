//! Bearer tokens. A token tells the server which agent is speaking, and so
//! whose voice a note is in.
//!
//! An agent can hold several tokens, each with a label (`claude/default` for
//! its devices, `claude/phone` for the chat app), so one can be revoked without
//! disturbing the others. Tokens live in `<root>/.lucid/tokens`, one
//! `agent label sha256` line each. Only the hash is stored, and `.lucid/` is
//! hidden from the read tools and from git.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::util::{sha256_hex, valid_name};

pub const DEFAULT_LABEL: &str = "default";

/// Names that mean something else in a note's `said_by` or `source`.
const RESERVED: [&str; 3] = ["user", "local", "all"];

struct Entry {
    agent: String,
    label: String,
    hash: String,
}

pub struct Tokens(Vec<Entry>);

pub fn path(root: &Path) -> PathBuf {
    root.join(".lucid").join("tokens")
}

impl Tokens {
    pub fn load(path: &Path) -> io::Result<Tokens> {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(e),
        };
        let entries = text
            .lines()
            .filter_map(|line| {
                let parts: Vec<&str> = line.split_whitespace().collect();
                let (agent, label, hash) = match parts.as_slice() {
                    [agent, label, hash] => (agent, label, hash),
                    // Files written before labels existed.
                    [agent, hash] => (agent, &DEFAULT_LABEL, hash),
                    _ => return None,
                };
                Some(Entry {
                    agent: agent.to_string(),
                    label: label.to_string(),
                    hash: hash.to_string(),
                })
            })
            .collect();
        Ok(Tokens(entries))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Each agent that holds at least one token, once.
    pub fn agents(&self) -> impl Iterator<Item = &str> {
        self.0
            .iter()
            .enumerate()
            .filter(|(i, e)| !self.0[..*i].iter().any(|earlier| earlier.agent == e.agent))
            .map(|(_, e)| e.agent.as_str())
    }

    /// Every token as (agent, label).
    pub fn entries(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|e| (e.agent.as_str(), e.label.as_str()))
    }

    pub fn has(&self, agent: &str, label: &str) -> bool {
        self.entries().any(|entry| entry == (agent, label))
    }

    /// The (agent, label) this token belongs to, if any.
    pub fn identify(&self, token: &str) -> Option<(&str, &str)> {
        let hash = sha256_hex(token.as_bytes());
        self.0
            .iter()
            .find(|e| constant_time_eq(e.hash.as_bytes(), hash.as_bytes()))
            .map(|e| (e.agent.as_str(), e.label.as_str()))
    }

    /// Create (or replace) the token `agent/label` and return it. It is shown once.
    pub fn add(path: &Path, agent: &str, label: &str) -> Result<String, String> {
        if !valid_name(agent) || RESERVED.contains(&agent) {
            return Err(format!(
                "bad agent name {agent:?}: use a-z, 0-9 and dashes; not \"user\", \"local\" or \"all\""
            ));
        }
        if !valid_name(label) {
            return Err(format!("bad label {label:?}: use a-z, 0-9 and dashes"));
        }
        let mut bytes = [0u8; 24];
        getrandom::fill(&mut bytes).map_err(|e| format!("no random source: {e}"))?;
        let token = format!(
            "ld_{}",
            bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
        );

        let mut tokens = Tokens::load(path).map_err(|e| e.to_string())?;
        tokens.0.retain(|e| !(e.agent == agent && e.label == label));
        tokens.0.push(Entry {
            agent: agent.to_string(),
            label: label.to_string(),
            hash: sha256_hex(token.as_bytes()),
        });
        tokens.save(path)?;
        Ok(token)
    }

    /// Revoke the token `agent/label`. Returns whether it existed.
    pub fn remove(path: &Path, agent: &str, label: &str) -> Result<bool, String> {
        let mut tokens = Tokens::load(path).map_err(|e| e.to_string())?;
        let before = tokens.0.len();
        tokens.0.retain(|e| !(e.agent == agent && e.label == label));
        if tokens.0.len() == before {
            return Ok(false);
        }
        tokens.save(path)?;
        Ok(true)
    }

    fn save(&self, path: &Path) -> Result<(), String> {
        let text: String = self
            .0
            .iter()
            .map(|e| format!("{} {} {}\n", e.agent, e.label, e.hash))
            .collect();
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        fs::write(path, text).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::tests::Temp;

    #[test]
    fn a_token_identifies_its_agent_and_label() {
        let t = Temp::new();
        let file = path(&t.0);
        let devices = Tokens::add(&file, "claude", DEFAULT_LABEL).unwrap();
        let phone = Tokens::add(&file, "claude", "phone").unwrap();
        let codex = Tokens::add(&file, "codex", DEFAULT_LABEL).unwrap();
        let tokens = Tokens::load(&file).unwrap();
        assert_eq!(tokens.identify(&devices), Some(("claude", "default")));
        assert_eq!(tokens.identify(&phone), Some(("claude", "phone")));
        assert_eq!(tokens.identify(&codex), Some(("codex", "default")));
        assert_eq!(tokens.identify("ld_wrong"), None);
        assert_eq!(tokens.agents().collect::<Vec<_>>(), ["claude", "codex"]);
        assert!(!fs::read_to_string(&file).unwrap().contains(&phone));
    }

    #[test]
    fn one_token_is_replaced_or_revoked_without_touching_the_others() {
        let t = Temp::new();
        let file = path(&t.0);
        let devices = Tokens::add(&file, "claude", DEFAULT_LABEL).unwrap();
        let old_phone = Tokens::add(&file, "claude", "phone").unwrap();
        let new_phone = Tokens::add(&file, "claude", "phone").unwrap();
        let tokens = Tokens::load(&file).unwrap();
        assert_eq!(tokens.identify(&old_phone), None);
        assert_eq!(tokens.identify(&new_phone), Some(("claude", "phone")));

        assert!(Tokens::remove(&file, "claude", "phone").unwrap());
        assert!(!Tokens::remove(&file, "claude", "phone").unwrap());
        let tokens = Tokens::load(&file).unwrap();
        assert_eq!(tokens.identify(&new_phone), None);
        assert_eq!(tokens.identify(&devices), Some(("claude", "default")));
    }

    #[test]
    fn files_without_labels_still_load() {
        let t = Temp::new();
        let file = path(&t.0);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, format!("grok {}\n", sha256_hex(b"ld_old"))).unwrap();
        assert_eq!(
            Tokens::load(&file).unwrap().identify("ld_old"),
            Some(("grok", "default"))
        );
    }

    #[test]
    fn reserved_and_bad_names_are_refused() {
        let t = Temp::new();
        for bad in ["user", "local", "all", "Bad Name", ""] {
            assert!(
                Tokens::add(&path(&t.0), bad, DEFAULT_LABEL).is_err(),
                "{bad}"
            );
        }
        assert!(Tokens::add(&path(&t.0), "claude", "my phone").is_err());
    }
}
