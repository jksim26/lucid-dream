# Changelog

## 0.18.6 (2026-11-14)

- The Stop hook's request for the session number in the title is made only
  in Claude Code. A Codex session has no title to set and was never tested
  with that request, so it is left alone.

## 0.18.5 (2026-11-14)

- A Claude Code session whose title does not start with its session number
  is stopped once at the end of a turn and asked to set it. The start text
  alone was not enough: a session busy with its first task put the rename
  off and forgot. It is asked once per session and number, so a session with
  no title tool is not stopped again, and never in a headless run.

## 0.18.4 (2026-11-14)

- The start text tells a Claude session to message another Claude session
  with its app's own session messaging (`ListAgents`, then `SendMessage`)
  first, and to use the lucid-dream `send` tool only when that fails or the
  session is not listed there. Messages to grok, codex and chatgpt still go
  through `send`.

## 0.18.3 (2026-11-14)

- The start text says to replace a session number already in the title when
  it differs from the session's current one. A session that ends and comes
  back gets a new number, and its title kept the old one.

## 0.18.2 (2026-11-14)

- One session can message another session of the same agent: `send` to your
  own agent name with `session` set to the other session's number. Before,
  any message from claude to claude was refused, so Claude sessions could
  not reach each other through the server. A session still cannot message
  itself, and a message to your own agent must name the session.
- The start text tells a session to load its title tool if it has to, so the
  session number reaches the title where that tool is not loaded yet.

## 0.18.1 (2026-11-14)

- Fixed: since 0.17.0 a session that started in a folder with a project page
  was shown only the first 2,000 characters of its start text, with no
  session number, no messages and most of the index missing. The index and
  the page together passed the size Claude Code shows whole. The start text
  now always fits: the session's number and unread messages come first, then
  the index, then the project's page if there is room, or a line saying which
  page to read.
- The dream is told to keep `MEMORY.md` under 7,000 characters, and warns
  when it is larger.

## 0.18.0 (2026-11-14)

- Automatic upgrades, off unless you turn them on. `lucid-dream upgrade --auto
  on` (or `serve --auto-upgrade`) lets a machine upgrade itself: a server
  looks once a day, half an hour after its dream, installs a newer release
  and restarts in place; sessions look about once a day in the background,
  and the next session runs the new version. `upgrade --auto status` shows
  the switch and what the last automatic upgrade did.
- An upgrade, automatic or by hand, now starts the downloaded program and
  checks the version it reports before putting it in place. A download that
  does not run changes nothing.

## 0.17.3 (2026-11-14)

- `hook wait` returns at once in a headless session (`claude -p`, an Agent SDK
  program, or any session Claude Code marks as unattended). Claude Code waits
  for background hooks before a `claude -p` run ends, so with the wait hook
  installed every headless run hung with no output.

## 0.17.2 (2026-11-13)

- `upgrade` and `doctor` know when a release is minutes old and its files
  are still being built: they say to try again shortly instead of failing on
  a missing file or telling you to upgrade to something that is not there.
- The dream log names the agent and model of each attempt, and which one
  finished the dream.
- A search that went into the deeper layer says so on its first line, and why
  (working memory had no match, or a reason was given).

## 0.17.1 (2026-11-13)

- The project page that comes with `index` is read with the caller's own
  rights, so a file named like a session record cannot reach an untrusted
  door that way.

## 0.17.0 (2026-11-13)

- A new session is handed the page about the project it works in. The
  session-start hook sends the folder's name with `index`, and the server
  adds the page from `wiki/` or `notes/` that is named like the folder, that
  lists it under `Also called:`, or whose name the folder's name starts with.
  Other agents can pass `project` to `index` themselves.
- The dream adds each project's folder name to its page's `Also called:` line.
- The instructions say when to search at all: for something about the user,
  their projects or past decisions that the index does not answer.

## 0.16.0 (2026-11-13)

- The server now decides when a search may go past working memory. A search
  that asks for the archive, session logs or messages (`deep`, or a `dir` that
  reaches them) gets them only when working memory has no match for the same
  words. Otherwise it returns the working-memory matches and says so.
- `search` takes a `reason`. With one, the deeper layer is searched even when
  working memory has matches, and the reason is written with the agent's name
  to `logs/<date>/deep-searches.log`.
- Reading a file that a result points to is unchanged and always allowed.

## 0.15.0 (2026-11-12)

- The dream picks its model by the size of the job: Sonnet for a normal
  night, Opus when more than 40 notes wait. A model you set is always used.
- A bare `codex` as the dream agent is now a built-in command on Codex's
  workhorse model (`gpt-6.1-sol`, or `LUCID_DREAM_CODEX_MODEL`), then on
  Codex's own default. `quickstart` and the guide use it.
- The dream prompt asks for the voice mark on each line, imported facts
  included (`(claude's record, imported, 2026-08-04)`), not one sentence at
  the top of a page.
- `lucid-dream dream --also "<request>"` adds one request to a dream started
  by hand.

## 0.14.0 (2026-11-12)

- The dream asks Claude Code for Sonnet instead of running on whatever
  model the machine's own sessions default to. `--dream-model` on `serve`
  and `dream`, or `LUCID_DREAM_MODEL`, picks another (`haiku`, `opus`, a full
  model name); `default` leaves it to Claude Code. If the model asked for is
  refused, Claude Code's own default is tried next.

## 0.13.1 (2026-11-12)

- `read` takes `from_line` and `lines`, so a file too large to read whole,
  such as a long session record, can be read around a search hit. Before
  this, a large record could be searched but not read.

## 0.13.0 (2026-11-12)

- `lucid-dream import --auto` finds the memory Claude Code and Codex already
  keep on a machine (`CLAUDE.md`, Claude Code's memory notes from every
  project, `AGENTS.md`), lists it and imports it after you agree. The dream
  marks imported notes as such and lets a newer note win.
- The Grok Bot and ChatGPT briefs ask the agent to save what it already
  remembers about you into shared memory.
- `lucid-dream call <tool> ['<json>']` uses one memory tool from a shell, so
  an agent with no MCP client can share the memory. `lucid-dream rule`
  prints the rule to paste into its instructions file. See
  `clients/generic`.
- Agents are told to say in one line what a picture or file shows, since
  pictures are noted in the record but not stored.

## 0.12.0 (2026-11-12)

- Session records are sealed at the doors. An agent that comes in through a
  door (the listeners the internet can reach) can no longer read, list or
  search `.record.md` files unless the user has named it trusted with
  `lucid-dream trust add <agent>`. Summaries, notes and pages are unchanged,
  and the user's own machines and the dream see everything as before.
- `lucid-dream record off|on|status [<folder>]` turns full records off for a
  project folder on this machine (a `.lucid-no-record` marker). Summaries
  are still sent.
- The scrub now also hides random-looking strings from 20 characters, not
  only from 32, while leaving names made of words alone.

## 0.11.0 (2026-11-12)

- Plugins. This repository is now a plugin marketplace for Claude Code
  (`.claude-plugin/marketplace.json`, `plugins/claude-code`) and for the
  Codex CLI (`.agents/plugins/marketplace.json`, `plugins/codex`). The Claude
  Code plugin brings the hooks and the memory tools and asks for the server
  address and token; the Codex plugin brings hooks that load the index when
  a session starts and log it when a turn ends. Both need the lucid-dream
  program installed.
- Codex sessions are logged by hook: `hook ... --agent codex` reads Codex's
  session file, takes the address from Codex's config and the token from
  `LUCID_TOKEN_CODEX`.
- Where `setup` already installed the hooks, a plugin's copy stands aside,
  and `doctor` says which route is in use.
- The session record notes a picture or file that was attached
  (`[image image/png attached; its contents are not stored]`) instead of
  dropping it silently.
- `CLAUDE_CONFIG_DIR` is honoured when looking for Claude Code's settings.

## 0.10.0 (2026-11-12)

- Full session records. The Claude Code stop hook now also sends what was
  said and done in the session (prompts, replies, each tool call and its
  result, long ones cut short, credentials scrubbed) to
  `logs/<date>/<session>-<agent>.record.md`. Only the new part is sent each
  turn. Nothing loads a record; it is there to be searched or checked.
- Search has two layers. By default it covers working memory (index, notes,
  wiki, notes not yet merged); `deep: true` adds the archive, logs and
  messages. A query may hold several words separated by commas, and files
  matching more of them come first.
- Dream links pages to each other, ends each with `Related:` and
  `Also called:` lines, and says where a left-out detail can be found.
- `--backup-to <folder>` on `serve` or `dream` writes the whole memory as one
  git bundle after each dream. Off unless asked for.
- `lucid-dream setup --codex --url URL` adds the MCP server and the memory
  rule to the Codex CLI's own files.
- The Grok Bot and ChatGPT briefs ask the agent to keep the logging and
  search rules as a standing instruction; the ChatGPT relay gains `log`.

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
