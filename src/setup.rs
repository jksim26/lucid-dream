//! `lucid-dream setup`: connect Claude Code on this machine to a memory
//! server in one step, instead of editing settings by hand.
//!
//! It adds the server address, this device's token and the four hooks to
//! `~/.claude/settings.json` (keeping a backup and everything already there),
//! and registers the MCP server with the `claude` command. Running it again
//! changes nothing.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Map, Value};

/// The hooks to install: event, `hook` subcommand, timeout in seconds, and
/// whether it runs in the background and wakes the session.
const HOOKS: [(&str, &str, u64, bool); 5] = [
    ("SessionStart", "session-start", 5, false),
    ("SessionStart", "wait", 86400, true),
    ("Stop", "stop", 5, false),
    ("Stop", "wait", 86400, true),
    ("SessionEnd", "session-end", 5, false),
];

pub struct Plan {
    pub url: String,
    pub token: String,
    /// Path of this binary, as the hooks should call it.
    pub exe: String,
    pub settings: PathBuf,
}

/// The settings file with lucid-dream merged in, and what was added.
pub fn merge(settings: &Value, plan: &Plan) -> (Value, Vec<String>) {
    let mut out = settings.clone();
    let mut added = Vec::new();
    if !out.is_object() {
        out = json!({});
    }
    let root = out.as_object_mut().expect("just made an object");

    let env = object(root, "env");
    for (key, value) in [("LUCID_URL", &plan.url), ("LUCID_TOKEN", &plan.token)] {
        if env.get(key).and_then(Value::as_str) != Some(value.as_str()) {
            env.insert(key.to_string(), json!(value));
            added.push(format!("env {key}"));
        }
    }

    let hooks = object(root, "hooks");
    for (event, name, timeout, wakes) in HOOKS {
        let groups = hooks.entry(event).or_insert_with(|| json!([]));
        let Some(groups) = groups.as_array_mut() else {
            continue;
        };
        let ours = |hook: &Value| {
            hook["command"]
                .as_str()
                .is_some_and(|c| c.contains("lucid-dream") && c.ends_with(&format!(" hook {name}")))
        };
        let present = groups
            .iter()
            .any(|g| g["hooks"].as_array().is_some_and(|h| h.iter().any(ours)));
        if present {
            continue;
        }
        let mut hook = json!({
            "type": "command",
            "command": format!("{} hook {name}", plan.exe),
            "timeout": timeout,
        });
        if wakes {
            hook["asyncRewake"] = json!(true);
        }
        // Keep our hooks together, in a group that runs for every session.
        let group = groups.iter_mut().find(|g| {
            g.get("matcher").is_none()
                && g["hooks"].as_array().is_some_and(|h| {
                    h.iter().any(|x| {
                        x["command"]
                            .as_str()
                            .is_some_and(|c| c.contains("lucid-dream"))
                    })
                })
        });
        match group.and_then(|g| g["hooks"].as_array_mut()) {
            Some(list) => list.push(hook),
            None => groups.push(json!({ "hooks": [hook] })),
        }
        added.push(format!("hook {event}: {name}"));
    }
    (out, added)
}

/// True for a hook entry that `setup` (or the README) added.
fn is_ours(hook: &Value) -> bool {
    hook["command"]
        .as_str()
        .is_some_and(|c| c.contains("lucid-dream") && c.contains(" hook "))
}

/// The settings file with everything lucid-dream added taken out again, and
/// what was removed. Everything else is left exactly as it was.
pub fn unmerge(settings: &Value) -> (Value, Vec<String>) {
    let mut out = settings.clone();
    let mut removed = Vec::new();
    let Some(root) = out.as_object_mut() else {
        return (out, removed);
    };
    if let Some(env) = root.get_mut("env").and_then(Value::as_object_mut) {
        for key in ["LUCID_URL", "LUCID_TOKEN"] {
            if env.shift_remove(key).is_some() {
                removed.push(format!("env {key}"));
            }
        }
        if env.is_empty() && !removed.is_empty() {
            root.shift_remove("env");
        }
    }
    if let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) {
        // Events this empties are dropped; ones that were empty before stay.
        let mut emptied = Vec::new();
        for (event, groups) in hooks.iter_mut() {
            let Some(groups) = groups.as_array_mut() else {
                continue;
            };
            let found = removed.len();
            groups.retain_mut(|group| {
                let Some(list) = group["hooks"].as_array_mut() else {
                    return true;
                };
                let before = list.len();
                for hook in list.iter().filter(|h| is_ours(h)) {
                    let command = hook["command"].as_str().unwrap_or("");
                    let name = command.rsplit(" hook ").next().unwrap_or("");
                    removed.push(format!("hook {event}: {name}"));
                }
                list.retain(|h| !is_ours(h));
                // Drop a group only when taking our hooks out is what emptied it.
                !(list.is_empty() && before > 0)
            });
            if groups.is_empty() && removed.len() > found {
                emptied.push(event.clone());
            }
        }
        let any = !emptied.is_empty();
        for event in emptied {
            hooks.shift_remove(&event);
        }
        if hooks.is_empty() && any {
            root.shift_remove("hooks");
        }
    }
    (out, removed)
}

/// Take lucid-dream out of Claude Code on this machine, or with `dry` only
/// say what would go. The memory and the binary are not touched.
pub fn uninstall(settings: &Path, dry: bool) -> Result<(), String> {
    println!("Claude Code settings: {}", settings.display());
    let removed = match fs::read_to_string(settings) {
        Ok(text) => {
            let existing: Value = serde_json::from_str(&text).map_err(|e| {
                format!(
                    "{} is not valid JSON ({e}); fix it first",
                    settings.display()
                )
            })?;
            let (cleaned, removed) = unmerge(&existing);
            if !dry && !removed.is_empty() {
                write_settings(settings, &cleaned, "json.bak-before-lucid-uninstall")?;
            }
            removed
        }
        Err(_) => Vec::new(),
    };
    if removed.is_empty() {
        println!("  nothing of lucid-dream's is in it");
    }
    for line in &removed {
        println!("  - {line}");
    }
    if dry {
        println!("  - MCP server registration `lucid-dream`, if present");
        println!("Nothing was changed (--plan).");
        return Ok(());
    }
    match Command::new("claude")
        .args(["mcp", "remove", "--scope", "user", "lucid-dream"])
        .output()
    {
        Ok(out) if out.status.success() => println!("MCP server: removed"),
        Ok(_) => println!("MCP server: was not registered"),
        Err(_) => println!(
            "MCP server: `claude` is not on the PATH; if it is registered, run:\n  claude mcp \
             remove --scope user lucid-dream"
        ),
    }
    let exe = std::env::current_exe()
        .map(|e| e.display().to_string())
        .unwrap_or_default();
    println!(
        "Done. Sessions already open keep their hooks until they end. The memory store was not \
         touched; to remove the program itself, delete {exe}"
    );
    Ok(())
}

/// The rule that makes Codex load the index and log its sessions. Codex has
/// no session hooks, so its instructions file is where this has to live.
const CODEX_RULE: &str = include_str!("../clients/codex/AGENTS.md");
const CODEX_RULE_MARK: &str = "## Shared memory (lucid-dream)";
const CODEX_SERVER_MARK: &str = "[mcp_servers.lucid-dream]";
pub const CODEX_TOKEN_ENV: &str = "LUCID_TOKEN_CODEX";

/// Where the Codex CLI keeps its files: CODEX_HOME, or `.codex` at home.
pub fn codex_home() -> Option<PathBuf> {
    let env = |key: &str| std::env::var_os(key).filter(|v| !v.is_empty());
    env("CODEX_HOME").map(PathBuf::from).or_else(|| {
        let home = env("HOME").or_else(|| env("USERPROFILE"))?;
        Some(PathBuf::from(home).join(".codex"))
    })
}

/// The server address in Codex's config, as `codex()` wrote it.
pub fn codex_url(home: &Path) -> Option<String> {
    let config = fs::read_to_string(home.join("config.toml")).ok()?;
    let ours = config.split(CODEX_SERVER_MARK).nth(1)?;
    ours.lines()
        .skip(1)
        .take_while(|line| !line.trim_start().starts_with('['))
        .find_map(|line| {
            let (key, value) = line.split_once('=')?;
            (key.trim() == "url").then(|| value.trim().trim_matches('"').to_string())
        })
        .filter(|url| !url.is_empty())
}

/// True when the settings file already runs lucid-dream's hooks, as `setup`
/// installs them. A plugin's copy of the same hooks then stands aside, so
/// nothing is logged or woken twice.
pub fn has_hooks(settings: &Value) -> bool {
    settings["hooks"].as_object().is_some_and(|events| {
        events
            .values()
            .filter_map(Value::as_array)
            .flatten()
            .filter_map(|group| group["hooks"].as_array())
            .flatten()
            .any(is_ours)
    })
}

/// Connect the Codex CLI on this machine: add the server to its config and
/// the rule to its instructions. Both files are only added to, and only once;
/// a copy of each is kept beside it before the first change.
pub fn codex(home: &Path, url: &str, dry: bool) -> Result<(), String> {
    let server = format!(
        "{CODEX_SERVER_MARK}\nurl = \"{}\"\nbearer_token_env_var = \"{CODEX_TOKEN_ENV}\"\n\
         default_tools_approval_mode = \"approve\"\n",
        url.replace(['"', '\\', '\n'], "")
    );
    let additions = [
        (
            "config.toml",
            CODEX_SERVER_MARK,
            server.as_str(),
            "the lucid-dream MCP server",
        ),
        (
            "AGENTS.md",
            CODEX_RULE_MARK,
            CODEX_RULE,
            "the shared-memory rule",
        ),
    ];
    for (name, mark, addition, what) in additions {
        let file = home.join(name);
        let io = |e: std::io::Error| format!("{}: {e}", file.display());
        let before = match fs::read_to_string(&file) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(io(e)),
        };
        if before.contains(mark) {
            println!("{}: already has {what}", file.display());
            continue;
        }
        if dry {
            println!("{}: would add {what}:\n\n{addition}", file.display());
            continue;
        }
        fs::create_dir_all(home).map_err(io)?;
        if !before.is_empty() {
            fs::write(home.join(format!("{name}.before-lucid-dream")), &before).map_err(io)?;
        }
        let gap = match before.as_str() {
            "" => "",
            text if text.ends_with("\n\n") => "",
            text if text.ends_with('\n') => "\n",
            _ => "\n\n",
        };
        fs::write(&file, format!("{before}{gap}{addition}")).map_err(io)?;
        println!("{}: added {what}", file.display());
    }
    Ok(())
}

fn object<'a>(parent: &'a mut Map<String, Value>, key: &str) -> &'a mut Map<String, Value> {
    let slot = parent.entry(key).or_insert_with(|| json!({}));
    if !slot.is_object() {
        *slot = json!({});
    }
    slot.as_object_mut().expect("just made an object")
}

/// Apply the plan, or with `dry` only say what would change.
pub fn run(plan: &Plan, dry: bool) -> Result<(), String> {
    let existing = match fs::read_to_string(&plan.settings) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| {
            format!(
                "{} is not valid JSON ({e}); fix it first",
                plan.settings.display()
            )
        })?,
        Err(_) => json!({}),
    };
    let (merged, added) = merge(&existing, plan);
    println!("Claude Code settings: {}", plan.settings.display());
    if added.is_empty() {
        println!("  already set up, nothing to change");
    }
    for line in &added {
        println!("  + {line}");
    }
    if dry {
        println!("Nothing was changed (--plan).");
        return Ok(());
    }
    if !added.is_empty() {
        write_settings(&plan.settings, &merged, "json.bak-before-lucid-setup")?;
    }
    register_mcp(plan);
    println!(
        "Done. Start a new Claude Code session; it loads the shared memory and listens for \
         messages. `lucid-dream who` lists the sessions."
    );
    Ok(())
}

fn write_settings(file: &Path, merged: &Value, backup: &str) -> Result<(), String> {
    let io = |e: std::io::Error| format!("{}: {e}", file.display());
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir).map_err(io)?;
    }
    // Both files hold this device's token, so only their owner may read them.
    if file.exists() {
        let backup = file.with_extension(backup);
        if backup.exists() {
            owner_only(&backup);
        } else {
            create_private(&backup, &fs::read(file).map_err(io)?).map_err(io)?;
            println!("  backup: {}", backup.display());
        }
    }
    let mut text = serde_json::to_string_pretty(merged).map_err(|e| e.to_string())?;
    text.push('\n');
    // Write beside the file and rename, so a crash cannot leave half a file.
    let temp = file.with_extension("json.lucid-tmp");
    let _ = fs::remove_file(&temp);
    create_private(&temp, text.as_bytes()).map_err(io)?;
    fs::rename(&temp, file).map_err(io)
}

/// Create a file that only its owner can read, and fill it. It fails if the
/// file exists, so nothing is ever written into a file made with a looser
/// mode. On Windows the user profile's own access rules do the restricting.
fn create_private(file: &Path, contents: &[u8]) -> std::io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut out = options.open(file)?;
    std::io::Write::write_all(&mut out, contents)?;
    out.sync_all()
}

/// Tighten a file that already exists.
fn owner_only(file: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(file, fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    let _ = file;
}

/// Register the MCP server so Claude can use the memory tools in a session.
/// `claude mcp add` only takes the header as an argument, so the token is on
/// that one command's line for the moment it runs. If `claude` is not on the
/// PATH, say how to do it by hand.
fn register_mcp(plan: &Plan) {
    let by_hand = format!(
        "claude mcp add --transport http --scope user lucid-dream {} --header \"Authorization: \
         Bearer <token>\"",
        plan.url
    );
    let listed = Command::new("claude")
        .args(["mcp", "get", "lucid-dream"])
        .output();
    match listed {
        Ok(out) if out.status.success() => println!("MCP server: already registered"),
        Ok(_) => {
            let header = format!("Authorization: Bearer {}", plan.token);
            let added = Command::new("claude")
                .args([
                    "mcp",
                    "add",
                    "--transport",
                    "http",
                    "--scope",
                    "user",
                    "lucid-dream",
                ])
                .arg(&plan.url)
                .args(["--header", &header])
                .output();
            match added {
                Ok(out) if out.status.success() => println!("MCP server: registered"),
                _ => println!("MCP server: could not register; run:\n  {by_hand}"),
            }
        }
        Err(_) => println!("MCP server: `claude` is not on the PATH; run:\n  {by_hand}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan() -> Plan {
        Plan {
            url: "http://pc:7878/mcp".into(),
            token: "ld_test".into(),
            exe: "/bin/lucid-dream".into(),
            settings: PathBuf::new(),
        }
    }

    #[test]
    fn adds_everything_to_empty_settings_and_then_nothing() {
        let (merged, added) = merge(&json!({}), &plan());
        assert_eq!(added.len(), 7);
        assert_eq!(merged["env"]["LUCID_URL"], "http://pc:7878/mcp");
        let start = merged["hooks"]["SessionStart"][0]["hooks"]
            .as_array()
            .unwrap();
        assert_eq!(start.len(), 2);
        assert_eq!(start[1]["command"], "/bin/lucid-dream hook wait");
        assert_eq!(start[1]["asyncRewake"], true);
        assert!(start[0].get("asyncRewake").is_none());
        assert_eq!(merge(&merged, &plan()).1, Vec::<String>::new());
    }

    #[test]
    fn codex_gets_the_server_and_the_rule_once_and_keeps_what_it_had() {
        let dir = std::env::temp_dir().join(format!("lucid-codex-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("config.toml"), "model = \"gpt\"").unwrap();
        let read = |name: &str| fs::read_to_string(dir.join(name)).unwrap();

        // A preview changes nothing.
        codex(&dir, "http://pc:7878/mcp", true).unwrap();
        assert_eq!(read("config.toml"), "model = \"gpt\"");
        assert!(!dir.join("AGENTS.md").exists());

        codex(&dir, "http://pc:7878/mcp", false).unwrap();
        let config = read("config.toml");
        assert!(config.starts_with("model = \"gpt\"\n\n[mcp_servers.lucid-dream]\n"));
        assert!(config.contains("url = \"http://pc:7878/mcp\"\n"));
        assert!(config.contains("bearer_token_env_var = \"LUCID_TOKEN_CODEX\""));
        assert_eq!(read("config.toml.before-lucid-dream"), "model = \"gpt\"");
        assert!(read("AGENTS.md").starts_with(CODEX_RULE_MARK));
        assert!(read("AGENTS.md").contains("call `log`"));

        assert_eq!(codex_url(&dir).as_deref(), Some("http://pc:7878/mcp"));

        // Again: nothing is added twice.
        codex(&dir, "http://other:1/mcp", false).unwrap();
        assert_eq!(read("config.toml"), config);
        assert_eq!(read("AGENTS.md").matches(CODEX_RULE_MARK).count(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_plugins_carry_this_version_and_the_same_launcher() {
        let version = env!("CARGO_PKG_VERSION");
        for manifest in [
            include_str!("../plugins/claude-code/.claude-plugin/plugin.json"),
            include_str!("../plugins/codex/.codex-plugin/plugin.json"),
        ] {
            let manifest: Value = serde_json::from_str(manifest).unwrap();
            assert_eq!(manifest["version"], version);
            assert_eq!(manifest["name"], "lucid-dream");
        }
        let market: Value =
            serde_json::from_str(include_str!("../.claude-plugin/marketplace.json")).unwrap();
        assert_eq!(market["plugins"][0]["version"], version);
        assert_eq!(
            include_str!("../plugins/claude-code/scripts/lucid-dream-hook"),
            include_str!("../plugins/codex/scripts/lucid-dream-hook")
        );
        // Every hook the plugins install is one this program has.
        for hooks in [
            include_str!("../plugins/claude-code/hooks/hooks.json"),
            include_str!("../plugins/codex/hooks/hooks.json"),
        ] {
            let hooks: Value = serde_json::from_str(hooks).unwrap();
            for (_, groups) in hooks["hooks"].as_object().unwrap() {
                for hook in groups[0]["hooks"].as_array().unwrap() {
                    let command = hook["command"].as_str().unwrap();
                    let event = command.rsplit(' ').next().unwrap();
                    assert!(
                        HOOKS.iter().any(|(_, name, _, _)| *name == event),
                        "{command}"
                    );
                }
            }
        }
    }

    #[test]
    fn knows_when_settings_already_run_the_hooks() {
        let ours = json!({ "hooks": { "Stop": [{ "hooks": [
            { "type": "command", "command": "/x/lucid-dream hook stop" }
        ]}]}});
        let other = json!({ "hooks": { "Stop": [{ "hooks": [
            { "type": "command", "command": "other-tool hook stop" }
        ]}]}});
        assert!(has_hooks(&ours));
        assert!(!has_hooks(&other) && !has_hooks(&json!({})));
    }

    #[test]
    fn uninstall_takes_out_only_what_setup_added() {
        let before = json!({
            "model": "opus",
            "env": { "OTHER": "1" },
            "hooks": {
                "SessionStart": [
                    { "matcher": "compact", "hooks": [{ "type": "command", "command": "mine" }] }
                ],
                "PreToolUse": [{ "hooks": [] }]
            }
        });
        let (installed, _) = merge(&before, &plan());
        let (cleaned, removed) = unmerge(&installed);
        assert_eq!(cleaned, before);
        assert_eq!(removed.len(), 7);
        assert_eq!(unmerge(&cleaned).1, Vec::<String>::new());

        // With nothing else in the file, the sections we made go too.
        let (installed, _) = merge(&json!({ "model": "opus" }), &plan());
        assert_eq!(unmerge(&installed).0, json!({ "model": "opus" }));
    }

    #[test]
    fn keeps_what_is_there_and_fills_in_what_is_missing() {
        let before = json!({
            "model": "opus",
            "env": { "OTHER": "1", "LUCID_URL": "http://old:7878/mcp" },
            "hooks": {
                "SessionStart": [
                    { "matcher": "compact", "hooks": [{ "type": "command", "command": "mine" }] },
                    { "hooks": [{ "type": "command", "command": "C:/x/lucid-dream.exe hook session-start", "timeout": 5 }] }
                ]
            }
        });
        let (merged, added) = merge(&before, &plan());
        assert_eq!(merged["model"], "opus");
        assert_eq!(merged["env"]["OTHER"], "1");
        assert_eq!(merged["env"]["LUCID_URL"], "http://pc:7878/mcp");
        let start = merged["hooks"]["SessionStart"].as_array().unwrap();
        assert_eq!(start[0]["hooks"][0]["command"], "mine");
        // The existing hook is kept as it was; the missing one joins its group.
        let ours = start[1]["hooks"].as_array().unwrap();
        assert_eq!(
            ours[0]["command"],
            "C:/x/lucid-dream.exe hook session-start"
        );
        assert_eq!(ours[1]["command"], "/bin/lucid-dream hook wait");
        assert!(!added.contains(&"hook SessionStart: session-start".to_string()));
        assert!(added.contains(&"hook SessionEnd: session-end".to_string()));
        // Key order is kept, so the file does not get reshuffled.
        let keys: Vec<&String> = merged.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["model", "env", "hooks"]);
    }
}
