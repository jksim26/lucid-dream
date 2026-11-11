//! MCP over JSON-RPC 2.0: one message in, at most one message out.
//! The same handler serves both the HTTP and the stdio transport.

use serde_json::{json, Value};

use crate::store::{Store, StoreError, INDEX};

const SUPPORTED_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];
const DEFAULT_SEARCH_LIMIT: u64 = 50;
const MAX_SEARCH_LIMIT: u64 = 200;

const INSTRUCTIONS: &str = "Shared memory for every agent and device. Call `index` at the \
start of a session to load MEMORY.md, then `read` the notes or wiki pages it points to. \
Use `search` to find something the index does not mention. This server is read-only.";

/// Handle one JSON-RPC message. Notifications and client responses yield `None`.
pub fn handle(store: &Store, msg: &Value) -> Option<Value> {
    let id = msg.get("id").cloned();
    let Some(method) = msg.get("method").and_then(Value::as_str) else {
        if msg.get("result").is_some() || msg.get("error").is_some() {
            return None;
        }
        return Some(error(id.unwrap_or(Value::Null), -32600, "invalid request"));
    };
    let id = id?;
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    let result = match method {
        "initialize" => Ok(initialize(&params)),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tools() })),
        "tools/call" => call_tool(store, &params),
        _ => Err((-32601, format!("method not found: {method}"))),
    };
    Some(match result {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err((code, message)) => error(id, code, &message),
    })
}

pub fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn initialize(params: &Value) -> Value {
    let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or("");
    let version = if SUPPORTED_VERSIONS.contains(&asked) { asked } else { SUPPORTED_VERSIONS[0] };
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": {} },
        "serverInfo": { "name": "lucid-dream", "version": env!("CARGO_PKG_VERSION") },
        "instructions": INSTRUCTIONS,
    })
}

fn tools() -> Value {
    let read_only = json!({ "readOnlyHint": true });
    json!([
        {
            "name": "index",
            "description": "Return MEMORY.md, the index of shared memory. Load it at session start.",
            "inputSchema": { "type": "object", "properties": {} },
            "annotations": read_only,
        },
        {
            "name": "list",
            "description": "List memory files with their sizes. `dir` narrows it to a folder such as notes or wiki.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "dir": { "type": "string", "description": "Folder relative to the memory root. Omit for everything." }
                }
            },
            "annotations": read_only,
        },
        {
            "name": "read",
            "description": "Read one memory file by its path relative to the memory root, e.g. notes/rust.md.",
            "inputSchema": {
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"]
            },
            "annotations": read_only,
        },
        {
            "name": "search",
            "description": "Case-insensitive text search across memory files. Returns path:line: text for each match.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "dir": { "type": "string", "description": "Folder to search. Omit for everything." },
                    "limit": { "type": "integer", "description": "Most matches to return (default 50, at most 200)." }
                },
                "required": ["query"]
            },
            "annotations": read_only,
        }
    ])
}

fn call_tool(store: &Store, params: &Value) -> Result<Value, (i64, String)> {
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let args = params.get("arguments").cloned().unwrap_or(Value::Null);
    let text = |key: &str| args.get(key).and_then(Value::as_str).unwrap_or("").to_string();
    let required = |key: &str| {
        args.get(key)
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or((-32602, format!("missing string argument: {key}")))
    };

    let outcome: Result<String, StoreError> = match name {
        "index" => store.read(INDEX),
        "list" => store.list(&text("dir")).map(|entries| {
            if entries.is_empty() {
                return "no files".to_string();
            }
            entries
                .iter()
                .map(|e| format!("{}\t{} bytes", e.path, e.bytes))
                .collect::<Vec<_>>()
                .join("\n")
        }),
        "read" => store.read(&required("path")?),
        "search" => {
            let limit = args
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(DEFAULT_SEARCH_LIMIT)
                .clamp(1, MAX_SEARCH_LIMIT) as usize;
            store.search(&required("query")?, &text("dir"), limit).map(|hits| {
                if hits.is_empty() {
                    return "no matches".to_string();
                }
                hits.iter()
                    .map(|h| format!("{}:{}: {}", h.path, h.line, h.text))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
        }
        _ => return Err((-32602, format!("unknown tool: {name}"))),
    };

    // A tool that fails reports it in the result so the model can read why.
    Ok(match outcome {
        Ok(text) => json!({ "content": [{ "type": "text", "text": text }], "isError": false }),
        Err(e) => json!({ "content": [{ "type": "text", "text": e.to_string() }], "isError": true }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::tests::Temp;

    fn call(store: &Store, name: &str, args: Value) -> Value {
        let msg = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                          "params": { "name": name, "arguments": args } });
        handle(store, &msg).unwrap()["result"].clone()
    }

    #[test]
    fn initialize_negotiates_the_version() {
        let t = Temp::new();
        let s = t.store();
        let ask = |v: &str| {
            let msg = json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize",
                              "params": { "protocolVersion": v } });
            handle(&s, &msg).unwrap()["result"]["protocolVersion"].clone()
        };
        assert_eq!(ask("2025-03-26"), "2025-03-26");
        assert_eq!(ask("1999-01-01"), SUPPORTED_VERSIONS[0]);
    }

    #[test]
    fn notifications_get_no_reply() {
        let t = Temp::new();
        let msg = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
        assert!(handle(&t.store(), &msg).is_none());
    }

    #[test]
    fn unknown_method_is_an_error() {
        let t = Temp::new();
        let msg = json!({ "jsonrpc": "2.0", "id": 7, "method": "nope" });
        let reply = handle(&t.store(), &msg).unwrap();
        assert_eq!(reply["error"]["code"], -32601);
        assert_eq!(reply["id"], 7);
    }

    #[test]
    fn lists_four_tools() {
        let t = Temp::new();
        let msg = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" });
        let reply = handle(&t.store(), &msg).unwrap();
        assert_eq!(reply["result"]["tools"].as_array().unwrap().len(), 4);
    }

    #[test]
    fn tools_read_the_store() {
        let t = Temp::new();
        let s = t.store();
        let index = call(&s, "index", json!({}));
        assert!(index["content"][0]["text"].as_str().unwrap().starts_with("# MEMORY"));
        let hits = call(&s, "search", json!({ "query": "said_by" }));
        assert_eq!(hits["content"][0]["text"], "notes/rust.md:2: said_by: user");
        let list = call(&s, "list", json!({ "dir": "notes" }));
        assert!(list["content"][0]["text"].as_str().unwrap().starts_with("notes/rust.md\t"));
    }

    #[test]
    fn a_refused_path_is_a_tool_error() {
        let t = Temp::new();
        let reply = call(&t.store(), "read", json!({ "path": "../secret" }));
        assert_eq!(reply["isError"], true);
    }
}
