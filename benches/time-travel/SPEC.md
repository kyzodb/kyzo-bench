# The as-of benchmark, v1 — a reference workload for time-travel queries

Story: [kyzo#28](https://github.com/kyzodb/kyzo/issues/28). Status: **draft, awaiting fairness
review** (`.claude/rules/methodology.md`: this spec must pass the methodology-fairness review
before any engine — KyzoDB included — publishes a number against it).

No standard benchmark exists for "what did the database believe at time *t*?". This document
defines one, neutrally: nothing in the workload presumes any particular history representation
(validity-in-key, version chains, commit graphs, or hand-rolled validity columns). An engine
qualifies if it can answer point and range reads *as of* an arbitrary past write, with committed
durability, from one process.

## Definitions

- **Global depth `d` of a read**: the number of committed writes to the *whole store* that
  happened after the moment the read asks about. "One year of other people's edits" is one of the
  two costs the benchmark exists to expose.
- **Per-key depth `v` of a read**: the number of versions the *queried key itself* has accumulated
  after the moment the read asks about. This is the other cost, and it stresses a different axis:
  an engine that clusters a key's versions together must skip `v` of them; an engine that chains
  versions must walk `v` links. A workload where every key has the same shallow history would hide
  this axis entirely, so the churn is deliberately skewed (below) to produce both shallow and deep
  keys, and both are queried and published.
- **Overhead curves**: median as-of read latency divided by median current-read latency
  (`d = 0, v = 0`), published twice — against global depth `d` (uniform-key reads) and against
  per-key depth `v` (hot-key reads). A perfect time-travel implementation is flat at 1.0 on both.

## Workload

All randomness derives from one published `u64` seed via SplitMix64 (reference implementation in
`kyzo-bench/harness`). Sizes are fixed by the tier table below.

1. **Load**: `K` keys, each written once with an 8-byte integer key and a 100-byte value. Value
   bytes are drawn from the seeded generator (incompressible); a compressible constant would let
   storage amplification flatter engines whose history format compresses runs.
2. **Churn**: `W` further writes. 90% pick a key uniformly at random; 10% pick uniformly from the
   **hot set**, the first `K / 1000` keys. Hot keys thus accumulate on the order of
   `100 × (W / K)` versions each (~1,000 at the small tier) while typical keys get `~0.9 × W / K`
   (~9), so both shallow and deep per-key histories exist in the same store. Every write is
   individually committed under the engine's documented durable-commit default (the rig records
   the engine's sync configuration verbatim in the result). A **pin** is the store's own
   timestamp/version handle captured immediately after every 1000th churn write commits; pins are
   taken by the rig, never computed by arithmetic on the engine's counters.
3. **Query phases**, each `Q` reads, latencies recorded individually:
   - `current`: read uniformly random keys at "now" (the `d = 0, v = 0` baseline);
   - `asof(p)` for each pin `p`: read uniformly random keys as of pin `p` — the global-depth curve;
   - `asof-hot(p)` for each pin `p`: read uniformly random *hot* keys as of pin `p` — the per-key
     depth curve, up to ~1,000 skippable versions per queried key at the small tier;
   - `range-asof(p)`: scan 100 consecutive keys starting inside the hot set as of pin `p` (ordered
     access over deep history, not just point lookups).
4. **Storage**: bytes on disk after load, and after churn, both taken after the engine's own
   documented compaction/checkpoint operation completes. History is supposed to cost something;
   the benchmark reports how much.

| Tier | K | W | Q |
| --- | --- | --- | --- |
| small | 100 000 | 1 000 000 | 100 000 |
| full | 1 000 000 | 10 000 000 | 1 000 000 |

## Metrics (all published, raw first)

- Median / p95 / p99 latency per phase, wall-clock, single-threaded reads.
- Both overhead curves: `asof(p)` ÷ `current` (global depth) and `asof-hot(p)` ÷ `current`
  (per-key depth), per pin. Ratios are published **with** their absolute numerators and
  denominators beside them — a ratio alone rewards a slow `current` baseline.
- Write throughput during churn (writes are committed individually; group commit allowed if it is
  the engine's default, and the default is quoted in the record).
- Storage amplification: bytes after churn ÷ bytes after load, with both absolute byte counts
  published beside the ratio.

## Comparator rules

- Engines with native as-of (KyzoDB, Dolt, XTDB, SQL:2011 temporal tables) run the workload as
  documented by their vendors, pinned versions, defaults unless their own performance docs say
  otherwise (cited in the rig).
- Engines without native time travel may enter with a **hand-rolled** validity-column schema
  (row + valid-from timestamp, index on (key, valid-from)); such entries are labeled `hand-rolled`
  in every table. The comparison is the point: what native support buys over the workaround.
- Every side runs the same logical history from the same seed, on the same hardware, warm/cold
  declared, under the house caps.

## What this benchmark refuses to do

- No cherry-picked depths: pins are every 1000th write, all of them queried, all published.
- No "time travel" via full restore-from-backup: a qualifying read answers inside a live process
  serving current reads concurrently is *not* required in v1 (single-threaded), but restoring a
  snapshot to different storage is disqualified — the read must be served by the same store files.
- No summary without raw: per-read latencies land in `results/` as raw distributions.

v1 freezes when the fairness review passes; changes after that are v2, and v1 results stay.
