# lucid-dream

> Your AI agents can message and wake each other, across vendors and devices,
> and share one memory. Ask Grok on your phone to tell Claude on your PC
> something; the Claude session wakes up and answers.

[![Release](https://img.shields.io/github/v/release/jksim26/lucid-dream)](https://github.com/jksim26/lucid-dream/releases/latest)
[![CI](https://github.com/jksim26/lucid-dream/actions/workflows/ci.yml/badge.svg)](https://github.com/jksim26/lucid-dream/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT-blue)](LICENSE)

Claude, Codex, Grok and ChatGPT each live in their own app with their own
memory, and none of them can reach another. lucid-dream is one small server
you run on your own machine that gives them two things in common.

**A bridge.** Any agent can send a message to another, or to one particular
session of it. The recipient is woken within a second where its app allows
it: a Claude Code session (terminal or desktop app) by a background hook, an
agent with a webhook by a call that says only "you have a message". `who`
lists every agent and session and which are listening:

```
claude:
  #1 "Stops review" on pc (X:\code\trading): listening now
  #2 "Agent memory repo" on mac (~/code/lucid-dream): listening now
grok: woken by webhook when a message arrives
chatgpt: reads messages when it next looks
```

**A memory.** Agents write notes to an add-only inbox over MCP. A "dream" job
merges them into notes, a wiki and a small `MEMORY.md` index that every agent
loads at session start. It is plain Markdown in git: read it, edit it, revert
it. Each note keeps its voice, so your decisions are never confused with an
agent's opinion.

It is one Rust binary with no database. Each client gets its own token, and a
cloud client such as a phone app gets its own door, so one can be cut off
without touching the others.

| Agent | Memory | Sends | Woken when a message arrives |
| --- | --- | --- | --- |
| Claude Code, terminal and desktop app | yes | yes | yes, by hook |
| Grok, through its remote MCP support | yes | yes | yes, by webhook |
| Claude app on phone and web | yes | yes | no, reads on request |
| ChatGPT, through a Site it hosts | yes | yes | no, reads on request |
| Codex | guide written, not yet tested | | not yet |

## Install

Linux and macOS:

```bash
curl -fsSL https://github.com/jksim26/lucid-dream/releases/latest/download/install.sh | sh
```

Windows, in PowerShell:

```powershell
irm https://github.com/jksim26/lucid-dream/releases/latest/download/install.ps1 | iex
```

Both check the download against the release's SHA-256 checksums and install
for the current user only. Or build it: `cargo install --git
https://github.com/jksim26/lucid-dream`. Then
[set up the machine that holds the memory](#set-up-the-pc-that-holds-the-memory)
and [connect the agents](#connect-the-agents).

## How it compares

Most agent-memory projects, [ai-memory](https://github.com/akitaonrails/ai-memory)
being the most complete, are about memory for coding agents: capture, search,
handoff. If that is what you need, use one of them; they do far more of it
than this does. lucid-dream is for the other half: agents from different
vendors, including the chat apps on your phone, talking to each other and
waking each other while you are not at the keyboard. Its memory is
deliberately small.

The memory design follows Anthropic's
[components of agent memory](https://www.anthropic.com/research/components-of-agent-memory);
[docs/references.md](docs/references.md) lists every project this borrows
ideas from.

## How memory flows

```
Writers ──▶ inbox/ + logs/ ──▶ Dream (nightly) ──▶ notes/ wiki/ MEMORY.md ──▶ Readers
Claude,     add-only via MCP,   merge, keep voices,   git history is the      all agents,
Codex,      session summaries   archive, commit       undo; daily digest/     all devices
Grok        from every device
```

One binary, `lucid-dream`, does all of it: it serves the memory, runs the dream,
imports old memory, and is the hook each device calls.

## The memory store

The store is a folder of Markdown in its own git repository. **Keep it outside
this repo**: this repo is public, and the store holds your notes.

| Path | What it holds | Who writes |
| --- | --- | --- |
| `inbox/` | Raw notes, one file each, tagged `source` + `said_by` | Agents, add-only |
| `logs/` | Session summaries by date | Stop hook, `log` tool |
| `messages/` | Messages the agents sent each other | `send` tool |
| `notes/` | Consolidated notes, one topic per file | Dream |
| `wiki/` | One page per entity (project, person, tool) | Dream |
| `digest/` | Daily digest of what changed | Dream |
| `archive/` | Processed and replaced notes; nothing is hard-deleted | Dream |
| `MEMORY.md` | The index loaded at session start (under 200 lines) | Dream |
| `open-questions.md` | Your own conflicting decisions, awaiting your call | Dream |
| `.lucid/` | Token hashes, who has seen which message, session check-ins; ignored by git | `token add`, server |

Every note records two things: `source`, the agent that wrote it down, and
`said_by`, whose statement it is. `said_by: user` is your own decision; anything
else is that agent's opinion. An agent can speak for itself or report you, never
for another agent. The [voice rules](docs/roadmap.md#voice-rules) build on this.

## Set up the PC that holds the memory

[Install](#install) the binary, then create the store:

```bash
lucid-dream init <memory dir>
```

Issue one token per agent. Each is printed once; only its hash is stored.

```bash
lucid-dream token add claude --root <memory dir>
```

Serve it to your other devices. Off loopback the server will not start without
at least one token.

```bash
lucid-dream serve --root <memory dir> --bind 0.0.0.0:7878
```

The endpoint is `http://<pc-name>:7878/mcp`. `/health` answers `ok` without a
token. For a client on the same machine, `serve --stdio --as <agent>` speaks MCP
over stdio instead.

## Connect the agents

- Claude Code, with hooks: [clients/claude-code](clients/claude-code)
- Codex: [clients/codex](clients/codex)
- Claude app on phone, web or desktop: [clients/claude-app](clients/claude-app)
- Grok: [clients/grok](clients/grok)
- ChatGPT, through a Site it hosts: [clients/chatgpt](clients/chatgpt)

The server offers ten tools:

| Tool | What it does |
| --- | --- |
| `index` | Returns `MEMORY.md` |
| `list`, `read`, `search` | Browse and search the store |
| `remember` | Adds a note to `inbox/` for dream to merge |
| `log` | Records a session summary in `logs/` |
| `send` | Leaves a message for another agent, or for `all`; `session` narrows it to one session by number |
| `check_in` | Announces a session and returns its number |
| `who` | Lists the agents and their sessions, and which are listening now |
| `messages` | Returns messages the caller has not seen yet |

Reads refuse absolute paths, `..`, hidden files and symlinks that leave the
store. Writes only ever add files to `inbox/`, `logs/` and `messages/`.

## Waking an agent

A message normally waits until its recipient next looks. Two optional pieces
close that gap.

**Webhook, for an agent that can be woken by one** (set on the PC). When a
message is sent to that agent, the server calls its webhook. The call says only
that a message is waiting and from whom, never what it says; the agent reads it
through its own connection. An agent is nudged at most once every 30 seconds.

```bash
lucid-dream webhook set grok --url <webhook address> --root <memory dir>
```

The webhook's `Authorization` value is read from the `LUCID_WEBHOOK_AUTH`
environment variable, so it never appears on a command line. `webhook test grok`
sends a test nudge; `webhook list` and `webhook remove grok` manage it. The
address and its secret are stored in `.lucid/webhooks.json`, outside git.

**Waiting hook, for any Claude Code session**, the desktop app included. A
background hook waits for a message to its session and wakes the session with
it; see [clients/claude-code](clients/claude-code#live-messages-waiting-hook).

**Channel, for a Claude Code terminal session** (research preview). The
`lucid-dream channel` helper stays connected to the server, which tells it the
moment a message is sent; the helper fetches it and pushes it into the running
session, in well under a second. It also checks every 20 seconds in case the
connection dropped. See
[clients/claude-code](clients/claude-code#live-messages-channel).

Session logs are scrubbed of anything that looks like a credential before they
are sent, since logs become memory every agent can read.

## Tokens and doors

An agent can hold several tokens, told apart by a label, so each client can be
cut off on its own:

```bash
lucid-dream token add claude --label phone --root <memory dir>
```

```bash
lucid-dream token list --root <memory dir>
```

```bash
lucid-dream token remove claude --label phone --root <memory dir>
```

The main server is for your own devices on Tailscale. A client that lives in
the cloud (the Claude app, Grok) gets a **door** instead: a second listener that
accepts exactly one token, with its own tunnel in front.

```bash
lucid-dream serve --root <memory dir> --bind 127.0.0.1:7881 --only claude/phone
```

| Listener | Reached through | Accepts |
| --- | --- | --- |
| Main, port 7878 | Tailscale only | Any issued token |
| Door for the Claude app, 7881 | Its own tunnel | `claude/phone` only |
| Door for Grok, 7882 | Its own tunnel | `grok/default` only |

Never point a tunnel at the main listener. A door never admits a caller without
its token, and a device token that leaks does not open any door.

## Messages between agents

The token says who is sending, so an agent cannot pose as another. A message
waits in `messages/` until its recipient asks for it; each agent is given a
message once. Claude Code gets its messages from the SessionStart hook; Codex
and Grok call `messages` themselves. Once tokens exist, a message can only be
addressed to an agent that has one.

### Choosing a session

An agent can have several sessions open. Each one announces itself when it
starts or listens, and the server gives it a number. The number is unique
across all agents, stays with the session while it keeps checking in, and is
how the session is addressed. `who` lists them, and so does `lucid-dream who`
in a terminal:

```
claude: reads messages when it next looks
  #1 "Agent memory repo" on mac (~/code/lucid-dream): listening now
  #2 on mac (~/code/lucid-dream): listening now
  #4 "Stops review" on sim (X:\code\trading): not listening, last seen 12 min ago
grok: woken by webhook
```

`send` with `session: "2"` reaches that session only; without it, every
session of that agent gets the message. A message reaches a listening session within a second. A session counts as listening while its
waiting hook or channel helper has checked in within the last 90 seconds. A
session that ends leaves the list at once; one that just goes quiet shows as
"not listening" with when it was last seen, and drops off after three days. Its
number then goes to the next new session.

Each Claude Code session is told its number at start and asked to keep it at
the front of its title, so in the app you see `#1 Agent memory repo`. The text
after the number is what `who` shows as the session's topic.

Messages are handed over labelled as a colleague's notes, not as instructions
from you. They are not memory either: dream reads them as evidence, like logs,
and anything worth keeping still has to be remembered.

## Dream

```bash
lucid-dream dream --root <memory dir>
```

Dream commits whatever arrived since last time (`intake: ...`), hands the inbox to
an agent with [this prompt](src/dream_prompt.md), and commits the result
(`dream: <date>`). Review it with `git show --stat`; undo it with `git revert`.
`--dry-run` shows what it would do.

- The agent is `claude -p` with edits allowed and only `git mv` and `mkdir` as
  commands. Set `LUCID_DREAM_CMD` to use another one; the prompt is appended.
- If the agent deletes a file instead of archiving it, dream puts the file back
  and says so in the commit message.
- If the agent fails, tracked files are restored and nothing is committed.

Run it by hand and read the diffs for the first week. Then schedule it, on
Windows with Task Scheduler:

```bash
schtasks /Create /SC DAILY /ST 03:00 /TN lucid-dream /TR "lucid-dream dream --root C:\path\to\memory"
```

## Import old memory

Each Markdown file becomes one inbox note, so dream merges it under the same
rules as everything else. Importing the same file twice adds nothing.

```bash
lucid-dream import ~/.claude/CLAUDE.md ~/Obsidian/Vault --url http://<pc-name>:7878/mcp
```

Notes are imported as your own statements. For files an agent wrote, such as
Claude's auto-memory folders, pass that agent's name and use its token:

```bash
lucid-dream import ~/.claude/projects/<project>/memory --said-by claude
```

On the PC itself, `--root <memory dir>` writes straight into the store without
the server.

## Settings

| Variable | Used by | Meaning |
| --- | --- | --- |
| `LUCID_ROOT` | serve, dream, token, import | Memory store, instead of `--root` |
| `LUCID_BIND` | serve | Listen address (default `127.0.0.1:7878`) |
| `LUCID_URL` | hooks, import | Server endpoint, e.g. `http://<pc-name>:7878/mcp` |
| `LUCID_TOKEN` | hooks, import | This device's agent token |
| `LUCID_DREAM_CMD` | dream | Agent command; the prompt is appended |
| `LUCID_DEBUG` | hooks | Print why a hook did nothing |
| `LUCID_WEBHOOK_AUTH` | webhook set | The webhook's `Authorization` value |
| `LUCID_CHANNEL_POLL` | channel | Seconds between fallback checks (default 20) |

## Status

Young and built for one person's machines: it runs daily on a Windows PC and
a Mac with Claude, Grok and ChatGPT connected. See
[docs/roadmap.md](docs/roadmap.md) for what is built,
[CHANGELOG.md](CHANGELOG.md) for what changed, and [SECURITY.md](SECURITY.md)
for what the design relies on. MIT licensed.
