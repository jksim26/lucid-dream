# Roadmap

Source: the Lucid Dream Roadmap board, https://claude.ai/artifact/A9MQA4ajBmiFMY1XnRyRaZ

## Build order

- [ ] **1. Repo + read server**: folder skeleton (done), MCP read tools, connect Claude on Windows
- [ ] **2. Write tools + voices**: `remember()`, one token per agent, `source` + `said_by`, connect Mac
- [ ] **3. Device hooks**: SessionStart loads the index, Stop sends a session summary
- [ ] **4. Dream job**: run by hand, review the git diff, then schedule nightly
- [ ] **5. Import old memory**: CLAUDE.md files, auto memory folders, Obsidian notes
- [ ] **6. Codex access**: AGENTS.md rule + its own token
- [ ] **7. Grok (later)**: digest file or direct MCP; verify Grok supports MCP first

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
