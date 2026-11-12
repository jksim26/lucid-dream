//! The dream job: hand the day's raw notes to an agent, then check and commit
//! what it did. Git is the undo: one commit before the dream, one after.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};

use crate::util::{git, Utc};

const PROMPT: &str = include_str!("dream_prompt.md");
const INDEX_LINE_LIMIT: usize = 200;

/// Set for the agent that dream runs; `lucid-dream hook` does nothing when it is.
pub const DREAMING_ENV: &str = "LUCID_DREAMING";

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

    let attempts = commands(&today);
    if dry_run {
        println!("would dream over {inbox} inbox notes in {}", root.display());
        for (i, (program, args, _)) in attempts.iter().enumerate() {
            let flags: Vec<&str> = args
                .iter()
                .map(String::as_str)
                .filter(|a| !a.contains('\n'))
                .collect();
            let when = if i == 0 { "would run" } else { "if that fails" };
            println!(
                "{when}: {program} {} (with the dream prompt)",
                flags.join(" ")
            );
        }
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
    let noted = inbox_files(root);
    first_success(&attempts, root)?;
    // An agent that exits cleanly having written nothing (out of quota, say)
    // has not dreamt: leave its notes in the inbox for the next run.
    let worked = !git(root, &["status", "--porcelain"])?.trim().is_empty();
    let archived = if worked {
        archive_inbox(root, &noted)?
    } else {
        Vec::new()
    };
    if !archived.is_empty() {
        println!(
            "archived {} inbox note(s) the agent left behind",
            archived.len()
        );
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

/// One agent to try: its program, arguments, and what to feed its stdin.
type Attempt = (String, Vec<String>, Option<String>);

/// The agents to try, in order. The default is Claude alone. LUCID_DREAM_CMD
/// replaces it, and may list several commands separated by `||`; the next is
/// tried when one fails, e.g. `claude || codex exec --sandbox workspace-write`.
fn commands(today: &str) -> Vec<Attempt> {
    let custom = std::env::var("LUCID_DREAM_CMD").unwrap_or_default();
    attempts(&custom, today)
}

fn attempts(custom: &str, today: &str) -> Vec<Attempt> {
    let prompt = format!("Today is {today}.\n\n{PROMPT}");
    let mut list: Vec<Attempt> = custom
        .split("||")
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .map(|c| {
            // A bare `claude` means the built-in command with its tool allowlist.
            if c == "claude" {
                return claude(&prompt);
            }
            let mut parts = c.split_whitespace().map(str::to_string);
            let program = parts.next().unwrap_or_default();
            let mut args: Vec<String> = parts.collect();
            // The prompt is the last argument, as e.g. `codex exec` expects.
            args.push(prompt.clone());
            (program, args, None)
        })
        .collect();
    if list.is_empty() {
        list.push(claude(&prompt));
    }
    list
}

/// Claude gets the prompt on stdin, which survives any shell or `.cmd` shim
/// unchanged, and may only edit files and move them.
fn claude(prompt: &str) -> Attempt {
    let mut args: Vec<String> = ["-p", "--permission-mode", "acceptEdits", "--allowedTools"]
        .map(str::to_string)
        .into();
    args.extend(CLAUDE_TOOLS.map(str::to_string));
    ("claude".to_string(), args, Some(prompt.to_string()))
}

/// Try each agent in turn: when the first is out of quota or not installed,
/// the next one dreams instead. A failed attempt's edits are undone first.
fn first_success(attempts: &[Attempt], root: &Path) -> Result<(), String> {
    let mut failures = Vec::new();
    for (program, args, stdin) in attempts {
        let failure = match run_agent(program, args, stdin.as_deref(), root) {
            Ok(status) if status.success() => return Ok(()),
            Ok(status) => format!("{program} failed ({status})"),
            Err(e) => format!("cannot run {program}: {e}"),
        };
        // Undo edits to tracked files. Untracked files stay, so a note that
        // arrived while dreaming is never lost.
        git(root, &["reset", "-q"])?;
        git(root, &["checkout", "--", "."])?;
        eprintln!("{failure}");
        failures.push(failure);
    }
    Err(format!("{}; memory left as it was", failures.join("; ")))
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
            // The dream is not a working session: this tells our own hooks,
            // which the agent inherits from the user's settings, to stay out.
            .env(DREAMING_ENV, "1")
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

/// Names of the notes directly in `inbox/`.
fn inbox_files(root: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(root.join("inbox"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| !name.starts_with('.'))
        .collect();
    names.sort();
    names
}

/// Move the notes the dream was given out of the inbox, unchanged, if the
/// agent did not: some agents run in a sandbox that refuses `git mv`, and a
/// note left behind would be dreamt over again every night. Notes that
/// arrived while dreaming are not in `noted` and stay for next time.
fn archive_inbox(root: &Path, noted: &[String]) -> Result<Vec<String>, String> {
    let (inbox, archive) = (root.join("inbox"), root.join("archive").join("inbox"));
    let mut moved = Vec::new();
    for name in noted {
        let (from, to) = (inbox.join(name), archive.join(name));
        if !from.is_file() || to.exists() {
            continue;
        }
        fs::create_dir_all(&archive)
            .and_then(|_| fs::rename(&from, &to))
            .map_err(|e| format!("cannot archive inbox/{name}: {e}"))?;
        moved.push(name.clone());
    }
    Ok(moved)
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
        let list = attempts("", "2026-11-11");
        assert_eq!(list.len(), 1);
        let (program, args, stdin) = list.into_iter().next().unwrap();
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
    fn a_list_of_agents_is_tried_in_order() {
        let list = attempts("claude || codex exec -s workspace-write ||", "2026-11-11");
        assert_eq!(list.len(), 2);
        // The bare name keeps Claude's allowlist and stdin prompt.
        assert!(list[0].1.contains(&"--allowedTools".to_string()) && list[0].2.is_some());
        assert_eq!(list[1].0, "codex");
        assert_eq!(list[1].1[..3], ["exec", "-s", "workspace-write"]);
        assert!(list[1].1[3].starts_with("Today is 2026-11-11.") && list[1].2.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn the_next_agent_dreams_when_the_first_fails() {
        let t = Temp::new();
        let store = t.0.join("store");
        init::run(&store).unwrap();
        git(&store, &["config", "user.email", "test@example.invalid"]).unwrap();
        git(&store, &["config", "user.name", "test"]).unwrap();
        fs::write(store.join("inbox/a.md"), "note a\n").unwrap();
        git(&store, &["add", "-A"]).unwrap();
        git(&store, &["commit", "-q", "-m", "intake"]).unwrap();

        fs::write(store.join("notes/keep.md"), "before\n").unwrap();
        git(&store, &["add", "-A"]).unwrap();
        git(&store, &["commit", "-q", "-m", "keep"]).unwrap();

        use std::os::unix::fs::PermissionsExt;
        let script = |name: &str, body: &str| {
            let file = t.0.join(name);
            fs::write(&file, format!("#!/bin/sh\n{body}\n")).unwrap();
            fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).unwrap();
            file.display().to_string()
        };
        // The first agent spoils a tracked file and fails; the second succeeds.
        let bad = script("bad.sh", "echo spoiled > notes/keep.md; exit 3");
        let good = script("good.sh", "echo dreamt > notes/dreamt.md");
        let list = attempts(
            &format!("{bad} || missing-agent-xyz || {good}"),
            "2026-11-11",
        );
        first_success(&list, &store).unwrap();
        assert_eq!(
            fs::read_to_string(store.join("notes/keep.md")).unwrap(),
            "before\n"
        );
        assert!(store.join("notes/dreamt.md").exists());

        let failed = first_success(&attempts(&bad, "2026-11-11"), &store).unwrap_err();
        assert!(failed.contains("memory left as it was"), "{failed}");
        assert_eq!(
            fs::read_to_string(store.join("notes/keep.md")).unwrap(),
            "before\n"
        );
    }

    #[test]
    fn notes_the_agent_left_in_the_inbox_are_archived() {
        let t = Temp::new();
        let store = t.0.join("store");
        init::run(&store).unwrap();
        fs::write(store.join("inbox/a.md"), "note a\n").unwrap();
        fs::write(store.join("inbox/b.md"), "note b\n").unwrap();
        let noted = inbox_files(&store);
        assert_eq!(noted, ["a.md", "b.md"]);

        // The agent archived one itself; a third note arrived while it worked.
        fs::create_dir_all(store.join("archive/inbox")).unwrap();
        fs::rename(store.join("inbox/a.md"), store.join("archive/inbox/a.md")).unwrap();
        fs::write(store.join("inbox/late.md"), "late\n").unwrap();

        assert_eq!(archive_inbox(&store, &noted).unwrap(), ["b.md"]);
        assert_eq!(
            fs::read_to_string(store.join("archive/inbox/b.md")).unwrap(),
            "note b\n"
        );
        assert_eq!(inbox_files(&store), ["late.md"]);
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
