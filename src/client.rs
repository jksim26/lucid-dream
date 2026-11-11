//! A minimal MCP client for the hooks and the importer: call one tool on a
//! lucid-dream server over HTTP and return its text.

use std::time::Duration;

use serde_json::{json, Value};

pub struct Client {
    url: String,
    token: Option<String>,
    timeout: Duration,
}

impl Client {
    /// The server comes from `--url` or LUCID_URL; the token only from
    /// LUCID_TOKEN, so it never shows up in a process list.
    pub fn from_env(url: Option<String>, timeout: Duration) -> Result<Client, String> {
        let env = |key: &str| std::env::var(key).ok().filter(|v| !v.is_empty());
        let url = url
            .or_else(|| env("LUCID_URL"))
            .ok_or("no server; pass --url or set LUCID_URL (e.g. http://my-pc:7878/mcp)")?;
        Ok(Client {
            url,
            token: env("LUCID_TOKEN"),
            timeout,
        })
    }

    pub fn call(&self, tool: &str, arguments: Value) -> Result<String, String> {
        let body = json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": tool, "arguments": arguments }
        });
        let agent = ureq::AgentBuilder::new().timeout(self.timeout).build();
        let mut request = agent
            .post(&self.url)
            .set("Content-Type", "application/json")
            .set("Accept", "application/json, text/event-stream");
        if let Some(token) = &self.token {
            request = request.set("Authorization", &format!("Bearer {token}"));
        }
        let response = request
            .send_string(&body.to_string())
            .map_err(|e| match e {
                ureq::Error::Status(401, _) => {
                    "server refused the token (check LUCID_TOKEN)".to_string()
                }
                ureq::Error::Status(code, _) => format!("server answered {code}"),
                ureq::Error::Transport(t) => format!("cannot reach {}: {t}", self.url),
            })?;
        let text = response.into_string().map_err(|e| e.to_string())?;
        let reply: Value = serde_json::from_str(&text).map_err(|e| format!("bad reply: {e}"))?;
        if let Some(message) = reply["error"]["message"].as_str() {
            return Err(message.to_string());
        }
        let text = reply["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or("")
            .to_string();
        if reply["result"]["isError"] == true {
            Err(text)
        } else {
            Ok(text)
        }
    }
}
