//! Pairing: let an agent collect its own token with a one-time code.
//!
//! `lucid-dream pair grok` prints a short code. The agent (or the relay it
//! builds) sends that code to `/pair` once and gets its token back, so the
//! user never copies a long-lived token by hand. A code works once, for a
//! limited time, and only for the agent it was made for. A wrong guess costs
//! the guesser nothing and the owner nothing: the code is far too long to
//! guess before it expires, and cancelling it on wrong guesses would let any
//! stranger cancel a setup in progress.
//!
//! Waiting codes live in `<root>/.lucid/pair/`, one file per agent and
//! label, holding only the code's hash.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::{json, Value};

use crate::tokens::Tokens;
use crate::util::{sha256_hex, unix_now, valid_name};

/// How long a code can be used, by default.
pub const DEFAULT_MINUTES: i64 = 60;
/// Letters and digits that are hard to confuse when read aloud or retyped.
const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

/// One at a time: a code must not be accepted twice by two requests at once.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn dir(root: &Path) -> PathBuf {
    root.join(".lucid").join("pair")
}

fn file(root: &Path, agent: &str, label: &str) -> PathBuf {
    dir(root).join(format!("{agent}.{label}.json"))
}

/// The code as typed, without its dashes, spaces and case.
fn normal(code: &str) -> String {
    code.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// Make a code for `agent/label`, replacing any that was waiting. The code
/// is returned to be shown once; only its hash is kept.
pub fn create(root: &Path, agent: &str, label: &str, minutes: i64) -> Result<String, String> {
    if !valid_name(agent) || !valid_name(label) {
        return Err(format!("bad agent or label: {agent}/{label}"));
    }
    // 15 characters of a 32-letter alphabet: 75 bits, far beyond guessing.
    let mut bytes = [0u8; 15];
    getrandom::fill(&mut bytes).map_err(|e| format!("no random source: {e}"))?;
    let letters: String = bytes
        .iter()
        .map(|b| ALPHABET[(b % 32) as usize] as char)
        .collect();
    let code = format!(
        "LD-{}-{}-{}",
        &letters[..5],
        &letters[5..10],
        &letters[10..]
    );
    let entry = json!({
        "hash": sha256_hex(normal(&code).as_bytes()),
        "expires": unix_now() + minutes.clamp(1, 24 * 60) * 60,
    });
    let path = file(root, agent, label);
    let io = |e: std::io::Error| format!("{}: {e}", path.display());
    fs::create_dir_all(dir(root)).map_err(io)?;
    let _ = fs::remove_file(&path);
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut out = options.open(&path).map_err(io)?;
    std::io::Write::write_all(&mut out, entry.to_string().as_bytes()).map_err(io)?;
    Ok(code)
}

/// Codes still waiting, as (agent, label, minutes left).
pub fn waiting(root: &Path) -> Vec<(String, String, i64)> {
    let now = unix_now();
    let mut found = Vec::new();
    for entry in fs::read_dir(dir(root)).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some((agent, label)) = name
            .strip_suffix(".json")
            .and_then(|stem| stem.split_once('.'))
        else {
            continue;
        };
        let expires = read(&entry.path()).map_or(0, |v| v["expires"].as_i64().unwrap_or(0));
        if expires > now {
            found.push((
                agent.to_string(),
                label.to_string(),
                (expires - now + 59) / 60,
            ));
        }
    }
    found.sort();
    found
}

fn read(path: &Path) -> Option<Value> {
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
}

/// What a caller gets for a right code.
#[derive(Debug, PartialEq)]
pub struct Paired {
    pub agent: String,
    pub label: String,
    pub token: String,
}

/// Exchange a code for the token of the agent it was made for. `only`
/// limits which agent may pair here, as on a door. The same answer is given
/// whatever was wrong, so a guesser learns nothing.
pub fn redeem(
    root: &Path,
    tokens: &Path,
    only: Option<&(String, String)>,
    code: &str,
) -> Result<Paired, String> {
    const REFUSED: &str = "this pairing code is wrong, used or expired; make a new one with \
                           `lucid-dream pair <agent>`";
    let _gate = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let given = sha256_hex(normal(code).as_bytes());
    let now = unix_now();
    let mut matched = None;
    for entry in fs::read_dir(dir(root)).into_iter().flatten().flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some((agent, label)) = name
            .strip_suffix(".json")
            .and_then(|stem| stem.split_once('.'))
        else {
            continue;
        };
        let Some(pairing) = read(&path) else {
            continue;
        };
        if pairing["expires"].as_i64().unwrap_or(0) <= now {
            let _ = fs::remove_file(&path);
            continue;
        }
        if only.is_some_and(|(a, l)| (a.as_str(), l.as_str()) != (agent, label)) {
            continue;
        }
        let hash = pairing["hash"].as_str().unwrap_or("");
        if same(hash.as_bytes(), given.as_bytes()) {
            matched = Some((path, agent.to_string(), label.to_string()));
        }
    }
    let Some((path, agent, label)) = matched else {
        return Err(REFUSED.into());
    };
    // Used up before the token exists, so it can never be exchanged twice.
    fs::remove_file(&path).map_err(|_| REFUSED.to_string())?;
    let token = Tokens::add(tokens, &agent, &label)?;
    Ok(Paired {
        agent,
        label,
        token,
    })
}

fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::tests::Temp;

    #[test]
    fn a_code_gives_its_agent_a_token_once() {
        let t = Temp::new();
        let root = t.store().root().to_path_buf();
        let tokens = crate::tokens::path(&root);
        let code = create(&root, "grok", "default", 60).unwrap();
        assert!(code.starts_with("LD-") && code.len() == 20);
        assert_eq!(waiting(&root), [("grok".into(), "default".into(), 60)]);
        // The file holds the hash, not the code.
        let saved = fs::read_to_string(file(&root, "grok", "default")).unwrap();
        assert!(!saved.contains(&normal(&code)));

        // Typed loosely, it still works; and the token it returns is live.
        let loose = code.to_lowercase().replace('-', " ");
        let paired = redeem(&root, &tokens, None, &loose).unwrap();
        assert_eq!(
            (paired.agent.as_str(), paired.label.as_str()),
            ("grok", "default")
        );
        let known = Tokens::load(&tokens).unwrap();
        assert_eq!(known.identify(&paired.token), Some(("grok", "default")));
        // Once only.
        assert!(redeem(&root, &tokens, None, &code).is_err());
        assert!(waiting(&root).is_empty());
    }

    #[test]
    fn a_door_pairs_only_its_own_agent_and_wrong_guesses_cancel_nothing() {
        let t = Temp::new();
        let root = t.store().root().to_path_buf();
        let tokens = crate::tokens::path(&root);
        let grok = create(&root, "grok", "default", 60).unwrap();
        let chatgpt = create(&root, "chatgpt", "default", 60).unwrap();
        let grok_door = ("grok".to_string(), "default".to_string());

        // ChatGPT's code is no use at Grok's door.
        assert!(redeem(&root, &tokens, Some(&grok_door), &chatgpt).is_err());
        assert!(redeem(&root, &tokens, Some(&grok_door), &grok).is_ok());

        // Wrong guesses, however many, do not cancel a code that is waiting.
        for _ in 0..50 {
            assert!(redeem(&root, &tokens, None, "LD-AAAAA-AAAAA-AAAAA").is_err());
        }
        assert_eq!(waiting(&root).len(), 1);
        assert!(redeem(&root, &tokens, None, &chatgpt).is_ok());
    }

    #[test]
    fn an_expired_code_is_refused() {
        let t = Temp::new();
        let root = t.store().root().to_path_buf();
        let code = create(&root, "grok", "default", 60).unwrap();
        let path = file(&root, "grok", "default");
        let mut entry = read(&path).unwrap();
        entry["expires"] = json!(unix_now() - 1);
        fs::write(&path, entry.to_string()).unwrap();
        assert!(redeem(&root, &crate::tokens::path(&root), None, &code).is_err());
        assert!(!path.exists());
    }
}
