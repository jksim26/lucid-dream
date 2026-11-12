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
/// A folder to copy the whole memory into after each dream, if set.
pub const BACKUP_ENV: &str = "LUCID_BACKUP_DIR";
pub const BACKUP_FILE: &str = "lucid-memory.bundle";

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

/// Dream once, printing what happened. Used by `lucid-dream dream --dry-run`
/// and the tests; everything else goes through `run_recorded`.
pub fn run(root: &Path, dry_run: bool) -> Result<(), String> {
    let summary = dream(root, dry_run)?;
    if !summary.is_empty() {
        println!("{summary}");
    }
    Ok(())
}

/// How a dream went, kept in `.lucid/dream-status.json` so that `doctor` and
/// the `dream` tool can say when memory was last tidied and whether it worked.
pub struct Status {
    pub started: i64,
    /// Zero while the dream is still running.
    pub finished: i64,
    pub ok: bool,
    pub message: String,
    /// What started it: "timer", "hand", or the agent that asked.
    pub by: String,
}

/// A dream that has run this long has hung or its machine went down.
const STALE_AFTER_SECS: i64 = 2 * 3600;

fn status_file(root: &Path) -> std::path::PathBuf {
    root.join(".lucid").join("dream-status.json")
}

fn lock_file(root: &Path) -> std::path::PathBuf {
    root.join(".lucid").join("dream.lock")
}

pub fn status(root: &Path) -> Option<Status> {
    let text = fs::read_to_string(status_file(root)).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    Some(Status {
        started: v["started"].as_i64()?,
        finished: v["finished"].as_i64().unwrap_or(0),
        ok: v["ok"] == true,
        message: v["message"].as_str().unwrap_or("").to_string(),
        by: v["by"].as_str().unwrap_or("").to_string(),
    })
}

fn record(root: &Path, status: &Status) {
    let text = serde_json::json!({
        "started": status.started, "finished": status.finished, "ok": status.ok,
        "message": status.message, "by": status.by,
    });
    let _ = fs::write(status_file(root), format!("{text}\n"));
}

/// When a dream that is running right now started, if one is.
pub fn running_since(root: &Path) -> Option<i64> {
    let started: i64 = fs::read_to_string(lock_file(root))
        .ok()?
        .trim()
        .parse()
        .ok()?;
    (crate::util::unix_now() - started < STALE_AFTER_SECS).then_some(started)
}

/// A path git and the agents accept as a working directory. Windows hands
/// back `\\?\C:\...` for a canonical path, which neither does.
fn plain(root: &Path) -> std::path::PathBuf {
    match root.to_str().and_then(|s| s.strip_prefix(r"\\?\")) {
        Some(rest) if !rest.starts_with("UNC\\") => std::path::PathBuf::from(rest),
        _ => root.to_path_buf(),
    }
}

/// Take the dream lock, or say why not. Only one thread of this process is
/// ever in here, and the file is made with `create_new`, so two dreams
/// cannot both get it. A lock is cleared only once it is stale: its dream
/// hung, or the machine went down in the middle of it.
fn acquire(root: &Path) -> Result<i64, String> {
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _gate = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let now = crate::util::unix_now();
    let lock = lock_file(root);
    if let Some(dir) = lock.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let busy = |since: i64| {
        format!(
            "a dream is already running (started {} min ago)",
            (now - since) / 60
        )
    };
    if let Some(since) = running_since(root) {
        return Err(busy(since));
    }
    // Here the lock is absent or stale. Removing a stale one is safe: its
    // owner is gone. If another process takes the lock between this line and
    // the next, `create_new` fails and that dream keeps it.
    let _ = fs::remove_file(&lock);
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock)
    {
        Ok(mut file) => {
            file.write_all(now.to_string().as_bytes())
                .map_err(|e| format!("cannot write the dream lock: {e}"))?;
            Ok(now)
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(busy(now)),
        Err(e) => Err(format!("cannot take the dream lock: {e}")),
    }
}

/// Dream with the lock already held since `started`, record how it went, and
/// give the lock back.
fn run_locked(root: &Path, by: &str, started: i64) -> Result<String, String> {
    let mut status = Status {
        started,
        finished: 0,
        ok: false,
        message: String::new(),
        by: by.to_string(),
    };
    record(root, &status);
    let outcome = dream(root, false).map(|summary| {
        let folder = std::env::var_os(BACKUP_ENV).filter(|dir| !dir.is_empty());
        match folder.map(|dir| backup(root, Path::new(&dir))) {
            None => summary,
            Some(Ok(file)) => format!("{summary}\nbacked up to {file}"),
            Some(Err(e)) => format!("{summary}\nwarning: backup failed: {e}"),
        }
    });
    let _ = fs::remove_file(lock_file(root));
    status.finished = crate::util::unix_now();
    status.ok = outcome.is_ok();
    status.message = match &outcome {
        // The first line says what the dream did; a failed backup must show too.
        Ok(summary) => summary
            .lines()
            .enumerate()
            .filter(|(i, line)| *i == 0 || line.starts_with("warning: backup"))
            .map(|(_, line)| line)
            .collect::<Vec<_>>()
            .join("; "),
        Err(e) => e.clone(),
    };
    record(root, &status);
    outcome
}

/// Copy the whole memory, with its history, into `folder` as one file: a git
/// bundle, which `git clone <file> <dir>` turns back into the store. It
/// holds what git holds, so tokens and the rest of `.lucid/` are not in it.
/// The file is written beside its final name and moved into place, so a
/// syncing folder never sees a half-written backup.
pub fn backup(root: &Path, folder: &Path) -> Result<String, String> {
    let io = |e: std::io::Error| format!("{}: {e}", folder.display());
    fs::create_dir_all(folder).map_err(io)?;
    let (file, part) = (
        folder.join(BACKUP_FILE),
        folder.join(format!("{BACKUP_FILE}.part")),
    );
    let _ = fs::remove_file(&part);
    if let Err(e) = git(
        root,
        &["bundle", "create", "-q", &part.to_string_lossy(), "--all"],
    ) {
        let _ = fs::remove_file(&part);
        return Err(e);
    }
    fs::rename(&part, &file).map_err(io)?;
    Ok(file.display().to_string())
}

/// Dream once, with nothing else dreaming at the same time, and record how
/// it went. `by` says what started it.
pub fn run_recorded(root: &Path, by: &str) -> Result<String, String> {
    let root = &plain(root);
    let started = acquire(root)?;
    run_locked(root, by, started)
}

/// One line on when memory was last tidied, for people and agents.
pub fn describe(root: &Path) -> String {
    let now = crate::util::unix_now();
    if let Some(since) = running_since(root) {
        return format!(
            "a dream is running now (started {} min ago)",
            (now - since) / 60
        );
    }
    match status(root) {
        None => "no dream has been recorded yet".to_string(),
        Some(s) if s.finished == 0 => "the last dream did not finish".to_string(),
        Some(s) => {
            let ago = match (now - s.finished) / 60 {
                m if m < 90 => format!("{m} min ago"),
                m if m < 48 * 60 => format!("{} hours ago", m / 60),
                m => format!("{} days ago", m / 1440),
            };
            let how = if s.ok { "finished" } else { "FAILED" };
            format!(
                "the last dream {how} {ago}, started by {}: {}",
                s.by, s.message
            )
        }
    }
}

/// An agent may ask for a dream this often. A dream spends the user's quota,
/// and any connected agent can ask, so one that asks in a loop is refused.
const ASKED_GAP_SECS: i64 = 30 * 60;

/// Start a dream because an agent asked, without waiting for it; it takes
/// minutes. The lock is taken before this returns, so "started" is true.
pub fn start_in_background(root: &Path, by: &str) -> Result<(), String> {
    let root = plain(root);
    if let Some(last) = status(&root) {
        let ago = crate::util::unix_now() - last.started;
        if last.by != "hand" && last.by != "timer" && (0..ASKED_GAP_SECS).contains(&ago) {
            return Err(format!(
                "an agent already asked for a dream {} min ago; wait {} min, or the user can \
                 run `lucid-dream dream` by hand",
                ago / 60,
                (ASKED_GAP_SECS - ago) / 60 + 1
            ));
        }
    }
    let started = acquire(&root)?;
    let by = by.to_string();
    std::thread::spawn(move || match run_locked(&root, &by, started) {
        Ok(summary) => eprintln!("lucid-dream: dream started by {by}: {summary}"),
        Err(e) => eprintln!("lucid-dream: dream started by {by} failed: {e}"),
    });
    Ok(())
}

/// Parse `HH:MM`, a time of day on this machine's clock.
pub fn time_of_day(text: &str) -> Result<(u32, u32), String> {
    let bad = || format!("bad time `{text}`; write it as HH:MM, e.g. 03:00");
    let (h, m) = text.trim().split_once(':').ok_or_else(bad)?;
    let (h, m): (u32, u32) = (h.parse().map_err(|_| bad())?, m.parse().map_err(|_| bad())?);
    if h > 23 || m > 59 {
        return Err(bad());
    }
    Ok((h, m))
}

/// True when today's dream is due: the time of day has passed and no dream
/// has started since. So a machine that was off at that time dreams when it
/// comes back, and a dream run by hand after it counts as today's.
fn due(now: i64, todays_time: i64, last_started: Option<i64>) -> bool {
    now >= todays_time && last_started.is_none_or(|last| last < todays_time)
}

/// Dream every day at `at`, local time, for as long as the server runs.
pub fn timer(root: std::path::PathBuf, at: (u32, u32)) {
    use chrono::{Local, TimeZone};
    std::thread::spawn(move || loop {
        let now = Local::now();
        let todays_time = now
            .date_naive()
            .and_hms_opt(at.0, at.1, 0)
            .and_then(|t| Local.from_local_datetime(&t).earliest())
            .map(|t| t.timestamp());
        let last = status(&root).map(|s| s.started);
        if todays_time.is_some_and(|t| due(now.timestamp(), t, last))
            && running_since(&root).is_none()
        {
            match run_recorded(&root, "timer") {
                Ok(summary) => eprintln!("lucid-dream: nightly dream: {summary}"),
                Err(e) => eprintln!("lucid-dream: nightly dream failed: {e}"),
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(60));
    });
}

fn dream(root: &Path, dry_run: bool) -> Result<String, String> {
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
        return Ok("nothing to dream: the inbox is empty and no logs are new".into());
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
        return Ok(String::new());
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
        return Ok("dream made no changes".into());
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
    Ok(format!(
        "dream committed:{}\nreview it with: git -C {} show --stat",
        stat.trim_end(),
        root.display()
    ))
}

/// One agent to try: its program, arguments, and what to feed its stdin.
type Attempt = (String, Vec<String>, Option<String>);

/// The agents to try, in order. The default is Claude alone. LUCID_DREAM_CMD
/// replaces it, and may list several commands separated by `||`; the next is
/// tried when one fails, e.g. `claude || codex exec --sandbox workspace-write`.
fn commands(today: &str) -> Vec<Attempt> {
    let custom = std::env::var("LUCID_DREAM_CMD").unwrap_or_default();
    let model = std::env::var(MODEL_ENV).unwrap_or_default();
    attempts(&custom, &model, today)
}

fn attempts(custom: &str, model: &str, today: &str) -> Vec<Attempt> {
    let prompt = format!("Today is {today}.\n\n{PROMPT}");
    // The built-in Claude command, on the chosen model first. If Claude Code
    // on this machine does not take that model, its own default is tried.
    let built_in = || match claude_model(model) {
        Some(model) => vec![claude(&prompt, Some(model)), claude(&prompt, None)],
        None => vec![claude(&prompt, None)],
    };
    let mut list: Vec<Attempt> = custom
        .split("||")
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .flat_map(|c| {
            // A bare `claude` means the built-in command with its tool allowlist.
            if c == "claude" {
                return built_in();
            }
            let mut parts = c.split_whitespace().map(str::to_string);
            let program = parts.next().unwrap_or_default();
            let mut args: Vec<String> = parts.collect();
            // The prompt is the last argument, as e.g. `codex exec` expects.
            args.push(prompt.clone());
            vec![(program, args, None)]
        })
        .collect();
    if list.is_empty() {
        list = built_in();
    }
    list
}

/// The model the built-in Claude command asks for when none is set. Merging
/// notes does not need the largest model, and the dream runs every night.
pub const DEFAULT_MODEL: &str = "sonnet";
pub const MODEL_ENV: &str = "LUCID_DREAM_MODEL";

/// The model to pass to Claude Code: an alias (`sonnet`, `haiku`, `opus`) or
/// a full model name. `default` means leave it to Claude Code's own setting.
/// Anything that is not a plain model name is ignored, since it becomes a
/// command-line argument.
fn claude_model(model: &str) -> Option<&str> {
    let model = match model.trim() {
        "" => DEFAULT_MODEL,
        model => model,
    };
    let plain = model
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '[' | ']'));
    (model != "default" && plain && !model.starts_with('-')).then_some(model)
}

/// Claude gets the prompt on stdin, which survives any shell or `.cmd` shim
/// unchanged, and may only edit files and move them.
fn claude(prompt: &str, model: Option<&str>) -> Attempt {
    let mut args: Vec<String> = vec!["-p".to_string()];
    if let Some(model) = model {
        args.extend(["--model".to_string(), model.to_string()]);
    }
    args.extend(["--permission-mode", "acceptEdits", "--allowedTools"].map(str::to_string));
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
            // An agent given its prompt as an argument gets no input: a
            // server has no terminal for it to wait on.
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
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
        // Sonnet first; then whatever Claude Code on the machine defaults to.
        let list = attempts("", "", "2026-11-11");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].1[..3], ["-p", "--model", "sonnet"]);
        assert!(!list[1].1.contains(&"--model".to_string()));
        assert_eq!(
            attempts("", "haiku", "d")[0].1[..3],
            ["-p", "--model", "haiku"]
        );
        // `default`, or something that is not a model name, asks for no model.
        for unset in ["default", "--dangerously-skip-permissions", "a b", "x;y"] {
            let list = attempts("", unset, "d");
            assert_eq!(list.len(), 1, "{unset}");
            assert!(!list[0].1.contains(&"--model".to_string()), "{unset}");
        }
        let (program, args, stdin) = list.into_iter().nth(1).unwrap();
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
        let list = attempts(
            "claude || codex exec -s workspace-write ||",
            "default",
            "2026-11-11",
        );
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
            "default",
            "2026-11-11",
        );
        first_success(&list, &store).unwrap();
        assert_eq!(
            fs::read_to_string(store.join("notes/keep.md")).unwrap(),
            "before\n"
        );
        assert!(store.join("notes/dreamt.md").exists());

        let failed = first_success(&attempts(&bad, "default", "2026-11-11"), &store).unwrap_err();
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
    fn todays_dream_is_due_once() {
        // 03:00 today is t=1000.
        assert!(!due(999, 1000, None));
        assert!(due(1000, 1000, None));
        assert!(due(5000, 1000, Some(400)));
        // It ran at 03:00, or by hand afterwards: not again today.
        assert!(!due(5000, 1000, Some(1000)));
        assert!(!due(5000, 1000, Some(3000)));
        assert_eq!(time_of_day("03:00").unwrap(), (3, 0));
        assert_eq!(time_of_day(" 23:59").unwrap(), (23, 59));
        for bad in ["3", "24:00", "03:60", "a:b", ""] {
            assert!(time_of_day(bad).is_err(), "{bad}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_recorded_dream_leaves_its_status_and_allows_one_at_a_time() {
        let t = Temp::new();
        let store = t.0.join("store");
        init::run(&store).unwrap();
        assert!(status(&store).is_none());
        // Nothing to do still counts as a dream that ran and worked.
        run_recorded(&store, "hand").unwrap();
        let s = status(&store).unwrap();
        assert!(s.ok && s.finished >= s.started && s.by == "hand");
        assert!(s.message.starts_with("nothing to dream"));
        assert!(describe(&store).contains("finished 0 min ago, started by hand"));

        // While one runs, another is refused; a stale lock is not in the way.
        fs::write(lock_file(&store), crate::util::unix_now().to_string()).unwrap();
        assert!(run_recorded(&store, "timer")
            .unwrap_err()
            .contains("already running"));
        assert!(start_in_background(&store, "grok").is_err());
        // A fresh lock is never removed by someone who was refused.
        assert!(lock_file(&store).exists());
        fs::write(lock_file(&store), "1000").unwrap();
        run_recorded(&store, "timer").unwrap();
        assert_eq!(status(&store).unwrap().by, "timer");
        assert!(!lock_file(&store).exists());

        // An agent may ask, but not again right away; by hand always works.
        start_in_background(&store, "grok").unwrap();
        for _ in 0..200 {
            if status(&store).is_some_and(|s| s.by == "grok" && s.finished > 0) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let again = start_in_background(&store, "chatgpt").unwrap_err();
        assert!(again.contains("already asked"), "{again}");
        run_recorded(&store, "hand").unwrap();
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
    fn a_backup_is_one_file_that_clones_back_into_the_memory() {
        let t = Temp::new();
        let store = t.0.join("store");
        init::run(&store).unwrap();
        git(&store, &["config", "user.email", "test@example.invalid"]).unwrap();
        git(&store, &["config", "user.name", "test"]).unwrap();
        fs::write(store.join("notes/keep.md"), "keep me\n").unwrap();
        fs::create_dir_all(store.join(".lucid")).unwrap();
        fs::write(store.join(".lucid/tokens"), "secret\n").unwrap();
        git(&store, &["add", "-A"]).unwrap();
        git(&store, &["commit", "-q", "-m", "intake"]).unwrap();

        let folder = t.0.join("drive/backups");
        let file = backup(&store, &folder).unwrap();
        assert!(file.ends_with(BACKUP_FILE));
        // A second backup replaces the first and leaves nothing half-written.
        backup(&store, &folder).unwrap();
        assert_eq!(count_files(&folder), 1);

        let restored = t.0.join("restored");
        git(&t.0, &["clone", "-q", &file, &restored.to_string_lossy()]).unwrap();
        assert_eq!(
            fs::read_to_string(restored.join("notes/keep.md")).unwrap(),
            "keep me\n"
        );
        assert!(!restored.join(".lucid").exists());
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
