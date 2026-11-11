# lucid-dream

One shared memory that every agent reads and dream tidies.

Claude, Codex and Grok write tagged notes to an add-only inbox over MCP. A nightly
"dream" job merges them into notes, a wiki and a small `MEMORY.md` index, keeps each
voice, archives what is replaced, and commits to git. Every agent on every device
loads the index at session start.

It is also the bridge between the agents: any of them can leave a message for
another, and it is delivered at the start of that agent's next session.

Roadmap board: https://claude.ai/artifact/A9MQA4ajBmiFMY1XnRyRaZ

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
| `.lucid/` | Token hashes and who has seen which message; ignored by git | `token add`, server |

Every note records two things: `source`, the agent that wrote it down, and
`said_by`, whose statement it is. `said_by: user` is your own decision; anything
else is that agent's opinion. An agent can speak for itself or report you, never
for another agent. The [voice rules](docs/roadmap.md#voice-rules) build on this.

## Set up the PC that holds the memory

```bash
cargo install --path .
```

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

The server offers eight tools:

| Tool | What it does |
| --- | --- |
| `index` | Returns `MEMORY.md` |
| `list`, `read`, `search` | Browse and search the store |
| `remember` | Adds a note to `inbox/` for dream to merge |
| `log` | Records a session summary in `logs/` |
| `send` | Leaves a message for another agent, or for `all` |
| `messages` | Returns messages the caller has not seen yet |

Reads refuse absolute paths, `..`, hidden files and symlinks that leave the
store. Writes only ever add files to `inbox/`, `logs/` and `messages/`.

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

## Status

All eight roadmap steps are built; see [docs/roadmap.md](docs/roadmap.md) for
what is left to deploy and [docs/references.md](docs/references.md) for the
projects this borrows from.
