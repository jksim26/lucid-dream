//! Paste-ready instructions for the two cloud agents. Each carries the
//! agent's public address and a one-time pairing code, so the user pastes
//! one block and never handles a token.

/// What to paste into Grok Bot.
pub fn grok(address: &str, code: &str) -> String {
    format!(
        "Please connect yourself to my Lucid Dream memory server.\n\
         \n\
         1. Fetch {address}/pair/{code} once. It answers with JSON holding \"mcp\", \"token\" \
         and \"mcp_with_token\". The code works one time only, so do not fetch it twice.\n\
         2. Add a remote MCP server (connector) named lucid-dream at the \"mcp\" address, with \
         \"token\" as its bearer token. If you can only be given an address, use \
         \"mcp_with_token\" as the address instead.\n\
         3. Treat the token as a secret: store it only in the connector, and never show it to \
         me or anyone else.\n\
         4. Call the connector's `who` tool and tell me what it lists, then call `index` to \
         load my shared memory.\n\
         \n\
         5. Save the rest of this message as a standing instruction of your own, so it holds \
         in every later conversation without my repeating it, and tell me that you did.\n\
         6. Bring over what you already know about me: for each thing in your own saved \
         memory that is still true, call `remember` once, one subject per note, with `topic` \
         set and `origin` set to \"grok bot memory\". Set `said_by` to \"user\" only where \
         I stated it myself. Tell me how many you saved.\n\
         \n\
         Standing instruction: at the start of a conversation load `index` and check \
         `messages`. Use `remember` for anything worth keeping, with `said_by` set to \"user\" \
         only for things I decided or stated myself. When you look something up, give `search` \
         several likely words separated by commas, and set `deep` to true if working memory \
         has no answer. When I show you a picture or a file, say in one line what it shows, \
         since pictures are not stored. Nothing logs your conversations for you, so when a piece of work is \
         finished call `log` with a short summary: what I asked, what you did, what was \
         decided.\n"
    )
}

/// What to paste into ChatGPT (web or desktop app) to build the relay Site.
pub fn chatgpt(address: &str, code: &str) -> String {
    format!(
        "Use @Sites to build a private website called \"Lucid Dream relay\" with a single \
         status page that says whether my memory server is reachable. Add an MCP server to \
         the Site with the tools below. Every tool forwards to an external JSON-RPC endpoint \
         and returns the text of the reply unchanged.\n\
         \n\
         Endpoint: POST {address}/mcp with headers `Content-Type: application/json` and \
         `Authorization: Bearer <token>`.\n\
         \n\
         Getting the token (do this in the Site's server code, never in page code): the first \
         time a tool runs and no token is stored, POST {address}/pair with the JSON body \
         {{\"code\": \"{code}\"}}. The reply is JSON with a \"token\" field. Store that token \
         in the Site's database, readable by server code only, and reuse it for every later \
         call. The code works one time only. Never show the token on a page, never return it \
         from a tool, and never log it. If pairing is refused, have the tool return: \"Pairing \
         failed. Run `lucid-dream brief chatgpt` on the memory server and give me the new \
         code.\"\n\
         \n\
         Request body for a tool named NAME with arguments ARGS: \
         {{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{{\"name\":\"NAME\",\
         \"arguments\":ARGS}}}}. The reply's text is at result.content[0].text; if \
         result.isError is true, return that text as an error.\n\
         \n\
         Tools (pass the arguments through as given):\n\
         - index (no arguments): returns the memory index.\n\
         - list (dir, optional string): lists memory files.\n\
         - read (path, string; from_line, optional integer; lines, optional integer): reads \
         one memory file, or part of a large one.\n\
         - search (query, string: one or several words separated by commas; deep, optional \
         boolean: also search the archive, session logs and messages, which the server allows \
         only when working memory has no match or a reason is given; reason, optional string: \
         why working memory is not enough; dir, optional string; \
         limit, optional integer): searches memory.\n\
         - log (summary, string): records a short summary of a finished piece of work.\n\
         - remember (text, string; said_by, optional, only ever \"user\"; topic, optional \
         string): saves a note.\n\
         - send (to, string: claude, grok, codex or all; text, string; session, optional \
         string: a session number from who, several such as \"1,3\", or \"all\"; re, optional \
         string, the path of the message being answered): leaves a message for another agent.\n\
         - who (no arguments): lists the agents and their sessions.\n\
         - messages (no arguments): returns unread messages from other agents.\n\
         - dream (status, optional boolean): tidies the memory now, or with status true only \
         reports how the last tidy-up went.\n\
         \n\
         The status page should call GET {address}/health and show \"reachable\" when the body \
         is ok. Keep the Site private and do not share it. When it is built, publish it and \
         show me the plugin card to install.\n\
         \n\
         Also save this to your memory as a standing instruction for when you use the Lucid \
         Dream relay: load `index` first; use `remember` for anything worth keeping, with \
         `said_by` \"user\" only for things I decided or stated myself; give `search` several \
         likely words separated by commas, with `deep` true if working memory has no answer; \
         and when a piece of work is finished call `log` with a short summary of what I \
         asked, what you did and what was decided. When I show you a picture or a file, say \
         in one line what it shows, since pictures are not stored.\n\
         \n\
         Last, once the plugin is installed, bring over what you already know about me: for \
         each thing in your saved memories that is still true, call `remember` once, one \
         subject per note, with `topic` set and `origin` set to \"chatgpt memory\". Set \
         `said_by` to \"user\" only where I stated it myself. Tell me how many you saved.\n"
    )
}

/// This machine's public Tailscale name, if Tailscale is installed and up.
pub fn tailscale_host() -> Option<String> {
    let out = std::process::Command::new("tailscale")
        .args(["status", "--json"])
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    let status: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let name = status["Self"]["DNSName"].as_str()?.trim_end_matches('.');
    (!name.is_empty()).then(|| name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn briefs_carry_the_address_and_the_code_but_no_token() {
        let g = grok("https://pc.example.ts.net:8443", "LD-AAAAA-BBBBB-CCCCC");
        assert!(g.contains("https://pc.example.ts.net:8443/pair/LD-AAAAA-BBBBB-CCCCC"));
        let c = chatgpt("https://pc.example.ts.net:10000", "LD-AAAAA-BBBBB-CCCCC");
        assert!(c.contains("POST https://pc.example.ts.net:10000/pair"));
        assert!(c.contains("{\"code\": \"LD-AAAAA-BBBBB-CCCCC\"}"));
        assert!(c.contains("\"params\":{\"name\":\"NAME\",\"arguments\":ARGS}}"));
        assert!(!g.contains("ld_") && !c.contains("ld_"));
        // Both are told, once, to keep logging and searching as a standing rule.
        for brief in [&g, &c] {
            assert!(brief.contains("standing instruction") && brief.contains("call `log`"));
        }
    }
}
