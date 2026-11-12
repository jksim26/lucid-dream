# References

Read 2026-11-11. What each one is and what this project takes from it.

## The components of agent memory (Anthropic)

https://www.anthropic.com/research/components-of-agent-memory

Memory as a hierarchy: transcript event log, notes the agent writes to a folder,
representations such as wikis, and a small injected context on top. Dreaming is the
offline pass that moves information between layers.

Take: the layer model maps onto `logs/` → `inbox/` + `notes/` → `wiki/` → `MEMORY.md`.
Keep the injected layer small (Claude Code loads the first 200 lines or 25 KB of
`MEMORY.md`). Tell agents what memory is for rather than fixed rules on what to
record. Have dream check notes against the logs. One wiki page per entity so
parallel agents do not overwrite each other.

## grandamenium/dream-skill (MIT)

https://github.com/grandamenium/dream-skill

Claude Code skill with a four-phase pass: Orient (read the memory dir), Gather
Signal (grep recent session transcripts), Consolidate (merge, absolute dates,
resolve contradictions, drop dead references), Prune & Index (rebuild `MEMORY.md`
under 200 lines, demote detail to topic files). A Stop hook checks whether 24 hours
have passed and flags the next session to dream.

Take: the four phases as the shape of the dream job (step 4), and the cheap
Stop-hook check as a fallback trigger when the nightly schedule is missed.

## karrolcia/hippocampus (AGPL-3.0)

https://github.com/karrolcia/hippocampus

Self-hosted MCP memory server: TypeScript, Streamable HTTP at `/mcp`, encrypted
SQLite knowledge graph, local embeddings, OAuth 2.1 or bearer token. Tools include
remember, recall, forget, merge, consolidate, context, export.

Take: tool naming (`remember`, `recall`, `context`) and Streamable HTTP as the
transport for a server other devices reach. Not its storage: this project stays
plain Markdown in git. AGPL, so ideas only, no code.

## CameronBadman/Hippocampus (no license stated)

https://github.com/CameronBadman/Hippocampus

Go vector store, one binary file per agent, on AWS Lambda with Bedrock embeddings.

Take: little. A cloud vector store is the opposite of local Markdown + git; worth
revisiting only if grep-style search stops being enough. No license, so no code.

## dreamide/dream (MIT)

https://github.com/dreamide/dream

Electron IDE for running several coding agents (Claude, Codex, OpenCode, Cursor,
Grok). Not a memory system.

Take: a possible client. Any agent it runs can use this memory through MCP.

## akitaonrails/ai-memory (MIT)

https://github.com/akitaonrails/ai-memory (read 2026-11-12, README only)

Long-term memory for coding agents: lifecycle hooks capture work silently, a
git-backed Markdown wiki is the source of truth with a derived search index,
memory ages without an LLM, and handoffs between agents are claimed once. Far
broader than this project (dozens of agent integrations, teams, a console).

Take: how it is packaged, not its code. A checksum-verified install script, a
changelog and security policy, and a comparison that says plainly what each
tool is for. Ideas to consider later: a one-command `setup` that wires every
agent it finds, an indexed full-text search, handoffs with an owner.
