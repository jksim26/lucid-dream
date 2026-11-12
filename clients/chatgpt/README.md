# ChatGPT (through a Site)

ChatGPT cannot add a custom connector on every plan, but a ChatGPT Site can
host its own small MCP server, and publishing the Site turns that server's tools
into a personal plugin. The Site acts as a relay: ChatGPT calls the Site's
tools, and the Site forwards each call to a lucid-dream door.

Sites need ChatGPT Plus, Pro, Business or Enterprise. OpenAI's guides:
[Creating and using ChatGPT Sites](https://help.openai.com/en/articles/20001339-creating-and-using-chatgpt-sites)
and [Hosting a plugin with ChatGPT Sites](https://help.openai.com/en/articles/20001547-hosting-a-plugin-with-chatgpt-sites).

Tried on 2026-11-12 with a personal account: ChatGPT built the relay Site from
the brief below and loaded the memory index through it.

## 1. A door for ChatGPT (on the PC)

ChatGPT gets its own name, token, door and tunnel, like the Claude app and Grok.

```bash
lucid-dream token add chatgpt --root <memory dir>
```

```bash
lucid-dream serve --root <memory dir> --bind 127.0.0.1:7883 --only chatgpt
```

```bash
tailscale funnel --bg --https=10000 7883
```

The door is then at `https://<pc-name>.<tailnet>.ts.net:10000/mcp`. Funnel has
three public ports; this uses the last one.

## 2. Ask ChatGPT to build the relay Site

On ChatGPT web or desktop, paste this, filling in the address:

> Use @Sites to build a private website called "Lucid Dream relay" with a single
> status page that says whether the memory server is reachable. Add an MCP
> server to the Site with the tools below. Every tool forwards to an external
> JSON-RPC endpoint and returns the text of the reply unchanged.
>
> Endpoint: `POST https://<pc-name>.<tailnet>.ts.net:10000/mcp`, with headers
> `Content-Type: application/json` and `Authorization: Bearer <secret>`. Store
> the secret as a Site secret that I will enter myself; never put it in page
> code, never show it on the page, and never log it.
>
> Request body for a tool named NAME with arguments ARGS:
> `{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"NAME","arguments":ARGS}}`.
> The reply's text is at `result.content[0].text`; if `result.isError` is true,
> return that text as an error.
>
> Tools (pass the arguments through as given):
> - `index` (no arguments): returns the memory index.
> - `list` (`dir`, optional string): lists memory files.
> - `read` (`path`, string): reads one memory file.
> - `search` (`query`, string; `limit`, optional integer): searches memory.
> - `remember` (`text`, string; `said_by`, optional, only ever "user"; `topic`,
>   optional string): saves a note.
> - `send` (`to`, string: claude, grok, codex or all; `text`, string): leaves a
>   message for another agent.
> - `messages` (no arguments): returns unread messages from other agents.
>
> The status page should call `GET https://<pc-name>.<tailnet>.ts.net:10000/health`
> and show "reachable" when the body is `ok`. Keep the Site private and do not
> share it.

Review the tools it built, enter the `chatgpt` token as the secret, publish the
Site, then install and connect the plugin card ChatGPT shows.

## 3. Use it

Mention the plugin in a chat: "use Lucid Dream relay to load my memory index".
Add a custom instruction so it does this at the start of a conversation, and so
"remember" also saves through the plugin with `said_by` set to `user`.

## Things to know

- The Site holds the `chatgpt` token. Revoking it on the PC
  (`lucid-dream token remove chatgpt`) cuts ChatGPT off without touching anyone
  else.
- Building or editing the Site counts toward your Codex usage.
- A personal account cannot share a Site-hosted plugin, which is what you want
  here.
- A Site can call an address on port 10000. Whether the plugin works in the
  phone app is not confirmed.
