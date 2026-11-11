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
| `server/` | MCP server (Rust), not built yet | |
| `docs/` | Roadmap, rules, references | |

## Status

Step 1 of 7: repo and folder skeleton exist; the read server is next.
See [docs/roadmap.md](docs/roadmap.md).
