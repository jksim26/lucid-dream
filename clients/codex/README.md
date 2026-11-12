# Codex

1. On the PC that holds the memory, issue Codex its own token, so its notes carry
   its own voice:

   ```bash
   lucid-dream token add codex --root <memory dir>
   ```

2. Export it where Codex runs as `LUCID_TOKEN_CODEX`, and add
   [config.example.toml](config.example.toml) to `~/.codex/config.toml`.

3. Append [AGENTS.md](AGENTS.md) to `~/.codex/AGENTS.md`. Codex has no session
   hooks, so this rule is what makes it load the index and log its sessions.

To let Codex run the nightly dream instead of Claude, set
`LUCID_DREAM_CMD="codex exec --sandbox workspace-write"` for the dream job.
