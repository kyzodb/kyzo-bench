# Whole-graph algorithms — LDBC Graphalytics vs Kuzu

Story: [kyzo#23](https://github.com/kyzodb/kyzo/issues/23) · Epic: [kyzo#39](https://github.com/kyzodb/kyzo/issues/39)

PageRank and WCC (BFS/CDLP/SSSP to follow) on LDBC Graphalytics datasets. The honest head-to-head
is Kuzu, the other embedded graph engine.

## Method

`fetch-datasets.sh` pulls the official Graphalytics datasets (wiki-Talk 2.39 M vertices,
cit-Patents 3.77 M vertices) with SHA-256 verification, including LDBC's reference outputs.
`kuzu_baseline.py` loads the graph into Kuzu (pinned pip package), runs each algorithm 5 times,
and checks against the reference. Timing is processing-only per Graphalytics practice: the CALL is
forced to completion inside the engine by aggregating over every output row; client-side row
marshaling stays outside the clock (the first cut timed a Python row loop inside the window —
the fairness review measured ~80% inflation and those records were discarded). Correctness:

- **WCC**: verified as a partition (component ids are arbitrary; the partition must match LDBC's
  reference exactly). It does, on both datasets.
- **PageRank**: Kuzu's implementation does **not** redistribute dangling-node rank as the
  Graphalytics spec requires (rank sum drifts to 0.17 instead of 1.0), so its output cannot be
  spec-verified. The record labels it Kuzu's own PageRank variant, timed as shipped.

Memory is capped through Kuzu's own `buffer_pool_size` (12 GiB) rather than `RLIMIT_AS`, because
Kuzu by design mmaps 8 TiB of virtual address space; the cap difference is declared in the record.

## Standings (this hardware)

| dataset | load | PageRank (median of 5) | WCC (median of 5) | correctness |
|---|---|---|---|---|
| wiki-Talk | 0.57 s | 1.17 s | 0.21 s | WCC partition = LDBC reference |
| cit-Patents | 1.13 s | 2.00 s | 0.78 s | WCC partition = LDBC reference |

Records are landed in `results/`. The KyzoDB side gates on the engine's fixed-rule algorithms
(PageRank/WCC operators) being reachable through `run_script`; wired the same way as every other
rig — external process, same datasets, same reference check.

## Run it

    ./benches/graph-algorithms/fetch-datasets.sh
    python -m venv benches/graph-algorithms/.venv && benches/graph-algorithms/.venv/bin/python -m pip install -r benches/graph-algorithms/requirements.txt
    benches/graph-algorithms/.venv/bin/python benches/graph-algorithms/kuzu_baseline.py --dataset wiki-Talk --runs 5
