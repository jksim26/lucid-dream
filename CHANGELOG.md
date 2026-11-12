# Changelog

## 0.9.1 (2026-11-12)

- Wrong pairing guesses no longer cancel a waiting code, which let anyone
  who could reach a door interrupt a setup. The code's length and expiry
  are the protection.

## 0.9.0 (2026-11-12)

- Pairing: `lucid-dream pair <agent>` makes a one-time code that the agent
  exchanges for its own token at `/pair`, so nobody copies a token by hand.
  A code works once, for 60 minutes, only at that agent's door; wrong
  guesses cancel waiting codes.
- `lucid-dream brief grok|chatgpt` prints a block to paste into that agent:
  its address and a pairing code, with instructions to connect itself.
- `quickstart --tunnels` opens the Tailscale tunnels; quickstart no longer
  prints tokens for Grok and ChatGPT. A door can start before its agent has
  a token.
- The README demo is now a real exchange between a Claude Code session and
  Grok Bot.

## 0.8.1 (2026-11-12)

- The dream lock is taken atomically and a running dream's lock is never
  removed, so two dreams cannot overlap; "dream started" is only said once
  the lock is held.
- Agents may ask for a dream at most every 30 minutes, and the `dream` tool
  is marked as one that rewrites, so clients can confirm it.

## 0.8.0 (2026-11-12)

- `serve --dream-at 03:00` makes the server tidy the memory itself every
  day, with no scheduler to set up; it catches up if the machine was off.
  `--dream-with` chooses the agent.
- A `dream` tool lets any agent start the dream now or ask how the last one
  went, so a cloud routine can be the clock and you can still run it by hand.
- Only one dream runs at a time. Each leaves a record, and `doctor` says when
  the last one failed.
- `quickstart` prints a server command with the nightly dream included.

## 0.7.0 (2026-11-12)

- A message sent with no `session` now goes to exactly one session of that
  agent instead of all of them: the session being replied to, otherwise the
  lowest-numbered one listening. `send` says which one it chose.
- `session` accepts several numbers (`"1,3"`) or `"all"` when more than one
  session should get the message.
- `send` takes `from_session`, the sender's own number, so a reply comes back
  to the session that asked. Claude Code sessions are told to pass it.

## 0.6.0 (2026-11-12)

- `lucid-dream quickstart <dir> --agents grok,chatgpt,codex` creates the
  store, issues a token per agent and prints the commands to run next.
- `serve --doors grok=7882,chatgpt=7883` runs the main server and a door per
  cloud agent in one process.
- A start-to-finish guide for people without Claude:
  docs/grok-chatgpt-codex.md. The ChatGPT guide now states the Sites limits.
- Codex works as a memory client (tried with Codex CLI 0.164).
- `doctor` is useful on a machine without Claude Code or a token: it checks
  the store and that the server answers.

## 0.5.2 (2026-11-12)

- Dream archives leftover inbox notes only when the agent actually wrote
  something, so an agent that exits cleanly without working loses no notes.

## 0.5.1 (2026-11-12)

- Dream archives the inbox notes itself when the agent could not, so it works
  with agents whose sandbox refuses `git mv`. Tried with the Codex CLI:
  `LUCID_DREAM_CMD="codex exec --sandbox workspace-write"`.
- A waiting hook left running across an upgrade is replaced by the new
  version after the session's next turn; `doctor` says when one is still old.
- `upgrade` on Windows no longer fails when an old copy is still running.

## 0.5.0 (2026-11-12)

- `lucid-dream doctor` checks the version, hooks, token, server and (with
  `--root`) the memory store, and says what to fix. It changes nothing.
- `lucid-dream upgrade` replaces the program with the latest release after
  verifying its checksum; `--check` only looks.
- `lucid-dream uninstall` takes out exactly what `setup` added, with a backup;
  `--plan` shows it first. The memory is not touched.
- Dream can fall back: `LUCID_DREAM_CMD` may list several agents separated by
  `||`, tried in order until one succeeds.
- CI scans every push for committed secrets.

## 0.4.2 (2026-11-12)

- `setup` creates its temp and backup files owner-only from the start,
  never writing the token into a file that already existed with a looser mode.

## 0.4.1 (2026-11-12)

- `setup` writes the settings file and its backup readable by their owner
  only, since they hold the device's token.

## 0.4.0 (2026-11-12)

- `lucid-dream setup` connects Claude Code in one step: hooks, wake-ups and
  the MCP server, with a backup of the settings file.
- Trusted messengers: `trust add <agent>` lets that agent's requests count
  as yours for routine, undoable actions. The server makes the mark.

## 0.3.3 (2026-11-12)

- MIT license, security policy, this changelog.
- Install scripts for Linux, macOS and Windows that check the download
  against the release's SHA-256 checksums.

## 0.3.2 (2026-11-12)

- The wake-up text shows which session a message was addressed to.

## 0.3.1 (2026-11-12)

- `hook wait`: a background hook that wakes any Claude Code session, the
  desktop app included, when a message arrives for it.
- `hook session-end` takes a closed session off the list at once; a quiet
  session stays listed for three days.

## 0.3.0 (2026-11-12)

- Sessions check in and get a number; `who` lists agents and sessions;
  `send` can address one session.
- The server signals listeners the moment a message is sent, so delivery
  takes under a second.

## 0.2.5 (2026-11-12)

- Webhook wake-ups for agents that can be woken by one.
- A Claude Code channel that pushes messages into an open session.
- Session logs are scrubbed of credentials. ChatGPT joins through a relay Site.

## 0.2.4 (2026-11-11)

- A message stays unread until a session that handled it marks it read.

## 0.2.3 (2026-11-11)

- `GET /mcp` answers, so clients that probe first can connect.

## 0.2.2 (2026-11-11)

- Dream writes only what its sources say. `--version`.

## 0.2.1 (2026-11-11)

- Dream no longer logs itself as a session.

## 0.2.0 (2026-11-11)

- First release: the memory server, tokens and doors, messages between
  agents, dream, import, Claude Code hooks, prebuilt binaries.
