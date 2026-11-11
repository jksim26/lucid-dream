# Claude Code

Do this on every device where Claude Code runs. `lucid-dream` must be on the PATH.

1. On the PC that holds the memory, issue Claude's token once and keep it:

   ```bash
   lucid-dream token add claude --root <memory dir>
   ```

2. Add the MCP server, so Claude can read and write memory during a session:

   ```bash
   claude mcp add --transport http --scope user lucid-dream http://<pc-name>:7878/mcp --header "Authorization: Bearer <the claude token>"
   ```

3. Merge [settings.example.json](settings.example.json) into `~/.claude/settings.json`.
   SessionStart prints the memory index, and any messages other agents left for
   Claude, into the session; Stop sends a summary of
   the session to `logs/` (each session keeps one log, replaced as it grows).

Both hooks give up after 3 seconds and always exit cleanly, so a sleeping PC only
means a session without shared memory. To see why a hook did nothing, run it by
hand with `LUCID_DEBUG=1`.
