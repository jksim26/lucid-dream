# lucid-dream

One shared memory that every agent reads and dream tidies.

Claude, Codex and Grok write tagged notes to an add-only inbox over MCP. A nightly
"dream" job merges them into notes, a wiki and a small `MEMORY.md` index, keeps each
voice, archives what is replaced, and commits to git. Every agent on every device
loads the index at session start.

Roadmap board: https://claude.ai/artifact/A9MQA4ajBmiFMY1XnRyRaZ

## How memory flows

```
Writers ──▶ inbox/ + logs/ ──▶ Dream (nightly) ──▶ notes/ wiki/ MEMORY.md ──▶ Readers
Claude,     add-only via MCP,   merge, keep voices,   git history is the      all agents,
Codex,      session summaries   archive, commit       undo; daily digest/     all devices
Grok        from every device
```

The server runs on the Windows PC and is reached over Tailscale.

## Layout

| Path | What it holds | Who writes |
| --- | --- | --- |
| `memory/inbox/` | Raw notes, one file each, tagged `source` + `said_by` | Agents, add-only |
| `memory/logs/` | Session summaries from device hooks | Stop hook, add-only |
| `memory/notes/` | Consolidated notes, one topic per file | Dream |
| `memory/wiki/` | One page per entity (project, person, tool) | Dream |
| `memory/digest/` | Daily digest of what changed | Dream |
| `memory/archive/` | Replaced or retired notes; nothing is hard-deleted | Dream |
| `memory/MEMORY.md` | The index loaded at session start (under 200 lines) | Dream |
| `memory/open-questions.md` | Your own conflicting decisions, awaiting your call | Dream |
| `server/` | MCP server (Rust): read tools over HTTP or stdio | |
| `docs/` | Roadmap, rules, references | |

## Server

Read-only MCP server with four tools: `index` (returns `MEMORY.md`), `list`, `read`
and `search`. It refuses absolute paths, `..`, hidden files and symlinks that leave
the memory root.

```bash
cd server && cargo build --release
```

Run it over HTTP (the endpoint is `/mcp`; `/health` answers `ok` without a token):

```bash
LUCID_TOKEN=<secret> ./target/release/lucid-dream-server --root <memory dir> --bind 0.0.0.0:7878
```

Or over stdio, for a client on the same machine:

```bash
./target/release/lucid-dream-server --root <memory dir> --stdio
```

| Setting | Flag | Environment | Default |
| --- | --- | --- | --- |
| Memory root | `--root` | `LUCID_ROOT` | none, required |
| Listen address | `--bind` | `LUCID_BIND` | `127.0.0.1:7878` |
| Bearer token | | `LUCID_TOKEN` | none; required off loopback |

Connect Claude Code from another device on the tailnet:

```bash
claude mcp add --transport http lucid-dream http://<pc-name>:7878/mcp --header "Authorization: Bearer <secret>"
```

## Keep real memory out of this repo

This repo is public. `memory/` here is an empty template. Copy it somewhere else,
run `git init` there, and point `--root` at that copy, so your notes and dream's
nightly commits stay in a private repo of their own.

## Status

Step 1 of 7: the skeleton and the read server exist; connecting Claude on Windows
is left. See [docs/roadmap.md](docs/roadmap.md).
