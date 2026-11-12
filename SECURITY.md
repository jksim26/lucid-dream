# Security

lucid-dream holds your notes and lets agents write to them, so please report
anything that lets a caller read or write without the right token, pose as
another agent, or reach files outside the memory store.

## Reporting

Open a private report at
https://github.com/jksim26/lucid-dream/security/advisories/new. Please do not
open a public issue for a vulnerability.

## What the design relies on

- **Tokens.** Each agent has its own token; only its SHA-256 hash is stored, in
  `.lucid/tokens`, outside git. The token decides who is speaking, so an agent
  cannot send or record as another.
- **Doors.** A listener started with `--only agent/label` accepts that one
  token and nothing else. Put a tunnel in front of a door, never in front of
  the main listener.
- **Reads and writes.** Reads refuse absolute paths, `..`, hidden files and
  symlinks that leave the store. Writes only add files to `inbox/`, `logs/`
  and `messages/`.
- **Messages are not instructions.** A message from another agent is handed
  over labelled as a colleague's note. Whether to act on it is the receiving
  agent's decision under its own rules.
- **Secrets.** Session logs are scrubbed of credential-shaped text before they
  are sent. A webhook's `Authorization` value is read from an environment
  variable and stored outside git.

- **Pairing.** `/pair` is the one thing a caller without a token may use: it
  exchanges a one-time code for the token of the agent the code was made
  for. Codes are 75 bits, stored as hashes, expire after 60 minutes, work
  once, and at a door only for that door's agent. Five wrong guesses cancel
  every waiting code, and a refusal never says why. A code is made only by
  someone with file access to the store.
- **Dreams on request.** Any agent with a token can start a dream through
  the `dream` tool, at most once every 30 minutes and one at a time. A dream
  rewrites pages but only inside the store, and each is one git commit.
- **Trusted messengers.** `trust add <agent>` is the user's own decision to
  let that agent's requests count as theirs for routine actions. It can only
  be set with file access to the store, and the mark is made by the server.

## Known limits

- Anyone with file access to the memory store can read and change it without
  the server. Protect the folder as you would the notes themselves.
- The main listener speaks plain HTTP. Keep it on a private network such as
  Tailscale; use a tunnel that adds TLS for anything public.
- The dream job runs an agent with edit rights inside the store. Review its
  commits; `git revert` undoes one.
