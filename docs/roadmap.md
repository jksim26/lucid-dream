# Roadmap

Source: the Lucid Dream Roadmap board, https://claude.ai/artifact/A9MQA4ajBmiFMY1XnRyRaZ

## Build order

Everything below is built and tested on a Mac. What is left is deployment: running
the server on the Windows PC and pointing each agent at it (see the README).

- [x] **1. Repo + read server**: folder skeleton, MCP read tools
- [x] **2. Write tools + voices**: `remember` and `log`, one token per agent, `source` + `said_by`
- [x] **3. Device hooks**: SessionStart loads the index, Stop sends a session summary
- [x] **4. Dream job**: `lucid-dream dream`, run by hand and review the git diff, then schedule nightly
- [x] **5. Import old memory**: `lucid-dream import` for CLAUDE.md files, auto memory folders, Obsidian notes
- [x] **6. Codex access**: AGENTS.md rule + its own token ([clients/codex](../clients/codex))
- [ ] **7. Grok (later)**: the xAI API can call a remote MCP server, but only one it can reach from
  the internet; until there is a tunnel, give Grok the daily digest ([clients/grok](../clients/grok))
- [x] **8. Agent bridge** (added 2026-11-11): `send` and `messages` tools, so the agents
  talk to each other across sessions and devices, not only share memory

Not done anywhere yet:

- [ ] Run the server on the Windows PC and connect Claude on Windows
- [ ] Connect the Mac, install the hooks, import old memory
- [ ] Schedule the nightly dream after a week of reviewing its diffs by hand

## Voice rules

- Your decisions beat agent opinions, whoever recorded them.
- A newer decision replaces the older one; the old one goes to `archive/`.
- When agents disagree, both views are kept.
- Your own conflicting decisions go to `open-questions.md`.
- Grok's own opinions never enter `MEMORY.md` unconfirmed.

## Watch out

- PC asleep means memory offline; hooks must fail quietly.
- Tailscale only reaches your devices; cloud agents need a tunnel.
- Grok MCP support is unverified.
- Dream can mis-merge: review diffs in the first week.
- Nothing is hard-deleted: `archive/` + git are the safety net.
