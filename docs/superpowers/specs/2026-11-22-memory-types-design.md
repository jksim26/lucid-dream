# Memory types: adding the procedural layer

Date: 2026-11-22. Asked for by the user ("can u plan it and build it now?") after the
2026-11-18 conversation in which the user proposed one lobe per memory type (working, episodic,
semantic, procedural) and agreed that lesson-learnt memory is missing.

## Goal

Every agent starts each session with the short list of lessons the user's agents have
learned the hard way, and the dream keeps that list honest and small.

## Where each memory type already lives

| Type | What it is | Where it lives | Status |
| --- | --- | --- | --- |
| Working | the live session | the agent's own context | exists |
| Episodic | what happened | `logs/`, `messages/`, `archive/` (the deep layer) | exists |
| Semantic | facts and decisions | `notes/`, `wiki/`, `MEMORY.md` | exists |
| Procedural | how to work: lessons learnt | nothing | **missing** |

Three of the four lobes already exist as folders. The smallest change that meets the goal
adds the fourth, rather than reorganising the store into lobe folders.

## Decision: one dream with ordered steps, not one dream per lobe

| Option | For | Against |
| --- | --- | --- |
| One dream per lobe | each lobe gets a focused prompt | several agent runs a night, each re-reading the same inbox; lobes would disagree about the same note; more cost and lock juggling |
| **One dream, an ordered lessons step** | one run, one commit, one inbox pass; the dream sees an incident and the lesson together | one longer prompt |

Chosen: one dream. The dream prompt already runs ordered steps; lessons become one of them.

## Shape

### `lessons.md` at the store root

```markdown
# Lessons

Rules every agent follows, learned from things that went wrong. Dream keeps this file.
At most 20 rules.

## Rules

- Never point a door at a new token label before the agent has redeemed its code. Why: Grok was locked out. Incidents: [2026-11-19](archive/inbox/...), [2026-11-18](logs/...). (rule since 2026-11-22)

## On probation

- <candidate rule>. Incidents: [2026-11-22](archive/inbox/...). (first seen 2026-11-22)
```

- One line per rule: the rule, `Why:`, cited incidents, and a date.
- Only lessons that apply across projects. A lesson about one project goes on that
  project's wiki page.
- Root of the store, so `search` already covers it as working memory.

### Promotion rules (dream)

- A candidate becomes a rule when it has **2 or more cited incidents** and has been on
  probation **at least 14 days**.
- A lesson the user stated as a rule themself (`said_by: user`) skips probation.
- At most **20 rules**. To add one when full, the dream retires the weakest (the least
  recently relevant) to `archive/lessons.md`, unchanged, with the date it left.
- A candidate with no new incident for 60 days is retired the same way.
- A lesson that code could enforce (a check, a guard, a test) is named in the digest under
  what the user should look at, so it can become a fix instead of a rule.

### `remember` gets `kind`

`kind`: `"fact"` (default) or `"lesson"`. A lesson note is written with `kind: lesson` in
its frontmatter. Anything else is refused. The dream reads `kind: lesson` notes as lesson
candidates; it also finds incidents in logs on its own.

### Loading

- `index` returns `MEMORY.md`, then the `## Rules` section of `lessons.md` (not the
  probation list), then the project page. Lessons are procedural: they work only if they are
  in front of the agent, so they are loaded, not searched for. Shown rules are cut at 3,000
  characters.
- A missing `lessons.md` or an empty Rules section adds nothing.

### Guard after the dream

After committing, the dream warns when `## Rules` holds more than 20 rules or more than
3,000 characters, as it already warns about an oversized `MEMORY.md`.

### New stores

`lucid-dream init` creates `lessons.md` with empty sections. The dream prompt tells the
dream to create it when missing, so existing stores get it on the next dream.

## Out of scope

- Separate lobe folders or dreams.
- Meaning search, rewriting pages to current state, a "fresh" layer: separate proposals.
- Turning lessons into crew issues automatically: the digest names them; a person or
  agent files them.

## Tests

- `remember` with `kind: "lesson"` writes `kind: lesson`; with no kind writes none; with
  another value is refused.
- `index` includes the Rules section and leaves out probation; a missing file changes nothing.
- The shown rules are cut at the limit.
- The rule counter counts `- ` lines under `## Rules` only.
- `init` creates `lessons.md` and leaves an existing one alone.
