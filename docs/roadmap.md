# Roadmap

Source: the Lucid Dream Roadmap board, https://claude.ai/artifact/A9MQA4ajBmiFMY1XnRyRaZ

## Build order

Everything ticked below is built, tested and running on a Windows PC and a Mac.

- [x] **1. Repo + read server**: folder skeleton, MCP read tools
- [x] **2. Write tools + voices**: `remember` and `log`, one token per agent, `source` + `said_by`
- [x] **3. Device hooks**: SessionStart loads the index, Stop sends a session summary
- [x] **4. Dream job**: `lucid-dream dream`, run by hand and review the git diff, then schedule nightly
- [x] **5. Import old memory**: `lucid-dream import` for CLAUDE.md files, auto memory folders, Obsidian notes
- [x] **6. Codex access**: AGENTS.md rule + its own token ([clients/codex](../clients/codex))
- [x] **7. Grok**: its own door and tunnel for the xAI API, or the daily digest without one
  ([clients/grok](../clients/grok)); not yet tried against the real API
- [x] **8. Agent bridge** (added 2026-11-11): `send` and `messages` tools, so the agents
  talk to each other across sessions and devices, not only share memory
- [x] **9. Doors for cloud clients** (added 2026-11-11): labelled, revocable tokens and
  `serve --only`, so the Claude app and Grok each get their own token and tunnel
- [x] **10. Wake-ups** (added 2026-11-12): webhook nudges for agents that have one, and a
  Claude Code channel helper that pushes messages into an open session
- [x] **11. ChatGPT** (added 2026-11-12): joins through a relay Site that ChatGPT hosts,
  with its own token, door and tunnel ([clients/chatgpt](../clients/chatgpt))
- [x] **12. Sessions** (added 2026-11-12): sessions announce themselves and get a
  number, `who` lists them, and `send` can address one by number
- [x] **13. Trusted messengers and setup** (added 2026-11-12): `trust add <agent>` for
  requests relayed on your behalf; `setup` wires Claude Code in one command

- [x] **14. Looking after an install** (added 2026-11-12): `doctor`, `upgrade` and
  `uninstall`; dream falls back to a second agent when the first fails
- [x] **15. Without Claude** (added 2026-11-12): `quickstart`, one server with several
  doors, Codex as a memory client, and a guide for Grok + ChatGPT + Codex users
- [x] **16. One session per message** (added 2026-11-12): an unaddressed message goes to
  one session, several or all on request, and replies return to the asker
- [x] **17. Dream without a scheduler** (added 2026-11-12): the server dreams daily with
  `--dream-at`, and any agent can start one with the `dream` tool
- [x] **18. Deeper memory** (added 2026-11-12): full session records by hook, search in
  two layers with several words, pages linked and given alternative words by dream,
  optional backup after each dream, `setup --codex`
- [x] **19. Plugins** (added 2026-11-12): this repo is a marketplace for a Claude Code
  plugin and a Codex plugin; Codex sessions are logged by hook
- [x] **20. Record limits** (added 2026-11-12): records sealed at the doors except for
  trusted agents, a per-project off switch, a tighter scrub

Not done yet:

- [ ] `service install`: start the server at login on Windows, macOS and Linux
- [ ] A server that needs no home computer or tunnel: a container for a cloud host,
  with the dream done by a connected agent over MCP

- [ ] Walk the Grok + ChatGPT + Codex guide from scratch on Windows and Linux
- [ ] Try `install.ps1` and `setup` on Windows from scratch
- [ ] Indexed full-text search, once scanning files gets slow
- [ ] A session-start index with a size budget, as the memory grows

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
