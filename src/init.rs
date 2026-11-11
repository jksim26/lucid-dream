//! Create a memory store: the folders, an empty index, and a git repository.
//! Safe to run on an existing store; it only adds what is missing.

use std::fs;
use std::path::Path;

use crate::util::git;

const FOLDERS: [&str; 7] = [
    "inbox", "logs", "messages", "notes", "wiki", "digest", "archive",
];

const MEMORY: &str = "# MEMORY\n\nThe index every agent loads at session start. Dream rebuilds it; \
keep it under 200 lines.\nEach line points at a note or wiki page.\n\n<!-- empty until the first dream -->\n";

const OPEN_QUESTIONS: &str =
    "# Open questions\n\nDecisions of yours that conflict with each other. \
Dream lists them here; you settle them.\n\n<!-- none yet -->\n";

/// `.lucid/` holds the token hashes; it never goes into git.
const GITIGNORE: &str = ".lucid/\n.DS_Store\n";

/// Store every file byte for byte. Without this, git on Windows rewrites line
/// endings, and notes would differ between the disk and the history.
const GITATTRIBUTES: &str = "* -text\n";

pub fn run(dir: &Path) -> Result<(), String> {
    let io = |e: std::io::Error| format!("{}: {e}", dir.display());
    for folder in FOLDERS {
        let path = dir.join(folder);
        fs::create_dir_all(&path).map_err(io)?;
        // Git does not track empty folders.
        let keep = path.join(".gitkeep");
        if !keep.exists() {
            fs::write(keep, "").map_err(io)?;
        }
    }
    fs::create_dir_all(dir.join(".lucid")).map_err(io)?;
    for (name, text) in [
        ("MEMORY.md", MEMORY),
        ("open-questions.md", OPEN_QUESTIONS),
        (".gitignore", GITIGNORE),
        (".gitattributes", GITATTRIBUTES),
    ] {
        let path = dir.join(name);
        if !path.exists() {
            fs::write(path, text).map_err(io)?;
        }
    }
    if !dir.join(".git").exists() {
        git(dir, &["init", "-q", "-b", "main"])?;
        git(dir, &["add", "-A"])?;
        // No identity configured yet is fine: the store exists, the first dream commits it.
        if let Err(e) = git(dir, &["commit", "-q", "-m", "Start memory store"]) {
            eprintln!("note: created the store but could not commit ({e})");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::tests::Temp;
    use crate::store::Store;

    #[test]
    fn creates_a_store_and_leaves_an_existing_one_alone() {
        let t = Temp::new();
        let dir = t.0.join("store");
        run(&dir).unwrap();
        assert!(dir.join(".git").is_dir() && dir.join("inbox/.gitkeep").exists());
        assert!(git(&dir, &["check-ignore", ".lucid/tokens"]).is_ok());

        fs::write(dir.join("MEMORY.md"), "mine\n").unwrap();
        run(&dir).unwrap();
        let store = Store::open(&dir).unwrap();
        assert_eq!(store.read("MEMORY.md").unwrap(), "mine\n");
        let files: Vec<String> = store
            .list("")
            .unwrap()
            .into_iter()
            .map(|e| e.path)
            .collect();
        assert_eq!(files, ["MEMORY.md", "open-questions.md"]);
    }
}
