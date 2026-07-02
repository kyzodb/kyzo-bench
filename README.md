# kyzo-bench

The public proving ground for [KyzoDB](https://github.com/kyzodb/kyzo): comparative benchmarks and
reproducible demos, run in the open against the strongest comparable systems.

> [!WARNING]
> **Under construction.** The engine is mid re-architecture
> ([board](https://github.com/orgs/kyzodb/projects/1)). Rigs, datasets, and opponent baselines are
> being built now; headline KyzoDB numbers land as the engine seals. Nothing here is a published
> claim until it appears in `results/` with a seed, a hardware spec, and a reproduction script.

## The charter

Benchmarks are only worth what their methodology is worth. Every comparison in this repo holds to
five rules:

1. **Losing runs are published.** We enter fights we expect to lose (raw unfiltered ANN throughput
   against FAISS, standalone full-text against Tantivy) and commit those results next to the wins.
2. **Opponents get their best game.** Exact pinned versions, configured per their own documentation,
   tuned in good faith. If that project's maintainers would object to the configuration, the run
   does not count.
3. **Everything is reproducible from a clean clone.** Scripted dataset fetch, published seeds,
   recorded hardware. If you cannot rerun it, we should not have published it.
4. **Raw results are append-only.** A committed result is never edited. Corrections supersede; they
   do not replace.
5. **The numbers speak for themselves.** No adjectives where a curve will do.

## What is measured here

| Bench | Against | Story |
| --- | --- | --- |
| `benches/datalog` | Souffle (compiled), DDlog | [kyzo#22](https://github.com/kyzodb/kyzo/issues/22) |
| `benches/graph-algorithms` | Kuzu (LDBC Graphalytics) | [kyzo#23](https://github.com/kyzodb/kyzo/issues/23) |
| `benches/snb` | Kuzu, Neo4j (LDBC SNB Interactive, single node) | [kyzo#24](https://github.com/kyzodb/kyzo/issues/24) |
| `benches/vector` | hnswlib, FAISS, embedded vector engines; big-ann filtered track | [kyzo#25](https://github.com/kyzodb/kyzo/issues/25) |
| `benches/oltp` | SQLite (mixed read/write) | [kyzo#26](https://github.com/kyzodb/kyzo/issues/26) |
| `benches/fts` | Tantivy standalone, SQLite FTS5; FTS composed inside joins | [kyzo#27](https://github.com/kyzodb/kyzo/issues/27) |
| `benches/time-travel` | Dolt, XTDB where shapes fit; we publish the reference benchmark | [kyzo#28](https://github.com/kyzodb/kyzo/issues/28) |

And four demos, each compressing one architectural truth into under a minute, each reproducible from
its directory: the consistency kill shot (`demos/consistency-kill-shot`), the Raspberry Pi replay
(`demos/raspberry-pi-replay`), the browser flex (`demos/browser-flex`), and ask-it-why
(`demos/ask-it-why`).

KyzoDB's self-referential trials (the determinism campaign, crash matrix, fuzzing ledger, and proof
audit) are not here: they are tests and scheduled campaigns in the
[engine repo](https://github.com/kyzodb/kyzo), where they run against every commit.

## Running

Each bench and demo directory is self-contained, with its own README covering opponents, datasets,
and invocation. Datasets are never committed; each directory ships a fetch script into `datasets/`.
Raw outputs land in `results/` with the run's seed, opponent versions, and hardware spec.

## License

Apache-2.0. The engine itself is MPL-2.0 in [kyzodb/kyzo](https://github.com/kyzodb/kyzo).
