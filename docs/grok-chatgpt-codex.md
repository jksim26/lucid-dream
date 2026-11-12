# Set up with Grok, ChatGPT and Codex (no Claude needed)

This is the whole setup for someone who uses Grok Bot and ChatGPT or Codex.
At the end, Grok and ChatGPT can message each other, Grok is woken when a
message arrives, and all of them share one memory that Codex tidies at night.

Plan for about 30 minutes.

## What you need

| You need | Why |
| --- | --- |
| A computer that stays on (Windows, macOS or Linux) | It holds the memory and runs the server |
| [Tailscale](https://tailscale.com), free plan | Gives Grok and ChatGPT a public HTTPS address to your computer |
| ChatGPT Plus, Pro, Business or Enterprise | ChatGPT connects through a Site, and those are the plans OpenAI lists Sites for |
| The Codex CLI, signed in with that ChatGPT account | Runs the nightly tidy-up ("dream"); optional as a memory client |
| Grok Bot | Connects to a remote MCP server and can be woken by a webhook |

No API keys are needed. Dream runs on your ChatGPT subscription through Codex.

## 1. Install

macOS or Linux:

```bash
curl -fsSL https://github.com/jksim26/lucid-dream/releases/latest/download/install.sh | sh
```

Windows, in PowerShell:

```powershell
irm https://github.com/jksim26/lucid-dream/releases/latest/download/install.ps1 | iex
```

## 2. Create the memory and the tokens

Pick a folder for the memory and name your agents:

```bash
lucid-dream quickstart ~/lucid-memory --agents grok,chatgpt,codex
```

It creates the folder, prints a token for Codex (shown once; a token is that
agent's password, never post one), and prints the commands for the next
steps. Grok Bot and ChatGPT get no token here: each fetches its own later.
Add `--tunnels` and it also opens the two Tailscale tunnels from step 4.

## 3. Start the server

```bash
lucid-dream serve --root ~/lucid-memory --doors grok=7882,chatgpt=7883 --dream-at 03:00 --dream-with codex
```

Leave it running. This one process is the server for your own computer
(port 7878) plus a "door" each for Grok and ChatGPT. It also tidies the
memory every night at 03:00 using Codex (the "dream"); if the computer was
off then, it does it when the server is next running. A door accepts only its
own agent's token, so a leaked Grok address cannot be used as ChatGPT.

To have it start by itself after a restart, add that command to your login
items (macOS), a systemd user service (Linux) or Task Scheduler (Windows).

## 4. Give the two doors a public address

Grok and ChatGPT run in the cloud, so they need an HTTPS address that reaches
your computer. Install Tailscale, sign in, turn on
[Funnel](https://tailscale.com/kb/1223/funnel) for your account, then:

```bash
tailscale funnel --bg --https=8443 7882
```

```bash
tailscale funnel --bg --https=10000 7883
```

Your addresses are now:

- Grok: `https://<computer>.<tailnet>.ts.net:8443/mcp`
- ChatGPT: `https://<computer>.<tailnet>.ts.net:10000/mcp`

Tailscale is what this was tried with, not a requirement. Each door is plain
HTTP on your own computer, so any tunnel that puts a stable HTTPS address in
front of it works the same way (a Cloudflare Tunnel with your own domain, for
example). Avoid tunnels whose address changes on every start: you would have
to re-enter it in Grok and ChatGPT each time.

`tailscale funnel status` shows the exact names. To check one, open
`https://<computer>.<tailnet>.ts.net:8443/health` in your phone's browser on
mobile data; it should say `ok`.

## 5. Connect Grok Bot

With the server running, print a ready-made block and paste all of it into
Grok Bot:

```bash
lucid-dream brief grok --root ~/lucid-memory
```

The block gives Grok Bot its address and a one-time code. Grok Bot exchanges
the code for its own token and adds the connector itself, so you never copy
a token. It should finish by listing `grok (you)`, `chatgpt` and `codex`.

If Grok Bot cannot set up its own connector, do it by hand: run
`lucid-dream token add grok --root ~/lucid-memory`, then ask Grok Bot to add
a remote MCP server at the Grok address from step 4 and enter the token in
the secure form it shows. If the form has no place for a token, use the
address with the token at the end: `.../mcp/<grok token>`.

**Wake Grok when a message arrives.** Ask Grok Bot to create a webhook that
starts it, and to tell you the webhook's address and its `Authorization`
value. On your computer, put that value in `LUCID_WEBHOOK_AUTH` (so it never
sits in your command history), then:

```bash
lucid-dream webhook set grok --url <webhook address> --root ~/lucid-memory
```

```bash
lucid-dream webhook test grok --root ~/lucid-memory
```

The wake-up carries no message text. Grok reads the message through its door.

## 6. Connect ChatGPT (through a Site)

On our Plus account, ChatGPT could not add an outside MCP server directly;
ChatGPT itself reports that OpenAI lists that for Business and
Enterprise/Edu only. What does work on a personal plan is a ChatGPT **Site**
that hosts a small relay: publishing the Site turns the
relay's tools into a personal plugin, and the relay forwards each call to
your ChatGPT door.

Print the brief and paste all of it into ChatGPT, on the web or in the
desktop app:

```bash
lucid-dream brief chatgpt --root ~/lucid-memory
```

ChatGPT builds the Site, and the Site fetches its own token with the
one-time code in the brief. Publish the Site, install the plugin card it
shows, then ask: "use Lucid Dream relay to call `who`". If pairing fails
(the code lasts 60 minutes), run the command again and give ChatGPT the new
code. The by-hand route, with the token typed into the Site's secret form,
is in [clients/chatgpt](../clients/chatgpt/README.md).

Things to know before you start:

- **Plans:** OpenAI lists Sites for Plus, Pro, Business and Enterprise. Free
  is not on that list. This was tried on Plus only.
- **You must build your own.** A personal account cannot share a Site-hosted
  plugin, so nobody can hand you theirs. Any AI can write the relay's code,
  but only your own ChatGPT can publish it as your plugin.
- **Build it on the web or the desktop app.** Those are where OpenAI says
  Sites can be created. Whether the finished plugin works in the phone app is
  not confirmed.
- **It uses some Codex quota.** Building or editing the Site counts toward
  your Codex usage. Running it is counted separately.
- **ChatGPT is not woken.** It reads its messages when you ask it to.

## 7. Try the dream, and connect Codex

The server from step 3 dreams by itself every night. Try one now so you have
seen what it does:

```bash
LUCID_DREAM_CMD=codex lucid-dream dream --root ~/lucid-memory
```

It merges the day's notes into pages, rebuilds the index and commits to the
memory's own git history; `git -C ~/lucid-memory show --stat` shows what it
did, and `git revert` undoes it. It runs on your ChatGPT subscription.

You can also ask any connected agent to do it: "use lucid-dream to tidy the
memory now". And `lucid-dream doctor` says if the last dream failed.

**Codex as a memory client** (optional). Put the `codex` token from step 2 in
the environment variable `LUCID_TOKEN_CODEX`, then:

```bash
lucid-dream setup --codex --url http://127.0.0.1:7878/mcp
```

It adds the server to `~/.codex/config.toml` and the memory rule to
`~/.codex/AGENTS.md`. More in [clients/codex](../clients/codex/README.md).

**A copy somewhere else** (optional, recommended). Add `--backup-to <folder>`
to the `serve` command in step 3, naming a second disk or a folder your cloud
drive syncs. After each dream the whole memory is written there as one file.

## 8. Check it

```bash
lucid-dream doctor --root ~/lucid-memory
```

Then try the bridge: ask ChatGPT to "send a message to grok saying hello
through Lucid Dream relay". Grok Bot should wake and tell you it arrived.

## What works for which agent

| Agent | Memory | Sends | Woken when a message arrives |
| --- | --- | --- | --- |
| Grok Bot | yes | yes | yes, by webhook |
| ChatGPT (Site plugin) | yes | yes | no, reads when asked |
| Codex CLI | yes | yes | no, reads at session start (it has no hook that can wake it) |

Codex is logged automatically if you add its plugin
([plugins/README.md](../plugins/README.md)); otherwise none of these three is. Each is told, in the block you
paste or in its `AGENTS.md`, to log a short summary when a piece of work is
done; whether it does is up to the agent.

## Turning something off

Each agent can be cut off without touching the others:

```bash
lucid-dream token remove grok --root ~/lucid-memory
```

`tailscale funnel reset` closes both public addresses. Deleting the folder
deletes the memory.

## Tried and not tried

Tried: the install script on macOS, `quickstart`, pairing codes (with a
script standing in for the agent), one server with two doors
and the nightly timer,
Grok Bot as a connector with a webhook, the ChatGPT relay Site on a Plus
account, Codex as a memory client and as the dream agent on macOS.

Not tried: Grok Bot or ChatGPT connecting themselves from a pasted block,
this whole guide from scratch on Windows or Linux, the plugin in
the ChatGPT phone app, Grok Bot accepting an address on a port other
than 8443, and the dream on the model `--dream-with codex` now asks for
first (`gpt-6.1-sol`; the same command without a model was tried, and is
what it falls back to).

If a step does not work as written, that is a fault in this guide: please
[open an issue](https://github.com/jksim26/lucid-dream/issues) saying which
step and what you saw.
