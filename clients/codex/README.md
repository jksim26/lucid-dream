# Codex

1. On the PC that holds the memory, issue Codex its own token, so its notes carry
   its own voice:

   ```bash
   lucid-dream token add codex --root <memory dir>
   ```

2. Export it where Codex runs as `LUCID_TOKEN_CODEX`, and add
   [config.example.toml](config.example.toml) to `~/.codex/config.toml`. On
   the machine that holds the memory the address is
   `http://127.0.0.1:7878/mcp`.

3. Append [AGENTS.md](AGENTS.md) to `~/.codex/AGENTS.md`. Codex has no session
   hooks, so this rule is what makes it load the index and log its sessions.

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
