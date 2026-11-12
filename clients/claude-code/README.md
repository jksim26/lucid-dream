# Claude Code

Do this on every device where Claude Code runs. `lucid-dream` must be on the PATH.

1. On the PC that holds the memory, issue Claude's token once and keep it:

   ```bash
   lucid-dream token add claude --root <memory dir>
   ```

2. Connect Claude Code in one step. It asks for the token, shows what it adds
   to `~/.claude/settings.json`, keeps a backup, and registers the MCP server:

   ```bash
   lucid-dream setup --url http://<pc-name>:7878/mcp
   ```

   `--plan` shows the changes without making them. That is all; the remaining
   steps are the same thing done by hand.

   By hand: add the MCP server, so Claude can read and write memory during a
   session:

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

## Live messages (waiting hook)

The `hook wait` entries in the settings example make every session listen,
including sessions in the desktop app. The hook runs in the background from
session start and after each turn, stays connected to the server, and when a
message arrives for the session it exits and Claude Code wakes the session with
the message (`asyncRewake`). Nothing else has to be running.

A message sent to the agent without a `session` wakes every waiting session;
one sent with a session's number wakes only that one. `hook session-end` takes
a closed session off the `who` list at once.

## Live messages (channel)

The channel does the same job for a terminal session and predates the waiting
hook; use one or the other.


By default Claude sees messages from other agents when a session starts. With a
channel, a message that arrives while a session is open is pushed into it and
Claude can answer straight away. Channels are a Claude Code research preview,
work in the terminal, and need a flag at every start.

1. Make a folder for the listening session, for example `~/lucid-listener`, and
   put this in a file named `.mcp.json` inside it:

   ```json
   { "mcpServers": { "lucid-channel": { "command": "lucid-dream", "args": ["channel"] } } }
   ```

2. Start Claude Code in that folder with the channel loaded:

   ```bash
   claude --dangerously-load-development-channels server:lucid-channel
   ```

   Accept the warning and the prompt to use the new server. The helper uses the
   `LUCID_URL` and `LUCID_TOKEN` already in your settings.

Each session gets a number automatically, and is asked to keep it at the front
of its title, like `#3 Login bug`. Run `lucid-dream who` to see them all, or
ask a session which number it is. Other agents see the numbers with the `who`
tool and reach one session by adding `session` to `send`. In a terminal session
Claude cannot set the title itself; use `/rename #3 Login bug` if you want it. A message sent without a
session goes to every listening session, so each of them would answer it. Messages stay unread on the
server until Claude marks them read, so nothing is lost if the session is
closed; the next session start shows them as usual.

A pushed message starts a turn in that session without you. The helper tells
Claude to treat it as a colleague's note and not to take risky actions on its
say-so, and the session's normal permission prompts still apply. Do not run the
listening session with permission prompts turned off.
