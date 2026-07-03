# Embedded OLTP — vs SQLite

Story: [kyzo#26](https://github.com/kyzodb/kyzo/issues/26) · Epic: [kyzo#39](https://github.com/kyzodb/kyzo/issues/39)

SQLite-methodology mixed read/write/update workloads (speedtest1-style). The goal is not to beat
SQLite at being SQLite; it is to quantify the premium the multi-model engine pays for what it adds,
and show it is acceptable.

## Method

One deterministic op stream per workload (seeded SplitMix64): a bulk-load prefix and a mixed
suffix of 60% point reads / 20% updates / 10% inserts / 10% deletes. The rig renders the same
stream into each subject's language and replays it in three externally timed phases against a
persistent on-disk database:

- **load** — 1000-row transactions (SQLite) / 1000-row `:put` scripts (KyzoDB)
- **mixed** — one statement/script per op, autocommit; this is the headline metric
- **dump** — full table ordered by key, a correctness phase

Every read prints its op index with its result, so the cross-subject comparison verifies each
individual read, not just the final state: the concatenated read output plus the final dump must
be **byte-identical** across subjects or the rig refuses the comparison. Both subjects run under
identical caps (12 GiB address space, 1800 s) as external processes, one connection, one thread.
SQLite runs WAL + `synchronous=NORMAL` (declared production config), built from the
hash-verified 3.53.3 amalgamation by `opponents/sqlite/build.sh`.

## Standings (this hardware; engine numbers are dev-tree indications until landed)

| workload | subject | mixed ops/s | load rows/s | peak RSS | answer |
|---|---|---|---|---|---|
| r100k-o20k | SQLite 3.53.3 | 88,000 | 1.32 M | 8 MiB | `e1fbefaa…` |
| r100k-o20k | KyzoDB @175b92a (dirty) | 34,900 | 179 k | 34 MiB | `e1fbefaa…` — identical |
| r1m-o100k | SQLite 3.53.3 | 73,700 | 1.96 M | 9 MiB | `3bca7c63…` |
| r1m-o100k | KyzoDB @175b92a (dirty) | 26,000 | 212 k | 220 MiB | `3bca7c63…` — identical |

The premium today: ~2.5–3x on mixed ops, ~7–9x on bulk load, with every one of the 100k answers
byte-identical. SQLite baselines are landed in `results/`; KyzoDB numbers land when they can name
a clean engine commit.

## Run it

    ./opponents/sqlite/build.sh
    cargo build --release -p oltp-rig -p kyzo-oltp-runner
    ./target/release/oltp-rig suite --runs 5 [--land]
