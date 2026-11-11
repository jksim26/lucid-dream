# Claude app (phone, web, desktop chat)

The Claude apps call a connector from Anthropic's cloud, not from your phone, so
Tailscale alone cannot reach the PC. The app gets its own door: a second
listener on the PC that accepts one token and nothing else, with a tunnel in
front of it. Your main server stays private to your Tailscale network.

All of this happens on the PC that holds the memory.

1. Issue a token only the app will use. Claude in the app is still Claude, so
   its notes carry Claude's voice, but this token can be revoked on its own.

   ```bash
   lucid-dream token add claude --label phone --root <memory dir>
   ```

2. Start the door on a port of its own. Keep it bound to 127.0.0.1.

   ```bash
   lucid-dream serve --root <memory dir> --bind 127.0.0.1:7881 --only claude/phone
   ```

3. Put a tunnel in front of that port. With Tailscale Funnel:

   ```bash
   tailscale funnel --bg 7881
   ```

   The first run asks you to approve Funnel for your tailnet. The door is then
   at `https://<pc-name>.<tailnet>.ts.net/mcp`.

4. On claude.ai (web or desktop; connectors added there also appear on the
   phone), add a custom connector with that URL. Under request headers, add
   `Authorization` with the value `Bearer <the phone token>`.

   If your account does not offer request headers, choose "No sign in" and put
   the token at the end of the URL instead:
   `https://<pc-name>.<tailnet>.ts.net/mcp/<the phone token>`. The URL is then
   the secret; treat it like a password.

5. The app has no hooks, so tell it when to use memory. In Settings, add to your
   preferences:

   > At the start of a conversation, call the lucid-dream `index` and `messages`
   > tools. When I decide or state something worth keeping, call `remember`
   > with said_by "user".

## Pulling in what the app already knows

Once connected, ask Claude in the app to search your past chats and its own
memory and `remember` what matters. The app does the recalling; the notes land
in the inbox and dream merges them like any others.

## Closing the door

Any one of these cuts the app off without touching your other devices or Grok:

```bash
lucid-dream token remove claude --label phone --root <memory dir>
```

```bash
tailscale funnel --bg 7881 off
```

Or stop the door process. Revoking the token takes effect on the next request.

## What the door exposes

Through the tunnel a caller can reach only this one process, and without the
phone token it gets a 401. With the token it can read the memory store and add
notes, logs and messages; it cannot edit or delete anything, and it cannot
reach other files on the PC. A device token that leaks is useless here: the
door accepts only `claude/phone`.
