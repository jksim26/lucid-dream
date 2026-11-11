//! Device hooks for Claude Code.
//!
//! `session-start` prints the memory index, which Claude Code adds to the
//! session's context. `stop` sends a summary of the session to `logs/`.
//! Both must never get in the way: the caller ignores their errors, so a
//! sleeping PC or a missing network only means no memory this time.

use std::fs;
use std::process::Command;

use serde_json::{json, Value};

use crate::client::Client;

const MAX_PROMPTS: usize = 20;
const MAX_PROMPT_CHARS: usize = 500;
const MAX_REPLY_CHARS: usize = 1500;

pub fn session_start(client: &Client) -> Result<(), String> {
    let index = client.call("index", json!({}))?;
    println!(
        "# Shared memory (lucid-dream)\n\n{}\n\nThis index comes from the lucid-dream MCP \
         server. Use its `read` and `search` tools for the pages it points to, and `remember` \
         for anything a future session should know.",
        index.trim()
    );
    // Messages other agents left for this one. Many sessions share one agent
    // name, so this only peeks: the message stays unread until a session that
    // has actually handled it calls `messages`.
    if let Ok(messages) = client.call("messages", json!({ "peek": true })) {
        if messages != "no new messages" {
            println!(
                "\n# Unread messages from your other agents\n\n{}\n\nTell the user about these \
                 if they are relevant or if they ask. Once they have been dealt with, call the \
                 lucid-dream `messages` tool to mark them read; until then every new session is \
                 shown them again.",
                messages.trim()
            );
        }
    }
    Ok(())
}

/// `input` is the JSON Claude Code passes a Stop hook on stdin.
pub fn stop(client: &Client, input: &str) -> Result<(), String> {
    let input: Value = serde_json::from_str(input).map_err(|e| format!("bad hook input: {e}"))?;
    let transcript = input["transcript_path"]
        .as_str()
        .and_then(|path| fs::read_to_string(path).ok())
        .unwrap_or_default();
    let (prompts, transcript_reply) = read_transcript(&transcript);
    if prompts.is_empty() {
        return Ok(());
    }
    let reply = input["last_assistant_message"]
        .as_str()
        .map(str::to_string)
        .or(transcript_reply)
        .unwrap_or_default();
    client.call(
        "log",
        json!({
            "summary": summarize(&prompts, &reply),
            "session": input["session_id"].as_str().unwrap_or(""),
            "project": input["cwd"].as_str().unwrap_or(""),
            "device": hostname(),
        }),
    )?;
    Ok(())
}

/// The user's prompts, and the last thing the assistant said, from a Claude
/// Code transcript (one JSON object per line).
fn read_transcript(jsonl: &str) -> (Vec<String>, Option<String>) {
    let mut prompts = Vec::new();
    let mut reply = None;
    for line in jsonl.lines() {
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let text = message_text(&entry["message"]["content"]);
        if text.is_empty() {
            continue;
        }
        match entry["type"].as_str() {
            // Text starting with a tag is harness plumbing, not something the user typed.
            Some("user") if entry["isMeta"] != true && !text.starts_with('<') => prompts.push(text),
            Some("assistant") => reply = Some(text),
            _ => {}
        }
    }
    (prompts, reply)
}

/// Message content is either a string or a list of blocks; keep only the text.
fn message_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.trim().to_string(),
        Value::Array(blocks) => blocks
            .iter()
            .filter(|b| b["type"] == "text")
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string(),
        _ => String::new(),
    }
}

fn summarize(prompts: &[String], reply: &str) -> String {
    let skipped = prompts.len().saturating_sub(MAX_PROMPTS);
    let mut out = String::from("## Asked\n");
    if skipped > 0 {
        out.push_str(&format!("- ({skipped} earlier prompts left out)\n"));
    }
    for prompt in &prompts[skipped..] {
        let one_line = prompt.split_whitespace().collect::<Vec<_>>().join(" ");
        out.push_str(&format!("- {}\n", clip(&one_line, MAX_PROMPT_CHARS)));
    }
    if !reply.trim().is_empty() {
        out.push_str(&format!(
            "\n## Last reply\n{}\n",
            clip(reply.trim(), MAX_REPLY_CHARS)
        ));
    }
    out
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        format!("{}…", text.chars().take(max).collect::<String>())
    }
}

fn hostname() -> String {
    Command::new("hostname")
        .output()
        .ok()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRANSCRIPT: &str = r#"{"type":"user","message":{"role":"user","content":"Fix the   login bug"}}
{"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"hm"},{"type":"text","text":"Looking."},{"type":"tool_use","id":"1","name":"Read","input":{}}]}}
{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"1","content":"file"}]}}
{"type":"user","isMeta":true,"message":{"role":"user","content":"Caveat: generated"}}
{"type":"user","message":{"role":"user","content":"<command-name>/clear</command-name>"}}
not json
{"type":"user","message":{"role":"user","content":[{"type":"text","text":"Use Rust for it"}]}}
{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Done: fixed in auth.rs."}]}}
"#;

    #[test]
    fn keeps_only_what_the_user_typed_and_the_last_reply() {
        let (prompts, reply) = read_transcript(TRANSCRIPT);
        assert_eq!(prompts, ["Fix the   login bug", "Use Rust for it"]);
        assert_eq!(reply.as_deref(), Some("Done: fixed in auth.rs."));
    }

    #[test]
    fn summary_is_bounded() {
        let prompts: Vec<String> = (0..25)
            .map(|i| format!("p{i} {}", "x".repeat(600)))
            .collect();
        let summary = summarize(&prompts, &"y".repeat(5000));
        assert!(summary.contains("- (5 earlier prompts left out)\n- p5 "));
        assert!(!summary.contains("- p4 "));
        assert!(
            summary.chars().count() < MAX_PROMPTS * (MAX_PROMPT_CHARS + 10) + MAX_REPLY_CHARS + 200
        );
        assert_eq!(summarize(&["a  b".into()], ""), "## Asked\n- a b\n");
    }
}
