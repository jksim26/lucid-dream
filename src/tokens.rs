//! One bearer token per agent. The token tells the server whose voice a note is in.
//!
//! Tokens live in `<root>/.lucid/tokens`, one `agent sha256` pair per line. Only
//! the hash is stored, and `.lucid/` is hidden from the read tools and from git.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::util::{sha256_hex, valid_name};

/// Names that mean something else in a note's `said_by` or `source`.
const RESERVED: [&str; 2] = ["user", "local"];

pub struct Tokens(Vec<(String, String)>);

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
                let mut parts = line.split_whitespace();
                Some((parts.next()?.to_string(), parts.next()?.to_string()))
            })
            .collect();
        Ok(Tokens(entries))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn agents(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|(agent, _)| agent.as_str())
    }

    /// The agent this token belongs to, if any.
    pub fn identify(&self, token: &str) -> Option<&str> {
        let hash = sha256_hex(token.as_bytes());
        self.0
            .iter()
            .find(|(_, stored)| constant_time_eq(stored.as_bytes(), hash.as_bytes()))
            .map(|(agent, _)| agent.as_str())
    }

    /// Create (or replace) the agent's token and return it. It is shown once.
    pub fn add(path: &Path, agent: &str) -> Result<String, String> {
        if !valid_name(agent) || RESERVED.contains(&agent) {
            return Err(format!(
                "bad agent name {agent:?}: use a-z, 0-9 and dashes; not \"user\" or \"local\""
            ));
        }
        let mut bytes = [0u8; 24];
        getrandom::fill(&mut bytes).map_err(|e| format!("no random source: {e}"))?;
        let token = format!(
            "ld_{}",
            bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
        );

        let mut tokens = Tokens::load(path).map_err(|e| e.to_string())?;
        tokens.0.retain(|(name, _)| name != agent);
        tokens
            .0
            .push((agent.to_string(), sha256_hex(token.as_bytes())));
        let text: String = tokens.0.iter().map(|(a, h)| format!("{a} {h}\n")).collect();
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        fs::write(path, text).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
        }
        Ok(token)
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
    fn a_token_identifies_its_agent() {
        let t = Temp::new();
        let file = path(&t.0);
        let claude = Tokens::add(&file, "claude").unwrap();
        let codex = Tokens::add(&file, "codex").unwrap();
        let tokens = Tokens::load(&file).unwrap();
        assert_eq!(tokens.identify(&claude), Some("claude"));
        assert_eq!(tokens.identify(&codex), Some("codex"));
        assert_eq!(tokens.identify("ld_wrong"), None);
        assert!(!fs::read_to_string(&file).unwrap().contains(&claude));
    }

    #[test]
    fn a_new_token_replaces_the_old_one() {
        let t = Temp::new();
        let file = path(&t.0);
        let old = Tokens::add(&file, "grok").unwrap();
        let new = Tokens::add(&file, "grok").unwrap();
        let tokens = Tokens::load(&file).unwrap();
        assert_eq!(tokens.identify(&old), None);
        assert_eq!(tokens.identify(&new), Some("grok"));
        assert_eq!(tokens.agents().count(), 1);
    }

    #[test]
    fn reserved_and_bad_names_are_refused() {
        let t = Temp::new();
        for bad in ["user", "local", "Bad Name", ""] {
            assert!(Tokens::add(&path(&t.0), bad).is_err(), "{bad}");
        }
    }
}
