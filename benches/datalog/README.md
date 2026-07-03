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

## Standings (this hardware; engine numbers are dev-tree indications until landed)

Souffle numbers are from the default 32-bit domain build (the first cut used the non-default
64-bit flag; the fairness review caught it, those records were discarded, and everything below
was re-measured — answer hashes were unaffected).

| workload | rows | Souffle 2.5 | Souffle compiled | KyzoDB @86b2f69 (dirty) |
|---|---|---|---|---|
| tc/sparse-n2k-m6k | 3.5 M | 0.56 s / 78 MiB | 0.89 s | 13.7 s, 2.8 GiB — answer identical |
| tc/sparse-n10k-m30k | 88.4 M | 13.1 s / 1.6 GiB | 10.7 s | not yet run |
| sg/layered-l12-w600-p2 | 2.7 M | 0.40 s / 76 MiB | 0.32 s | 9.7 s, 2.7 GiB — answer identical |
| pointsto/v3k-a2k-s6k | 4.1 M | 16.6 s / 108 MiB | 7.5 s | **OOM-killed** at 12 GiB cap (~27 s) |
| tc/snap-wiki-Vote (real) | 11.9 M | 2.25 s / 275 MiB | 1.76 s | 165.9 s, 9.8 GiB — answer identical |
| tc/snap-p2p-Gnutella08 (real) | 13.1 M | 1.89 s / 237 MiB | 1.53 s | **OOM-killed** at 12 GiB cap (~74 s) |

tc/snap-p2p-Gnutella31 was tried and refused for everyone: its closure blows the 12 GiB cap
(Souffle included). Souffle baselines are landed in `results/`; KyzoDB numbers land when they can
name a clean engine commit. DDlog is not yet rigged: it is unmaintained upstream and needs a
Haskell toolchain; if it joins, it joins pinned like everything else.

## Run it

    ./opponents/souffle/build.sh
    ./benches/datalog/fetch-datasets.sh
    cargo build --release -p datalog-rig -p kyzo-runner
    ./target/release/datalog-rig suite --runs 5 [--land]
