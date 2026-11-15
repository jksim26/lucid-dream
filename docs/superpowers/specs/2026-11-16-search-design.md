# Search that grows with memory: keyword index, then meaning

Date: 2026-11-16. Status: draft for JK's approval.

## Why

JK wants the vault built "the right way, like RAG" because memory will keep
growing. lucid-dream is already retrieval-augmented: `index` hands every
session MEMORY.md and `search` finds pages. What does not scale is how
`search` finds them:

- It reads every Markdown file on every call (`Store::search`,
  `src/store.rs:558`). Fine at today's size, slow later.
- A query splits only on commas, so "Kronos Chronos model C2 candle" is one
  exact phrase and finds nothing, though "Kronos" alone has 7 hits
  (Grok's miss, 2026-11-15).
- Ranking is "how many terms a file contains", so a page that names a word
  once ranks the same as the page about it.
- It matches words, not meaning. Dream's `Also called:` lines cover part of
  that, by hand.

## Size today (measured 2026-11-16 with `list`)

| Layer | Files | Size |
| --- | --- | --- |
| Working memory (MEMORY.md, notes, wiki, digest, open-questions) | about 50 | about 0.4 MB |
| Deep: archive | about 300 | about 0.6 MB |
| Deep: logs (summaries and full records) | about 170 | about 12 MB, almost all in `.record.md` files |
| Deep: messages | about 80 | under 0.1 MB |

Records add roughly 2 to 3 MB a day. JK's rule is "keep it unbounded and log
everything" (2026-11-12), so the deep layer is where size will hurt first.

## Goals

- Search stays fast and finds the right page as memory grows to tens of
  thousands of files.
- A query finds pages that use different words for the same thing.
- Exact terms still win: PR numbers, paths, session ids, names.
- Agents change nothing: same `search` tool, same arguments, same
  `path:line: text` output.

## Non-goals

- No change to how memory is written, dreamed or stored.
- No answer generation inside the server. Agents read what search returns and
  answer themselves.
- No knowledge graph or entity extraction.

## Anti-goals (would make this a failure even if it works)

- Memory moving into a database. Markdown in git stays the only real copy.
- Breaking the two-layer rule or its gate: working memory first, deep only
  when it is empty or a reason is given, and the reason logged.
- Session records leaking through search to doors that are sealed from them.
- A second program JK has to install, start or keep running on sim.
- Results no one can explain. Every result must still show the line that
  matched.

## Constraints

- Rust, one binary, same as today. Builds on Windows (sim), macOS and Linux.
- The server runs on sim. Agents on the Mac and phone reach it over MCP.
- Dream runs as an agent CLI and changes files directly, not through the
  server, so the index must notice changes it did not make.
- Memory stays usable when the index is missing, broken or being rebuilt.

## Approaches considered

1. **Keep scanning files, add meaning on top.** Cheapest now, but every search
   still reads all of memory, which is the part that will not scale.
2. **Put the memory in a vector database (Qdrant, LanceDB, a hosted one).**
   What most RAG tutorials show. It breaks the anti-goals: the database
   becomes the memory, a second service runs on sim, and results stop being
   explainable. Rejected.
3. **Chosen: a rebuildable index beside the Markdown, in stages.** First a
   proper keyword index with relevance ranking, then local embeddings as a
   second index, merged at search time. The index lives in `.lucid/index/`
   (already ignored by git in the memory folder), and deleting it loses
   nothing. JK chose this direction on 2026-11-16.

## Design

### Stage 1: split better (about 1 hour)

- Split the query on spaces as well as commas. Text in double quotes stays
  one exact phrase ("pull request 541").
- A line matches if it has any of the words, as with commas today, and files
  that match more of the words come first. (My call: requiring every word
  would have missed Grok's query too, since no line holds all five.)
- **The gate (JK decided, 2026-11-16).** Today any working-memory hit holds
  the deep layer back. Once common words like "model" count on their own,
  working memory would nearly always have a hit, so deep search would almost
  never open without a reason. Decided: working memory counts as having
  found something only when one file matches at least half of the words
  (rounded up). With a `reason`, deep search opens as today.
- Change the "no matches" hint to match.
- Ship on its own as a patch release. It fixes the class of miss Grok hit.

### Stage 2: keyword index with ranking (about 2 days)

- Use the `tantivy` crate (Rust full-text search, BM25 ranking).
- **Pieces, not files.** Cut each file at Markdown headings, then at about
  300 words, keeping each piece's first line number. A hit is reported as
  the best-matching line inside the best pieces, so output stays
  `path:line: text`.
- **Each piece carries its layer** (working or deep) and whether it is a
  session record. The search applies the gate and the seal as filters before
  ranking, using the same rules as today (`is_deep`, `is_record`,
  `mcp.rs:408`).
- **Staying fresh.** The index keeps each file's size and modified time. Before
  a search, the server looks for files that changed since the last check (a
  walk of names and times, no reading) and re-indexes only those. Writes made
  through the server (`remember`, `log`, `record`) re-index straight away.
  Dream's changes are picked up the same way on the next search.
- **Safety.** If the index is missing or fails to open, search falls back to
  today's scan and says so once in the server log. `lucid-dream reindex`
  rebuilds it from scratch. `doctor` reports its state.
- **Word forms.** English stemming, so "trade" finds "trading". Exact terms
  still match exactly: each piece is indexed twice, once stemmed and once as
  raw tokens that keep paths, ids and `#541` whole, and raw matches score
  higher. Quoted phrases search only the raw field. (My call.)
- **Output and `limit`.** Unchanged in meaning: `limit` counts result lines,
  one line per matching piece (the best line in it), and a file may appear
  more than once. Order is by score instead of by count of words. (My call.)
- **One writer.** Several processes can open the memory folder at once (the
  HTTP server, `serve --stdio`, `reindex`). The first takes a lock file in
  `.lucid/index/` and is the only one that writes; the others open the index
  read-only and see its updates. One that cannot open it falls back to the
  scan. (My call.)

### Stage 3: meaning search, hybrid (about 4 to 5 days)

- Use the `fastembed` crate (ONNX, runs inside the binary) with a small
  English model, `bge-small-en-v1.5` (384 numbers per piece, about 130 MB,
  downloaded once on first use into `.lucid/models/`).
- Embed the same pieces as stage 2. Store the vectors in a flat file in
  `.lucid/index/`, searched by brute force. At 100,000 pieces that is about
  150 MB and well under a second (an estimate). No vector database.
- **What gets embedded at first:** working memory, the archive, messages and
  log summaries. Full session records stay keyword-only until stage 2 shows
  how slow embedding them is on sim. This is my call; the records are about
  95 percent of the text and mostly tool output.
- **Merging.** Run both indexes and combine their rankings with reciprocal
  rank fusion: a piece ranked high in either list rises. Exact terms still
  surface through the keyword side.
- **Switch.** Off until JK turns it on (`serve --meaning` or a setting), so a
  machine without the model still works. The `search` tool says nothing new;
  results just get better.
- Embedding runs in the background after changes, so a search never waits on
  it; until a piece is embedded, it is found by keywords only.

### Measuring it (part of each stage, about half a day once)

- A test set of 30 to 50 real questions with the page that should come back,
  taken from past searches in the logs (`deep-searches.log`, readable with
  the lucid-dream `read` tool from any machine) and misses like Grok's. Stored in the repo as a test file.
- For each stage, report how many questions find the right page in the top 5,
  before and after. Stage 3 ships only if it beats stage 2 on this set.

## Time estimate

| Stage | Build (Claude) | JK's part |
| --- | --- | --- |
| 1. Split on spaces | about 1 hour | none, auto release |
| 2. Keyword index | about 2 days | none |
| Test set | about half a day | glance at the questions, 10 minutes |
| 3. Meaning search | about 4 to 5 days | say when to turn it on |
| **Total** | **about 1.5 weeks of work** | |

These are estimates for an agent building it with tests, in working days, not
calendar time. Stage 3 carries the most risk (ONNX on Windows, model
download, embedding speed on sim), so it may run over.

## When to do stage 3

Stage 1 now, and stage 2 soon after, since logs are growing fast. Stage 3
when either the test set shows misses that only meaning would fix, or
working memory passes about 2,000 files. My estimate.

## Left to the builder

- Exact piece size and overlap, and the tantivy schema.
- How the merged score is shown, if at all.
- Where the background embedding queue lives in the server.
- The file format of the vector store.
