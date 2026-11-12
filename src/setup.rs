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
        write_settings(&plan.settings, &merged)?;
    }
    register_mcp(plan);
    println!(
        "Done. Start a new Claude Code session; it loads the shared memory and listens for \
         messages. `lucid-dream who` lists the sessions."
    );
    Ok(())
}

fn write_settings(file: &Path, merged: &Value) -> Result<(), String> {
    let io = |e: std::io::Error| format!("{}: {e}", file.display());
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir).map_err(io)?;
    }
    if file.exists() {
        let backup = file.with_extension("json.bak-before-lucid-setup");
        if !backup.exists() {
            fs::copy(file, &backup).map_err(io)?;
            owner_only(&backup);
            println!("  backup: {}", backup.display());
        }
    }
    let mut text = serde_json::to_string_pretty(merged).map_err(|e| e.to_string())?;
    text.push('\n');
    // Write beside the file and rename, so a crash cannot leave half a file.
    // The file holds this device's token, so only its owner may read it.
    let temp = file.with_extension("json.lucid-tmp");
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut out = options.open(&temp).map_err(io)?;
    std::io::Write::write_all(&mut out, text.as_bytes()).map_err(io)?;
    out.sync_all().map_err(io)?;
    drop(out);
    // A leftover temp file from an earlier run keeps the mode it had.
    owner_only(&temp);
    fs::rename(&temp, file).map_err(io)
}

/// Let only the file's owner read it. On Windows the user profile's own
/// access rules already do this.
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
