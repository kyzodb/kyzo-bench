# Full-text search — vs Tantivy standalone and SQLite FTS5

Story: [kyzo#27](https://github.com/kyzodb/kyzo/issues/27) · Epic: [kyzo#39](https://github.com/kyzodb/kyzo/issues/39)

Tantivy alone will likely win standalone and we publish that; the win we claim is FTS composed
inside joins, negation, and recursion, which a standalone search library cannot enter.

## Method

Corpus: 40 pinned Project Gutenberg books (`fetch-corpus.sh`, hash-recorded), split
deterministically into ~41k paragraph documents. Queries: 120 per seed (40 term / 30 AND /
30 OR / 20 phrase), drawn from the corpus's own mid-document-frequency vocabulary, restricted
to pure ASCII `[a-z]{4,15}` tokens so Tantivy's `default` tokenizer and FTS5's `unicode61`
provably agree on tokenization — a tokenizer quirk must not masquerade as a correctness result.

Two externally timed phases per subject under the house caps (12 GiB / 1800 s): **index**
(docs.tsv → persistent index; Tantivy force-merges to one segment as its own benchmark does,
inside the clock) and **query** (20 timing passes + 1 verified pass, single-threaded).
Match sets for term/AND/OR/phrase are engine-independent facts and must be byte-identical
across subjects — `suite` refuses to emit numbers otherwise. Ranked BM25 top-10 is each
engine's own and is recorded, never cross-compared.

The rig caught its own defect here before it could ship one: SQLite's `.mode tabs` import
CSV-quote-processes lines starting with `"`, silently merging documents; the agreement gate
flagged the mismatch and the import now uses raw `.mode ascii`.

## Standings (this hardware)

| subject | queries/s (median) | index build | peak RSS | match sets |
|---|---|---|---|---|
| Tantivy 0.26.1 | 25,400 | 0.72 s | 17 MiB | `34f1f8ef…` |
| SQLite FTS5 3.53.3 | 3,300 | 0.67 s | 9 MiB | `34f1f8ef…` — identical |

Records in `results/`. The KyzoDB side gates on the engine's FTS operator (landed in the
engine tree, mid-integration); it enters the same rig, same corpus, same query set, same
agreement gate — plus the composed queries (FTS inside joins/negation/recursion) that the
standalone opponents cannot run, which will be published as KyzoDB-only with the workload
spec open for any engine to enter.

## Run it

    ./opponents/sqlite/build.sh
    ./benches/fts/fetch-corpus.sh
    cargo build --release -p fts-rig -p tantivy-runner
    ./target/release/fts-rig suite --runs 5 [--land]
