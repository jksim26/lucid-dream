//! Bring old memory in: CLAUDE.md files, Claude's auto-memory folders, Obsidian
//! notes. Each file becomes one inbox note, so dream merges it like any other
//! note and nothing bypasses the voice rules. Importing twice adds nothing.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::json;

use crate::client::Client;
use crate::store::{Note, Remembered, Store, MAX_NOTE_BYTES};
use crate::util::Utc;

const SKIP_DIRS: [&str; 2] = ["node_modules", "target"];

/// Where imported notes go: straight into a store on this machine, or to a
/// server (which records them under the token's agent).
pub enum Sink<'a> {
    Local { store: &'a Store, source: &'a str },
    Remote(&'a Client),
}

#[derive(Default, Debug, PartialEq)]
pub struct Report {
    pub added: usize,
    pub duplicates: usize,
    pub skipped: usize,
}

pub fn run(paths: &[String], sink: &Sink, said_by: &str) -> Result<Report, String> {
    let mut files = Vec::new();
    for path in paths {
        let path = Path::new(path);
        if path.is_file() {
            files.push(path.to_path_buf());
        } else if path.is_dir() {
            collect(path, &mut files).map_err(|e| format!("{}: {e}", path.display()))?;
        } else {
            return Err(format!("no such file or folder: {}", path.display()));
        }
    }
    files.sort();

    let mut report = Report::default();
    for file in files {
        let shown = file.display();
        let text = match fs::read_to_string(&file) {
            Ok(text) if text.trim().is_empty() => {
                report.skipped += 1;
                continue;
            }
            Ok(text) if text.trim().len() > MAX_NOTE_BYTES => {
                eprintln!("skipped (over {MAX_NOTE_BYTES} bytes, split it first): {shown}");
                report.skipped += 1;
                continue;
            }
            Ok(text) => text,
            Err(e) => {
                eprintln!("skipped ({e}): {shown}");
                report.skipped += 1;
                continue;
            }
        };
        let topic = file
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let origin = shown.to_string();
        let new = match sink {
            Sink::Local { store, source } => {
                let note = Note {
                    source,
                    said_by,
                    text: &text,
                    topic: Some(&topic),
                    origin: Some(&origin),
                };
                let done = store
                    .remember(&note, &Utc::now())
                    .map_err(|e| format!("{shown}: {e}"))?;
                matches!(done, Remembered::New(_))
            }
            Sink::Remote(client) => {
                let mut args = json!({ "text": text, "topic": topic, "origin": origin });
                // A server lets a caller speak for the user or for itself. An
                // agent's own old notes go in as the caller's.
                if said_by == "user" {
                    args["said_by"] = json!(said_by);
                }
                let reply = client
                    .call("remember", args)
                    .map_err(|e| format!("{shown}: {e}"))?;
                !reply.starts_with("already")
            }
        };
        if new {
            report.added += 1;
        } else {
            report.duplicates += 1;
        }
    }
    Ok(report)
}

/// Old memory found in the places agents are known to keep it: what it is,
/// whose words it holds, and the files.
pub struct Found {
    pub what: &'static str,
    pub said_by: &'static str,
    pub files: Vec<PathBuf>,
}

/// Look for memory this machine's agents already have. `claude` is Claude
/// Code's folder (`~/.claude`) and `codex` the Codex CLI's (`~/.codex`).
/// Instruction files are the user's own words; Claude's memory folders hold
/// what Claude noted, so those go in as Claude's view. Each project's
/// `MEMORY.md` is only an index of the files beside it, so it is left out.
pub fn discover(claude: &Path, codex: &Path) -> Vec<Found> {
    let existing = |file: PathBuf| file.is_file().then_some(file).into_iter().collect();
    let mut notes = Vec::new();
    for project in fs::read_dir(claude.join("projects"))
        .into_iter()
        .flatten()
        .flatten()
    {
        let _ = collect(&project.path().join("memory"), &mut notes);
    }
    notes.retain(|file| file.file_name().is_some_and(|name| name != "MEMORY.md"));
    notes.sort();
    let found = vec![
        Found {
            what: "Claude Code's instructions (CLAUDE.md)",
            said_by: "user",
            files: existing(claude.join("CLAUDE.md")),
        },
        Found {
            what: "Claude Code's own memory notes, from every project",
            said_by: "claude",
            files: notes,
        },
        Found {
            what: "Codex's instructions (AGENTS.md)",
            said_by: "user",
            files: existing(codex.join("AGENTS.md")),
        },
    ];
    found.into_iter().filter(|f| !f.files.is_empty()).collect()
}

/// Every Markdown file under `dir`, skipping hidden entries and build folders.
fn collect(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || SKIP_DIRS.contains(&name.as_str()) {
            continue;
        }
        let kind = entry.file_type()?;
        if kind.is_dir() {
            collect(&entry.path(), out)?;
        } else if kind.is_file() && name.to_ascii_lowercase().ends_with(".md") {
            out.push(entry.path());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::tests::Temp;

    #[test]
    fn finds_old_memory_where_agents_keep_it() {
        let t = Temp::new();
        let (claude, codex) = (t.0.join("claude"), t.0.join("codex"));
        let memory = claude.join("projects/-Users-me-app/memory");
        fs::create_dir_all(&memory).unwrap();
        fs::create_dir_all(claude.join("projects/-Users-me-empty")).unwrap();
        fs::create_dir_all(&codex).unwrap();
        fs::write(claude.join("CLAUDE.md"), "Use Rust.").unwrap();
        fs::write(memory.join("MEMORY.md"), "- [x](x.md)").unwrap();
        fs::write(memory.join("x.md"), "The app ships on Fridays.").unwrap();
        fs::write(claude.join("projects/-Users-me-app/session.jsonl"), "{}").unwrap();
        let found = discover(&claude, &codex);
        let shape: Vec<(&str, usize)> = found.iter().map(|f| (f.said_by, f.files.len())).collect();
        // No AGENTS.md, so Codex is not listed; the index file is left out.
        assert_eq!(shape, [("user", 1), ("claude", 1)]);
        assert!(found[1].files[0].ends_with("x.md"));
        assert!(discover(&t.0.join("nowhere"), &codex).is_empty());
    }

    #[test]
    fn imports_markdown_once() {
        let t = Temp::new();
        let old = t.0.join("old");
        fs::create_dir_all(old.join("sub")).unwrap();
        fs::create_dir_all(old.join(".obsidian")).unwrap();
        fs::create_dir_all(old.join("node_modules")).unwrap();
        fs::write(old.join("CLAUDE.md"), "Always use Rust.").unwrap();
        fs::write(old.join("sub/trip.md"), "Tokyo in May.").unwrap();
        fs::write(old.join("sub/empty.md"), "  \n").unwrap();
        fs::write(old.join("sub/photo.png"), "png").unwrap();
        fs::write(old.join(".obsidian/app.md"), "config").unwrap();
        fs::write(old.join("node_modules/readme.md"), "dep").unwrap();

        let store = t.store();
        let sink = Sink::Local {
            store: &store,
            source: "import",
        };
        let paths = [old.to_string_lossy().into_owned()];
        let first = run(&paths, &sink, "user").unwrap();
        assert_eq!(
            first,
            Report {
                added: 2,
                duplicates: 0,
                skipped: 1
            }
        );
        let again = run(&paths, &sink, "user").unwrap();
        assert_eq!(
            again,
            Report {
                added: 0,
                duplicates: 2,
                skipped: 1
            }
        );

        let inbox = store.list("inbox").unwrap();
        assert_eq!(inbox.len(), 2);
        let bodies: Vec<String> = inbox.iter().map(|e| store.read(&e.path).unwrap()).collect();
        let body = bodies
            .iter()
            .find(|b| b.contains("Always use Rust."))
            .unwrap();
        assert!(body.contains("source: import\nsaid_by: user\n"));
        assert!(body.contains("\ntopic: CLAUDE\norigin: "));
    }

    #[test]
    fn a_missing_path_is_an_error() {
        let t = Temp::new();
        let store = t.store();
        let sink = Sink::Local {
            store: &store,
            source: "import",
        };
        assert!(run(&["/no/such/place".to_string()], &sink, "user").is_err());
    }
}
