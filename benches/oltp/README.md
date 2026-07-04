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

## Standings (this hardware)

KyzoDB numbers are commit `7447589`, clean tree, landed in `results/`. The load phase there uses
the `$data`-param calling convention (`?[id,grp,val] <- $data :put …`) rather than an inlined
literal list — the fair comparison against SQLite's own prepared-statement idiom, and ~2x the
end-to-end load throughput of the literal form it replaced.

| workload | subject | mixed ops/s | load rows/s | peak RSS | answer |
|---|---|---|---|---|---|
| r100k-o20k | SQLite 3.53.3 | 48,757 | 665,194 | 8.4 MiB | `e1fbefaa…` |
| r100k-o20k | KyzoDB @7447589 | 53 | 272,509 | 190.7 MiB | `e1fbefaa…` — identical |
| r1m-o100k | SQLite 3.53.3 | 36,648 | 866,310 | 52.7 MiB | `3bca7c63…` |
| r1m-o100k | KyzoDB @7447589 | — | — | — | **times out**: mixed phase does not complete inside the 1800 s house cap, even on the warm-up iteration |

Load is now within striking distance (2.4x, was 7-9x before the `$data` fix). Mixed ops are not:
~920x slower at r100k-o20k and non-terminating at r1m-o100k. A prior bench-side report (closed as
fixed alongside the load-path work) had measured the mixed-op premium at only ~2.5-3x on an earlier
commit — this reads as a regression, not a remeasurement, and is filed as
[kyzo#82](https://github.com/kyzodb/kyzo/issues/82). SQLite baselines are landed in `results/`.

## Run it

    ./opponents/sqlite/build.sh
    cargo build --release -p oltp-rig -p kyzo-oltp-runner
    ./target/release/oltp-rig suite --runs 5 [--land]
