# Grok

Two ways in, depending on what can reach the server.

## Digest file (works today)

Dream writes `digest/YYYY-MM-DD.md` every night: what was added, changed and
archived, readable on its own. Paste or attach the latest digest (and
`MEMORY.md`) at the start of a Grok conversation. Grok reads memory this way but
writes nothing back; tell another agent what Grok concluded and it can
`remember` it as your statement.

## Direct MCP (needs a tunnel)

The xAI API accepts a remote MCP server as a tool
(https://docs.x.ai/docs/guides/tools/remote-mcp-tools):

```json
{ "type": "mcp", "server_url": "https://<public-host>/mcp", "server_label": "lucid-dream", "authorization": "<the grok token>" }
```

xAI's servers make the call, so the server must be reachable from the internet
over HTTPS. Tailscale alone only reaches your own devices; it needs Tailscale
Funnel or another tunnel in front of port 7878. Issue the token with
`lucid-dream token add grok --root <memory dir>`. Whether the Grok apps (as
opposed to the API) can use a remote MCP server is not confirmed.

With direct MCP, Grok can also use `send` and `messages` to talk to the other
agents; with the digest route it cannot, so relay through another agent.

Either way the voice rule holds: Grok's own opinions stay in `notes/` and
`wiki/`, attributed, and never enter `MEMORY.md` until you confirm them.
