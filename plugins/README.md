# Plugins

This repository is a plugin marketplace for two agents. A plugin wires the
installed `lucid-dream` program into the agent; it does not replace it, and
it sends nothing anywhere itself. [Install the program](../README.md#install)
first.

| | Claude Code | Codex CLI |
| --- | --- | --- |
| Loads the memory index when a session starts | yes | yes |
| Logs each session (summary and full record) | yes | yes |
| Memory tools (MCP) | yes, from the plugin | from `lucid-dream setup --codex` |
| Woken when another agent sends a message | yes | no: Codex has no hook that can wake an idle session |

## Claude Code

```bash
claude plugin marketplace add jksim26/lucid-dream && claude plugin install lucid-dream@lucid-dream
```

When you enable it, Claude Code asks for two options: the server address
(`http://<pc-name>:7878/mcp`) and this device's token, from
`lucid-dream token add claude` on the machine that holds the memory. The
token is kept in your system's credential store.

If you already ran `lucid-dream setup` on this machine you do not need the
plugin. With both present, the plugin's hooks do nothing and `lucid-dream
doctor` says so; keep one.

## Codex CLI

```bash
codex plugin marketplace add jksim26/lucid-dream && codex plugin add lucid-dream@lucid-dream
```

Then:

1. Put Codex's token (`lucid-dream token add codex` on the memory's machine)
   in the environment variable `LUCID_TOKEN_CODEX` where Codex runs.
2. Run `lucid-dream setup --codex --url http://<pc-name>:7878/mcp`. It adds
   the memory tools to Codex's config; the hooks read the address from there.
3. Start Codex and trust the plugin's hooks with `/hooks`. Codex skips a
   plugin's hooks until you do.

## What was tried

On 2026-11-12, macOS, from a local copy of this marketplace: both plugins
install; the Codex plugin's hooks loaded the index and logged a real
`codex exec` session (Codex CLI 0.164, hooks trusted for that run with
`--dangerously-bypass-hook-trust`); the Claude Code plugin's hooks were run
by hand with the plugin's options and loaded the index and logged a session.

Not tried: a real Claude Code session with the plugin enabled (so the option
prompt, the memory tools through the plugin and the wake-up are unproven
there), installing from GitHub rather than a local folder, and either plugin
on Windows or Linux.
