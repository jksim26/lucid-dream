# Changelog

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
