# Grok

Two ways in, depending on what can reach the server.

## Digest file (works today)

Dream writes `digest/YYYY-MM-DD.md` every night: what was added, changed and
archived, readable on its own. Paste or attach the latest digest (and
`MEMORY.md`) at the start of a Grok conversation. Grok reads memory this way but
writes nothing back; tell another agent what Grok concluded and it can
`remember` it as your statement.

## Direct MCP (its own door and tunnel)

The xAI API accepts a remote MCP server as a tool
(https://docs.x.ai/docs/guides/tools/remote-mcp-tools). xAI's servers make the
call, so Grok needs a public HTTPS address. Give it a door of its own, separate
from the Claude app's, on the PC that holds the memory:

```bash
lucid-dream token add grok --root <memory dir>
```

```bash
lucid-dream serve --root <memory dir> --bind 127.0.0.1:7882 --only grok
```

```bash
tailscale funnel --bg --https=8443 7882
```

Funnel offers three public ports (443, 8443, 10000), so the Claude app can keep
443 while Grok uses 8443. Then pass the tool to the API:

```json
{ "type": "mcp", "server_url": "https://<pc-name>.<tailnet>.ts.net:8443/mcp", "server_label": "lucid-dream", "authorization": "<the grok token>" }
```

The door accepts the token with or without a `Bearer ` prefix. If a client can
send no header at all, put the token at the end of the URL: `.../mcp/<token>`.

To cut Grok off, without touching anything else:

```bash
lucid-dream token remove grok --root <memory dir>
```

```bash
tailscale funnel --bg --https=8443 7882 off
```

## Waking Grok when a message arrives

If Grok can be started by a webhook (Grok Bot can), register it on the PC and
other agents' messages will wake it instead of waiting to be read:

```bash
lucid-dream webhook set grok --url <webhook address> --root <memory dir>
```

Set `LUCID_WEBHOOK_AUTH` to the webhook's `Authorization` value first. The nudge
carries no message text; Grok reads the message through its door.

Whether the Grok apps (as opposed to the API) can use a remote MCP server, and
whether xAI accepts a URL on port 8443, are not confirmed.

With direct MCP, Grok can also use `send` and `messages` to talk to the other
agents; with the digest route it cannot, so relay through another agent.

Either way the voice rule holds: Grok's own opinions stay in `notes/` and
`wiki/`, attributed, and never enter `MEMORY.md` until you confirm them.
