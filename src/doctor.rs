//! `lucid-dream doctor`: check this machine's connection to the memory, and
//! the memory store itself when run where it lives. It changes nothing.

use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use serde_json::Value;

use crate::client::Client;
use crate::upgrade;
use crate::util::git;

const TIMEOUT: Duration = Duration::from_secs(5);
/// The hooks a connected Claude Code has: event and `hook` subcommand.
const HOOKS: [(&str, &str); 5] = [
    ("SessionStart", "session-start"),
    ("SessionStart", "wait"),
    ("Stop", "stop"),
    ("Stop", "wait"),
    ("SessionEnd", "session-end"),
];
/// A store whose last dream is older than this is probably not scheduled.
const DREAM_DUE_DAYS: i64 = 2;

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Level {
    Ok,
    Warn,
    Fail,
}

pub struct Check {
    pub level: Level,
    pub text: String,
}

fn check(level: Level, text: impl Into<String>) -> Check {
    Check {
        level,
        text: text.into(),
    }
}

pub struct Options<'a> {
    pub settings: &'a Path,
    pub url: Option<String>,
    pub root: Option<&'a Path>,
    /// Skip the checks that need the internet or the `claude` command.
    pub offline: bool,
}

/// Run every check and print the report. False when something is broken.
pub fn run(options: &Options) -> bool {
    let mut checks = vec![this_binary()];
    if !options.offline {
        checks.push(release());
    }
    if let Some(root) = options.root {
        checks.push(heading("Memory store"));
        checks.extend(store(root));
    }
    checks.push(heading("Claude Code on this machine"));
    let connected = options.settings.exists();
    let settings = if connected {
        read_settings(options.settings, &mut checks)
    } else {
        checks.push(check(
            Level::Ok,
            "not connected here (nothing to do unless you use Claude Code; then run \
             `lucid-dream setup`)",
        ));
        None
    };
    let from_settings = |key: &str| {
        settings
            .as_ref()
            .and_then(|s| s["env"][key].as_str())
            .filter(|v| !v.is_empty())
            .map(str::to_string)
    };
    let env = |key: &str| std::env::var(key).ok().filter(|v| !v.is_empty());
    let url = options
        .url
        .clone()
        .or_else(|| env("LUCID_URL"))
        .or_else(|| from_settings("LUCID_URL"))
        // On the machine that holds the memory, the server is normally here.
        .or_else(|| {
            options
                .root
                .map(|_| "http://127.0.0.1:7878/mcp".to_string())
        });
    let token = env("LUCID_TOKEN").or_else(|| from_settings("LUCID_TOKEN"));
    // The plugin brings the same hooks; where both are present the plugin's
    // stand aside, so say which route is in use rather than call it broken.
    let plugin = plugin_installed(options.settings);
    let by_setup = settings.as_ref().is_some_and(crate::setup::has_hooks);
    match (&settings, plugin, by_setup) {
        (_, true, false) => checks.push(check(
            Level::Ok,
            "the hooks and memory tools come from the lucid-dream plugin (its address and \
             token are the plugin's options, which this check cannot read)",
        )),
        (Some(settings), true, true) => {
            checks.extend(hooks(settings, &|exe| version_of(exe)));
            checks.push(check(
                Level::Warn,
                "both `setup` and the lucid-dream plugin are installed; the plugin's hooks \
                 stand aside, but its memory tools are listed twice. Keep one: run \
                 `lucid-dream uninstall`, or remove the plugin",
            ));
        }
        (Some(settings), false, _) => checks.extend(hooks(settings, &|exe| version_of(exe))),
        (None, _, _) => {}
    }
    if connected {
        if !options.offline {
            checks.push(mcp_registered());
        }
        checks.push(waiters());
    }
    checks.push(heading("Server"));
    checks.extend(server(url, token));

    let mut broken = 0;
    for c in &checks {
        match c.level {
            _ if c.text.starts_with("## ") => println!("\n{}", &c.text[3..]),
            Level::Ok => println!("  ok    {}", c.text),
            Level::Warn => println!("  warn  {}", c.text),
            Level::Fail => {
                broken += 1;
                println!("  FAIL  {}", c.text)
            }
        }
    }
    let warnings = checks.iter().filter(|c| c.level == Level::Warn).count();
    println!(
        "\n{}",
        match (broken, warnings) {
            (0, 0) => "Everything checks out.".to_string(),
            (0, w) => format!("Working, with {w} thing(s) to look at."),
            (b, _) => format!("{b} thing(s) are broken."),
        }
    );
    broken == 0
}

fn heading(title: &str) -> Check {
    check(Level::Ok, format!("## {title}"))
}

fn this_binary() -> Check {
    let exe = std::env::current_exe()
        .map(|e| e.display().to_string())
        .unwrap_or_else(|_| "?".into());
    check(
        Level::Ok,
        format!("lucid-dream {} at {exe}", upgrade::CURRENT),
    )
}

fn release() -> Check {
    match upgrade::latest() {
        Ok(latest) if !upgrade::newer(&latest.version, upgrade::CURRENT) => {
            check(Level::Ok, "this is the latest release")
        }
        Ok(latest) if !latest.ready => check(
            Level::Ok,
            format!("version {} {}", latest.version, upgrade::NOT_READY),
        ),
        Ok(latest) => check(
            Level::Warn,
            format!(
                "version {} is out; run `lucid-dream upgrade`",
                latest.version
            ),
        ),
        Err(e) => check(
            Level::Warn,
            format!("could not check for a newer release: {e}"),
        ),
    }
}

/// True when Claude Code's list of installed plugins, kept beside the
/// settings file, names the lucid-dream plugin.
fn plugin_installed(settings: &Path) -> bool {
    settings
        .parent()
        .map(|dir| dir.join("plugins").join("installed_plugins.json"))
        .and_then(|file| fs::read_to_string(file).ok())
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|list| {
            let plugins = list["plugins"].as_object()?;
            Some(plugins.keys().any(|name| name.starts_with("lucid-dream@")))
        })
        .unwrap_or(false)
}

fn read_settings(file: &Path, checks: &mut Vec<Check>) -> Option<Value> {
    let text = match fs::read_to_string(file) {
        Ok(text) => text,
        Err(_) => {
            checks.push(check(
                Level::Warn,
                format!(
                    "no settings file at {}; run `lucid-dream setup`",
                    file.display()
                ),
            ));
            return None;
        }
    };
    match serde_json::from_str(&text) {
        Ok(value) => Some(value),
        Err(e) => {
            checks.push(check(
                Level::Fail,
                format!("{} is not valid JSON: {e}", file.display()),
            ));
            None
        }
    }
}

/// The program a hook command starts: everything before ` hook <name>`.
fn program_of(command: &str) -> &str {
    command
        .rsplit_once(" hook ")
        .map_or(command, |(exe, _)| exe)
        .trim()
        .trim_matches('"')
}

/// Which of our hooks are in the settings, and whether each points at a
/// program that exists and is this version. `version_of` runs the program.
pub fn hooks(settings: &Value, version_of: &dyn Fn(&str) -> Option<String>) -> Vec<Check> {
    let mut checks = Vec::new();
    let mut missing = Vec::new();
    let mut programs: Vec<String> = Vec::new();
    for (event, name) in HOOKS {
        let command = settings["hooks"][event]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|group| group["hooks"].as_array().into_iter().flatten())
            .filter_map(|hook| hook["command"].as_str())
            .find(|c| c.contains("lucid-dream") && c.ends_with(&format!(" hook {name}")));
        match command {
            Some(command) => {
                let program = program_of(command).to_string();
                if !programs.contains(&program) {
                    programs.push(program);
                }
            }
            None => missing.push(format!("{event}: {name}")),
        }
    }
    if missing.is_empty() {
        checks.push(check(Level::Ok, "all five hooks are installed"));
    } else if missing.len() == HOOKS.len() {
        checks.push(check(
            Level::Warn,
            "no lucid-dream hooks are installed; run `lucid-dream setup`",
        ));
    } else {
        checks.push(check(
            Level::Warn,
            format!(
                "missing hooks ({}); run `lucid-dream setup` to add them",
                missing.join(", ")
            ),
        ));
    }
    for program in programs {
        checks.push(match version_of(&program) {
            Some(v) if v == upgrade::CURRENT => {
                check(Level::Ok, format!("hooks run {program} ({v})"))
            }
            Some(v) => check(
                Level::Warn,
                format!(
                    "hooks run {program}, which is version {v}, not {}",
                    upgrade::CURRENT
                ),
            ),
            None => check(
                Level::Fail,
                format!("hooks run {program}, which cannot be started"),
            ),
        });
    }
    checks
}

/// The version a lucid-dream binary reports, if it runs.
fn version_of(program: &str) -> Option<String> {
    let out = Command::new(program).arg("--version").output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.trim().strip_prefix("lucid-dream ").map(str::to_string)
}

fn mcp_registered() -> Check {
    match Command::new("claude")
        .args(["mcp", "get", "lucid-dream"])
        .output()
    {
        Ok(out) if out.status.success() => check(Level::Ok, "the memory tools are registered"),
        Ok(_) => check(
            Level::Warn,
            "the memory tools are not registered with `claude`; run `lucid-dream setup`",
        ),
        Err(_) => check(
            Level::Warn,
            "`claude` is not on the PATH, so its MCP registration could not be checked",
        ),
    }
}

/// How many sessions on this machine have a waiter that reported recently.
fn waiters() -> Check {
    let home = ["HOME", "USERPROFILE"]
        .iter()
        .find_map(|key| std::env::var_os(key).filter(|v| !v.is_empty()));
    let Some(home) = home else {
        return check(
            Level::Warn,
            "no home folder, so waiting sessions could not be counted",
        );
    };
    let dir = Path::new(&home).join(".lucid-dream").join("wait");
    let versions: Vec<Option<String>> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "alive"))
        .filter(|e| {
            e.metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|at| at.elapsed().ok())
                .is_some_and(|age| age < Duration::from_secs(90))
        })
        // Each file names its waiter as "<pid> <version>"; before 0.5.1, "<pid>".
        .map(|e| {
            fs::read_to_string(e.path())
                .ok()
                .and_then(|text| text.split_whitespace().nth(1).map(str::to_string))
        })
        .collect();
    waiting(&versions)
}

/// The report for the waiters found, each with the version it runs if known.
fn waiting(versions: &[Option<String>]) -> Check {
    let old = versions
        .iter()
        .filter(|v| v.as_deref() != Some(upgrade::CURRENT))
        .count();
    match (versions.len(), old) {
        (0, _) => check(
            Level::Warn,
            "no session on this machine is waiting for messages right now",
        ),
        (n, 0) => check(
            Level::Ok,
            format!("{n} session(s) on this machine are waiting for messages"),
        ),
        (n, old) => check(
            Level::Warn,
            format!(
                "{n} session(s) are waiting for messages, {old} of them with an older version \
                 of lucid-dream; each switches to {} after its next turn",
                upgrade::CURRENT
            ),
        ),
    }
}

fn server(url: Option<String>, token: Option<String>) -> Vec<Check> {
    let Some(url) = url else {
        return vec![check(
            Level::Fail,
            "no server address; pass --url or run `lucid-dream setup`",
        )];
    };
    let Some(token) = token else {
        // No token on this machine to test with: at least see that it is up.
        let health = match url.strip_suffix("/mcp") {
            Some(base) => format!("{base}/health"),
            None => url.clone(),
        };
        let agent = ureq::AgentBuilder::new().timeout(TIMEOUT).build();
        return vec![match agent.get(&health).call() {
            Ok(_) => check(
                Level::Ok,
                format!("{url} answers (no LUCID_TOKEN here, so no token was tested)"),
            ),
            Err(_) => check(
                Level::Fail,
                format!("nothing answers at {url}; start it with `lucid-dream serve`"),
            ),
        }];
    };
    match Client::with(url.clone(), token, TIMEOUT).call("who", serde_json::json!({})) {
        Ok(list) => {
            let me = list
                .lines()
                .find_map(|l| l.split_once(" (you)").map(|(name, _)| name.trim()))
                .unwrap_or("an agent");
            let sessions = list
                .lines()
                .filter(|l| l.trim_start().starts_with('#'))
                .count();
            vec![check(
                Level::Ok,
                format!("{url} answers, and the token is accepted as {me} ({sessions} session(s) listed)"),
            )]
        }
        Err(e) => vec![check(Level::Fail, e)],
    }
}

/// Checks for the machine that holds the memory.
pub fn store(root: &Path) -> Vec<Check> {
    let mut checks = Vec::new();
    if git(root, &["rev-parse", "--git-dir"]).is_err() {
        return vec![check(
            Level::Fail,
            format!(
                "{} is not a memory store (no git repository)",
                root.display()
            ),
        )];
    }
    checks.push(check(Level::Ok, format!("store at {}", root.display())));

    let tokens = crate::tokens::Tokens::load(&crate::tokens::path(root))
        .map(|t| t.entries().count())
        .unwrap_or(0);
    checks.push(match tokens {
        0 => check(Level::Warn, "no tokens issued; no agent can connect"),
        n => check(Level::Ok, format!("{n} token(s) issued")),
    });

    let inbox = fs::read_dir(root.join("inbox"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .count();
    let last = git(root, &["log", "-1", "--grep=^dream: ", "--format=%ct"])
        .ok()
        .and_then(|t| t.trim().parse::<i64>().ok());
    checks.push(match last {
        None if inbox == 0 => check(Level::Ok, "no dream has run yet; the inbox is empty"),
        None => check(
            Level::Warn,
            format!("no dream has run yet; {inbox} note(s) are waiting in the inbox"),
        ),
        Some(at) => {
            let days = (crate::util::unix_now() - at) / 86400;
            let when = match days {
                0 => "today".to_string(),
                1 => "yesterday".to_string(),
                n => format!("{n} days ago"),
            };
            let text = format!("last dream ran {when}; {inbox} note(s) in the inbox");
            if days > DREAM_DUE_DAYS && inbox > 0 {
                check(Level::Warn, format!("{text}; is dream scheduled?"))
            } else {
                check(Level::Ok, text)
            }
        }
    });

    // The record a dream leaves says more than git does: a failed dream
    // makes no commit.
    if let Some(last) = crate::dream::status(root) {
        if last.finished > 0 && !last.ok {
            checks.push(check(
                Level::Warn,
                format!("the last dream failed: {}", last.message),
            ));
        }
    }
    for (agent, label, minutes) in crate::pair::waiting(root) {
        checks.push(check(
            Level::Ok,
            format!("a pairing code for {agent}/{label} is waiting ({minutes} min left)"),
        ));
    }
    let trusted = crate::trust::load(&crate::trust::path(root));
    if !trusted.is_empty() {
        checks.push(check(
            Level::Ok,
            format!("trusted messengers: {}", trusted.join(", ")),
        ));
    }
    let woken: Vec<String> = crate::webhook::load(&crate::webhook::path(root))
        .into_keys()
        .collect();
    if !woken.is_empty() {
        checks.push(check(
            Level::Ok,
            format!("woken by webhook: {}", woken.join(", ")),
        ));
    }
    checks
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn levels(checks: &[Check]) -> Vec<Level> {
        checks.iter().map(|c| c.level).collect()
    }

    #[test]
    fn reports_missing_stale_and_broken_hooks() {
        let current = |_: &str| Some(upgrade::CURRENT.to_string());
        let none = hooks(&json!({}), &current);
        assert_eq!(levels(&none), [Level::Warn]);

        let plan = crate::setup::Plan {
            url: "http://pc:7878/mcp".into(),
            token: "ld_test".into(),
            exe: "/bin/lucid-dream".into(),
            settings: std::path::PathBuf::new(),
        };
        let (mut settings, _) = crate::setup::merge(&json!({}), &plan);
        assert_eq!(levels(&hooks(&settings, &current)), [Level::Ok, Level::Ok]);

        let old = hooks(&settings, &|_| Some("0.1.0".to_string()));
        assert_eq!(levels(&old), [Level::Ok, Level::Warn]);
        assert!(old[1].text.contains("0.1.0"));
        let gone = hooks(&settings, &|_| None);
        assert_eq!(gone[1].level, Level::Fail);

        settings["hooks"]
            .as_object_mut()
            .unwrap()
            .remove("SessionEnd");
        let partial = hooks(&settings, &current);
        assert_eq!(partial[0].level, Level::Warn);
        assert!(partial[0].text.contains("SessionEnd: session-end"));
    }

    #[test]
    fn says_when_a_waiter_runs_an_older_version() {
        let current = Some(upgrade::CURRENT.to_string());
        assert_eq!(waiting(&[]).level, Level::Warn);
        assert_eq!(
            waiting(&[current.clone(), current.clone()]).level,
            Level::Ok
        );
        let mixed = waiting(&[current, None, Some("0.4.0".into())]);
        assert_eq!(mixed.level, Level::Warn);
        assert!(mixed.text.contains("3 session(s)") && mixed.text.contains("2 of them"));
    }

    #[test]
    fn finds_the_program_in_a_hook_command() {
        assert_eq!(program_of("/bin/lucid-dream hook wait"), "/bin/lucid-dream");
        assert_eq!(
            program_of("\"C:/my tools/lucid-dream.exe\" hook stop"),
            "C:/my tools/lucid-dream.exe"
        );
    }

    #[test]
    fn checks_a_store_and_rejects_a_plain_folder() {
        let t = crate::store::tests::Temp::new();
        assert_eq!(levels(&store(&t.0.join("missing"))), [Level::Fail]);

        let root = t.0.join("store");
        crate::init::run(&root).unwrap();
        let fresh = store(&root);
        // A new store: found, no tokens yet, no dream yet with an empty inbox.
        assert_eq!(levels(&fresh), [Level::Ok, Level::Warn, Level::Ok]);

        fs::write(root.join("inbox/a.md"), "note\n").unwrap();
        let waiting = store(&root);
        assert_eq!(waiting[2].level, Level::Warn);
        assert!(waiting[2].text.contains("1 note(s)"));
    }
}
