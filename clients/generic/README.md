# Any other agent

An agent does not need to be Claude, ChatGPT or Grok Bot to share the memory.
What it needs depends on the job.

| Job | What the agent needs |
| --- | --- |
| Read and write memory | MCP, **or** just a shell |
| Log its sessions | Hooks (Claude Code, Codex) or a rule it follows |
| Run the nightly dream | To be an agent CLI that can read and edit files |

## With MCP

Most agent CLIs can add a remote MCP server. Give yours the address
`http://<pc-name>:7878/mcp` and a bearer token from
`lucid-dream token add <name> --root <memory dir>`, then paste the rule from
`lucid-dream rule --mcp` into its instructions file.

## With only a shell

An agent that can run commands can use the memory with no MCP at all. Set
`LUCID_URL` and `LUCID_TOKEN` where it runs (or pass `--root <dir>` on the
machine that holds the memory), and every tool is one command:

```bash
lucid-dream call search '{"query": "release, deploy, ship"}'
```

```bash
lucid-dream call remember '{"text": "We ship on Fridays.", "topic": "release"}'
```

`lucid-dream rule` prints the rule that teaches an agent these commands;
paste it into the file your agent reads at the start of a session
(`AGENTS.md` for many of them). The text is [AGENTS.md](AGENTS.md).

## As the dream agent

`LUCID_DREAM_CMD` (or `serve --dream-with`) takes any command; the dream's
prompt is added as its last argument. It must be an agent that reads and
edits files in the current folder and then exits, and it may run `git mv`
and `mkdir`. Several can be listed with `||`, tried in order.

## Limits to know

- A chat model alone (DeepSeek's API, a local model in Ollama) is not an
  agent. It needs an agent CLI around it that can run commands and edit
  files.
- The dream is only as good as its model. It has to follow a long prompt and
  keep who said what apart. Small models are likely to merge badly; every
  dream is one git commit, so a bad one can be reverted.
- Without hooks nothing is logged automatically and the agent is not woken
  by a message; it logs and reads messages when its rule tells it to.
- A picture is noted, not stored, and only an agent that can see pictures
  can say what one shows.

Tried: the commands above against a store, by hand. Not tried: any agent
other than Claude Code and Codex following the rule, and the dream with any
agent other than those two.
