# Recursive Datalog — vs Souffle

Story: [kyzo#22](https://github.com/kyzodb/kyzo/issues/22) · Epic: [kyzo#39](https://github.com/kyzodb/kyzo/issues/39)

Transitive closure, same-generation, and context-insensitive Andersen points-to, against Souffle 2.5
in both interpreted and compiled mode. Souffle compiles to C++; within striking distance of
interpreted mode is the win we claim.

## Method

Workloads are a closed registry in `rig/src/workloads.rs`: three seeded synthetic families
(uniform digraph TC, layered same-generation, Andersen statement mix) plus two **real graphs**
(SNAP wiki-Vote and p2p-Gnutella08, fetched hash-verified by `fetch-datasets.sh`). The story also
names Doop Java facts and Graspan dataflow graphs; Doop needs its JVM toolchain run and Graspan's
datasets sit on unpinnable Google Drive links, so both are tracked as follow-ups on the issue
rather than silently dropped.

Both subjects are external processes under identical caps (12 GiB address space, 1800 s): facts
in as TSV, answer out as TSV, wall clock covers the full pipeline. Souffle gets `-j <nproc>` per
its own docs; compiled mode's `souffle -o` compilation happens **before** the clock. KyzoDB runs
through its one public front door (`Db::run_script`) against a fresh fjall store — it persists
facts durably inside the measured window where Souffle holds them in memory; same window,
different obligations, stated rather than hidden.

Correctness is enforced, not sampled: canonical answers (sorted unique lines, SHA-256) must be
identical across every run of every subject, and `suite` mode refuses to emit numbers if any two
subjects disagree on any workload.

## Standings (this hardware)

Souffle numbers are from the default 32-bit domain build (the first cut used the non-default
64-bit flag; the fairness review caught it, those records were discarded, and everything below
was re-measured — answer hashes were unaffected). KyzoDB numbers are commit `7447589`, clean tree,
landed in `results/`.

The load phase for the numbers below used inlined literal `:put` scripts (a fresh script string
re-parsed per 5,000-row chunk). `kyzo-runner` has since switched to the `$data`-param calling
convention (`?[...] <- $data :put ...`) the OLTP runner's own README already documents as ~2x
end-to-end load throughput over the literal form — the same fix, applied here across
`load_relation`'s arbitrary arity. Answer hashes are unaffected (verified against every landed
record above); wall-clock numbers below predate the change and will shift on the next re-run.

| workload | rows | Souffle 2.5 | Souffle compiled | KyzoDB @7447589 |
|---|---|---|---|---|
| tc/sparse-n2k-m6k | 3.5 M | 0.56 s / 78 MiB | 0.89 s | 9.0 s, 1.74 GiB — answer identical |
| tc/sparse-n10k-m30k | 88.4 M | 13.1 s / 1.6 GiB | 10.7 s | **OOM-killed** at 12 GiB cap (~77 s) |
| sg/layered-l12-w600-p2 | 2.7 M | 0.40 s / 76 MiB | 0.32 s | 7.1 s, 1.76 GiB — answer identical |
| pointsto/v3k-a2k-s6k | 4.1 M | 16.6 s / 108 MiB | 7.5 s | **OOM-killed** at 12 GiB cap (~27-38 s) — [kyzo#68](https://github.com/kyzodb/kyzo/issues/68), reopened: a prior fix's closing comment claimed this completes at 1.93 GiB on this exact commit; independent re-measurement contradicts it |
| tc/snap-wiki-Vote (real) | 11.9 M | 2.25 s / 275 MiB | 1.76 s | 82.1 s, 6.51 GiB — answer identical |
| tc/snap-p2p-Gnutella08 (real) | 13.1 M | 1.89 s / 237 MiB | 1.53 s | 42.3 s, 6.09 GiB — answer identical |

tc/snap-p2p-Gnutella31 was tried and refused for everyone: its closure blows the 12 GiB cap
(Souffle included). Souffle baselines are landed in `results/`. DDlog is not yet rigged: it is
unmaintained upstream and needs a Haskell toolchain; if it joins, it joins pinned like everything
else.

## Run it

    ./opponents/souffle/build.sh
    ./benches/datalog/fetch-datasets.sh
    cargo build --release -p datalog-rig -p kyzo-runner
    ./target/release/datalog-rig suite --runs 5 [--land]
