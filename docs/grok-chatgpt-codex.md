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

It creates the folder, prints one token per agent, and prints the commands
for the next two steps. Each token is shown once: copy the three of them
somewhere safe for the next few minutes. A token is that agent's password;
never post one.

## 3. Start the server

```bash
lucid-dream serve --root ~/lucid-memory --doors grok=7882,chatgpt=7883 --dream-at 03:00 --dream-with "codex exec --sandbox workspace-write"
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

Ask Grok Bot, in chat, to add a remote MCP server (a connector):

- **URL:** the Grok address from step 4
- **Token:** the `grok` token, entered in the secure form Grok Bot shows

If its form has no place for a token, use the address with the token at the
end instead: `https://<computer>.<tailnet>.ts.net:8443/mcp/<grok token>`.

Then ask it: "use the lucid-dream tools to call `who`". It should list
`grok (you)`, `chatgpt` and `codex`.

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

Follow [clients/chatgpt](../clients/chatgpt/README.md) from step 2: paste the
brief into ChatGPT, enter the `chatgpt` token as the Site's secret, publish,
then install the plugin card it shows. Then ask: "use Lucid Dream relay to
call `who`".

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
LUCID_DREAM_CMD="codex exec --sandbox workspace-write" lucid-dream dream --root ~/lucid-memory
```

It merges the day's notes into pages, rebuilds the index and commits to the
memory's own git history; `git -C ~/lucid-memory show --stat` shows what it
did, and `git revert` undoes it. It runs on your ChatGPT subscription.

You can also ask any connected agent to do it: "use lucid-dream to tidy the
memory now". And `lucid-dream doctor` says if the last dream failed.

**Codex as a memory client** (optional). Follow
[clients/codex](../clients/codex/README.md): three lines in
`~/.codex/config.toml`, the `codex` token in an environment variable, and a
rule in `AGENTS.md`.

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
| Codex CLI | yes | yes | no, reads at session start |

## Turning something off

Each agent can be cut off without touching the others:

```bash
lucid-dream token remove grok --root ~/lucid-memory
```

`tailscale funnel reset` closes both public addresses. Deleting the folder
deletes the memory.

## Tried and not tried

Tried: the install script on macOS, `quickstart`, one server with two doors
and the nightly timer,
Grok Bot as a connector with a webhook, the ChatGPT relay Site on a Plus
account, Codex as a memory client and as the dream agent on macOS.

Not tried: this whole guide from scratch on Windows or Linux, the plugin in
the ChatGPT phone app, and Grok Bot accepting an address on a port other
than 8443.
