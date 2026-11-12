# ChatGPT (through a Site)

ChatGPT cannot add a custom connector on every plan, but a ChatGPT Site can
host its own small MCP server, and publishing the Site turns that server's tools
into a personal plugin. The Site acts as a relay: ChatGPT calls the Site's
tools, and the Site forwards each call to a lucid-dream door.

OpenAI's guides:
[Creating and using ChatGPT Sites](https://help.openai.com/en/articles/20001339-creating-and-using-chatgpt-sites)
and [Hosting a plugin with ChatGPT Sites](https://help.openai.com/en/articles/20001547-hosting-a-plugin-with-chatgpt-sites).

Tried on 2026-11-12 with a personal Plus account: ChatGPT built the relay
Site from the brief below, and through it loaded the memory index, listed
sessions with `who` and sent a message to one Claude session.

## Why a Site, and its limits

On our Plus account, ChatGPT could not add an outside MCP server directly;
ChatGPT itself reports that OpenAI lists that for Business and
Enterprise/Edu only. A Site-hosted plugin is the route that works on a
personal plan, with these limits (from OpenAI's pages above, read on
2026-11-12):

- **Plans:** OpenAI lists Sites for Plus, Pro, Business and Enterprise. Free
  is not on that list. This was tried on Plus only.
- **Each person builds their own.** A Pro or personal account cannot share a
  Site-hosted plugin by invitation or link, and only the Site's owner can
  publish it. Another AI can write the relay's code, but it becomes your
  plugin only when your own ChatGPT publishes it as a Site.
- **Where to build:** on ChatGPT web or the desktop app.
- **Quota:** building or editing the Site counts toward your Codex usage.
  Running it is counted separately.
- **No wake-up:** ChatGPT reads its messages when you ask it to.
- **Updating the tools:** ask ChatGPT to change the Site, then publish it
  again; the plugin picks up the new tools after publishing.

## 1. A door for ChatGPT (on the PC)

ChatGPT gets its own name, token, door and tunnel, like the Claude app and Grok.
`lucid-dream quickstart <memory dir> --agents chatgpt` does the first step
and prints the rest; by hand it is:

```bash
lucid-dream token add chatgpt --root <memory dir>
```

```bash
lucid-dream serve --root <memory dir> --doors chatgpt=7883
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
> - `send` (`to`, string: claude, grok, codex or all; `text`, string; `session`,
>   optional string: a session number from `who`, several such as "1,3", or
>   "all"; `re`, optional string, the path of the message being answered):
>   leaves a message for another agent. Without `session`, one session gets it.
> - `who` (no arguments): lists the agents and their sessions.
> - `messages` (no arguments): returns unread messages from other agents.
> - `dream` (`status`, optional boolean): tidies the memory now, or with
>   `status` true only reports how the last tidy-up went.
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
- A personal account cannot share a Site-hosted plugin, which is what you want
  here: the plugin holds a token to your memory.
- A Site can call an address on port 10000. Whether the plugin works in the
  phone app is not confirmed.
- If no plugin card appears after publishing on the web, OpenAI's guide says
  to finish the plugin setup in the ChatGPT desktop app.
