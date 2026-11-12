# Codex

1. On the PC that holds the memory, issue Codex its own token, so its notes carry
   its own voice:

   ```bash
   lucid-dream token add codex --root <memory dir>
   ```

2. Export it where Codex runs as `LUCID_TOKEN_CODEX`, then let lucid-dream add
   the server and the memory rule to Codex's own files (`--plan` only shows
   what it would add):

   ```bash
   lucid-dream setup --codex --url http://<pc-name>:7878/mcp
   ```

   On the machine that holds the memory the address is
   `http://127.0.0.1:7878/mcp`. It adds a `[mcp_servers.lucid-dream]` block to
   `~/.codex/config.toml` ([config.example.toml](config.example.toml)) and
   [AGENTS.md](AGENTS.md) to `~/.codex/AGENTS.md`, each once, keeping a copy
   of the file as it was. Codex has no session hooks, so that rule is what
   makes it load the index and log its sessions. To undo it, delete those
   two additions.

4. Optional: add the plugin, so Codex loads the index and logs its sessions
   by hook instead of by rule. See [plugins/README.md](../../plugins/README.md).

Check it: start Codex and ask it to "call `who` on lucid-dream". It should
list `codex (you)`.

Codex asks before each tool call that writes (`remember`, `send`, `log`,
`messages`). The last line of the example config approves this one server's
tools in advance; without it, `codex exec` (which cannot ask) refuses them.

Tried on 2026-11-12 with Codex CLI 0.164 on macOS: `who`, `messages`, `send`
and `remember` all worked against a lucid-dream server.

## Codex as the dream agent

To let Codex run the nightly dream instead of Claude, set
`LUCID_DREAM_CMD="codex exec --sandbox workspace-write"` for the dream job.
It runs on your ChatGPT subscription. Codex's sandbox does not let it run
`git mv`, so lucid-dream archives the processed inbox notes itself.
