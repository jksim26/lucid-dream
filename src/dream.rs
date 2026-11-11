//! The dream job: hand the day's raw notes to an agent, then check and commit
//! what it did. Git is the undo: one commit before the dream, one after.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};

use crate::util::{git, Utc};

const PROMPT: &str = include_str!("dream_prompt.md");
const INDEX_LINE_LIMIT: usize = 200;

/// Tools the default agent may use. It can edit files and move them; nothing else.
const CLAUDE_TOOLS: [&str; 7] = [
    "Read",
    "Write",
    "Edit",
    "Glob",
    "Grep",
    "Bash(git mv *)",
    "Bash(mkdir *)",
];

pub fn run(root: &Path, dry_run: bool) -> Result<(), String> {
    git(root, &["rev-parse", "--git-dir"]).map_err(|_| {
        format!(
            "{} is not a git repository; create the store with `lucid-dream init`",
            root.display()
        )
    })?;
    let today = Utc::now().date();
    let inbox = count_files(&root.join("inbox"));
    let dirty = !git(root, &["status", "--porcelain"])?.trim().is_empty();
    if inbox == 0 && !dirty {
        println!("nothing to dream: the inbox is empty and no logs are new");
        return Ok(());
    }

    let (program, args, stdin) = command(&today);
    if dry_run {
        println!("would dream over {inbox} inbox notes in {}", root.display());
        let flags: Vec<&str> = args
            .iter()
            .map(String::as_str)
            .filter(|a| !a.contains('\n'))
            .collect();
        println!(
            "would run: {program} {} (with the dream prompt)",
            flags.join(" ")
        );
        return Ok(());
    }

    if dirty {
        git(root, &["add", "-A"])?;
        git(
            root,
            &[
                "commit",
                "-q",
                "-m",
                &format!("intake: notes and logs before dream {today}"),
            ],
        )?;
    }

    println!("dreaming over {inbox} inbox notes...");
    let status = run_agent(&program, &args, stdin.as_deref(), root)
        .map_err(|e| format!("cannot run {program}: {e}"))?;
    if !status.success() {
        // Undo edits to tracked files. Untracked files stay, so a note that
        // arrived while dreaming is never lost.
        git(root, &["reset", "-q"])?;
        git(root, &["checkout", "--", "."])?;
        return Err(format!(
            "{program} failed ({status}); memory left as it was"
        ));
    }

    git(root, &["add", "-A"])?;
    let restored = restore_deleted(root)?;
    let stat = git(root, &["diff", "--cached", "--shortstat"])?;
    if stat.trim().is_empty() {
        println!("dream made no changes");
        return Ok(());
    }
    let mut message = format!("dream: {today}\n\n{}", stat.trim());
    if !restored.is_empty() {
        message.push_str(&format!(
            "\n\nPut back {} file(s) the dream deleted instead of archiving:\n{}",
            restored.len(),
            restored.join("\n")
        ));
    }
    git(root, &["commit", "-q", "-m", &message])?;

    let index_lines = fs::read_to_string(root.join("MEMORY.md"))
        .map(|t| t.lines().count())
        .unwrap_or(0);
    if index_lines > INDEX_LINE_LIMIT {
        eprintln!("warning: MEMORY.md is {index_lines} lines; sessions load only the first {INDEX_LINE_LIMIT}");
    }
    if !restored.is_empty() {
        eprintln!(
            "warning: put back {} deleted file(s); see the commit message",
            restored.len()
        );
    }
    println!(
        "dream committed:{stat}review it with: git -C {} show --stat",
        root.display()
    );
    Ok(())
}

/// The agent to run, its arguments, and what to feed its stdin. Claude gets the
/// prompt on stdin, which survives any shell or `.cmd` shim unchanged.
/// LUCID_DREAM_CMD replaces the default, with the prompt appended as its final
/// argument (e.g. `codex exec --full-auto`).
fn command(today: &str) -> (String, Vec<String>, Option<String>) {
    let prompt = format!("Today is {today}.\n\n{PROMPT}");
    let custom = std::env::var("LUCID_DREAM_CMD")
        .ok()
        .filter(|v| !v.trim().is_empty());
    if let Some(custom) = custom {
        let mut parts = custom.split_whitespace().map(str::to_string);
        let program = parts.next().unwrap_or_default();
        let mut args: Vec<String> = parts.collect();
        args.push(prompt);
        return (program, args, None);
    }
    let mut args: Vec<String> = ["-p", "--permission-mode", "acceptEdits", "--allowedTools"]
        .map(str::to_string)
        .into();
    args.extend(CLAUDE_TOOLS.map(str::to_string));
    ("claude".to_string(), args, Some(prompt))
}

fn run_agent(
    program: &str,
    args: &[String],
    stdin: Option<&str>,
    root: &Path,
) -> std::io::Result<ExitStatus> {
    let spawn = |program: &str| -> std::io::Result<Child> {
        Command::new(program)
            .args(args)
            .current_dir(root)
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::inherit()
            })
            .spawn()
    };
    let mut child = match spawn(program) {
        // On Windows an npm-installed CLI is a `.cmd` shim, which is only found by its full name.
        Err(e) if cfg!(windows) && e.kind() == std::io::ErrorKind::NotFound => {
            spawn(&format!("{program}.cmd"))?
        }
        other => other?,
    };
    if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
        pipe.write_all(text.as_bytes())?;
    }
    child.wait()
}

/// Nothing is hard-deleted: any staged deletion that is not a move is undone.
fn restore_deleted(root: &Path) -> Result<Vec<String>, String> {
    let staged = git(root, &["diff", "--cached", "--name-status", "-M", "-z"])?;
    let deleted = deleted_paths(&staged);
    for path in &deleted {
        git(root, &["reset", "-q", "HEAD", "--", path])?;
        git(root, &["checkout", "--", path])?;
    }
    Ok(deleted)
}

/// Paths with status D in `git diff --name-status -z` output. Renames and
/// copies carry two paths and are skipped.
fn deleted_paths(name_status_z: &str) -> Vec<String> {
    let mut deleted = Vec::new();
    let mut tokens = name_status_z.split('\0');
    while let Some(status) = tokens.next() {
        if status.is_empty() {
            break;
        }
        let Some(path) = tokens.next() else {
            break;
        };
        if status.starts_with('R') || status.starts_with('C') {
            tokens.next();
        } else if status == "D" {
            deleted.push(path.to_string());
        }
    }
    deleted
}

/// Visible files under `dir`, at any depth.
fn count_files(dir: &Path) -> usize {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .map(|e| {
            if e.path().is_dir() {
                count_files(&e.path())
            } else {
                1
            }
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init;
    use crate::store::tests::Temp;

    #[test]
    fn finds_pure_deletions_only() {
        let staged = "M\0MEMORY.md\0D\0notes/old.md\0R100\0inbox/a.md\0archive/inbox/a.md\0A\0notes/new.md\0D\0wiki/x y.md\0";
        assert_eq!(deleted_paths(staged), ["notes/old.md", "wiki/x y.md"]);
        assert!(deleted_paths("").is_empty());
    }

    #[test]
    fn the_default_agent_gets_the_dated_prompt_and_a_tool_allowlist() {
        let (program, args, stdin) = command("2026-11-11");
        assert_eq!(program, "claude");
        assert_eq!(args[0], "-p");
        assert!(stdin
            .unwrap()
            .starts_with("Today is 2026-11-11.\n\nYou are the dream job"));
        assert!(args.iter().all(|a| !a.contains('\n')));
        assert!(args.contains(&"Bash(git mv *)".to_string()));
        assert!(!args.iter().any(|a| a == "Bash" || a.contains("bypass")));
    }

    #[test]
    fn deleted_files_come_back_and_moves_stay() {
        let t = Temp::new();
        let store = t.0.join("store");
        init::run(&store).unwrap();
        // Tests may run where git has no identity configured.
        git(&store, &["config", "user.email", "test@example.invalid"]).unwrap();
        git(&store, &["config", "user.name", "test"]).unwrap();
        fs::write(store.join("inbox/a.md"), "note a\n").unwrap();
        fs::write(store.join("notes/keep.md"), "keep me\n").unwrap();
        git(&store, &["add", "-A"]).unwrap();
        git(&store, &["commit", "-q", "-m", "intake"]).unwrap();
        assert_eq!(count_files(&store.join("inbox")), 1);

        // What a careless dream might do: archive one file properly, delete another.
        fs::create_dir_all(store.join("archive/inbox")).unwrap();
        git(&store, &["mv", "inbox/a.md", "archive/inbox/a.md"]).unwrap();
        fs::remove_file(store.join("notes/keep.md")).unwrap();
        git(&store, &["add", "-A"]).unwrap();

        assert_eq!(restore_deleted(&store).unwrap(), ["notes/keep.md"]);
        assert_eq!(
            fs::read_to_string(store.join("notes/keep.md")).unwrap(),
            "keep me\n"
        );
        assert!(store.join("archive/inbox/a.md").exists() && !store.join("inbox/a.md").exists());
    }

    #[test]
    fn an_empty_store_has_nothing_to_dream() {
        let t = Temp::new();
        let store = t.0.join("store");
        init::run(&store).unwrap();
        assert!(run(&store, false).is_ok());
        assert!(run(&t.0.join("missing"), false).is_err());
    }
}
