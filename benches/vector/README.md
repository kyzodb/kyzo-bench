# Vector search — ann-benchmarks SIFT1M ([kyzo#25](https://github.com/kyzodb/kyzo/issues/25))

Recall@10 vs single-threaded QPS on the standard ann-benchmarks dataset
`sift-128-euclidean` (1,000,000 base vectors, 128-dim, 10,000 queries, exact
ground truth shipped in the file, hash-pinned by `fetch-datasets.sh`).

## Method

- **ann-benchmarks practice**: recall@10 against the dataset's exact ground
  truth; the recall/QPS curve swept over `efSearch` for HNSW subjects.
- **Single-threaded queries for everyone** (thread knobs pinned to 1), so the
  curve measures the algorithm, not the host's core count. Index build may
  use each library's default threading; build time is reported separately,
  outside the query clock, with the threading declared per record.
- Each sweep point runs the full 10k-query set 3 times; QPS is the median
  pass. Memory capped at 12 GiB (RLIMIT_AS for the Python subjects).
- **Class honesty**: hnswlib and FAISS are embedded libraries with no
  durability, no transactions, and no filters — the raw-throughput ceiling.
  KyzoDB enters as a persistent database whose vectors live in relations
  (`::hnsw create`, `~item:emb{...}` search through the one public
  `run_script` door). We expect to lose raw unfiltered throughput and
  publish it; the filtered track (search as a join) is the comparison this
  bench exists to reach, and lands as a second workload.

Subjects, pinned:

| subject | provenance | config |
|---|---|---|
| hnswlib 0.8.0 | pip | HNSW, M=16, efC=200 (ann-benchmarks standard) |
| faiss-cpu 1.14.3 | pip | IndexHNSWFlat, M=16, efC=200 |
| faiss-cpu 1.14.3 | pip | IndexFlatL2 — exact, the recall=1.0 anchor |
| KyzoDB | sibling checkout, commit in record | `::hnsw` index, m=16, efC=200, L2 |

## Standings (this hardware, single-threaded queries)

hnswlib 0.8.0 (build 33.4 s, library-default threading):

| efSearch | recall@10 | QPS |
|---|---|---|
| 10 | 0.708 | 43,648 |
| 20 | 0.839 | 27,840 |
| 40 | 0.927 | 17,264 |
| 80 | 0.974 | 9,821 |
| 120 | 0.988 | 4,370 |
| 200 | 0.996 | 4,452 |
| 400 | 0.9986 | 2,447 |
| 800 | 0.9993 | 1,326 |

FAISS IndexHNSWFlat 1.14.3 (build 34.3 s, library-default threading):

| efSearch | recall@10 | QPS |
|---|---|---|
| 10 | 0.721 | 24,840 |
| 20 | 0.849 | 15,290 |
| 40 | 0.934 | 9,050 |
| 80 | 0.978 | 4,958 |
| 120 | 0.989 | 3,454 |
| 200 | 0.996 | 2,201 |
| 400 | 0.9989 | 1,143 |
| 800 | 0.9993 | 592 |

FAISS IndexFlatL2 (exact): recall 0.99935 at 188 QPS. **0.99935 is this
dataset's recall ceiling, not an approximation error**: the 65 misses in
1,000,000 neighbor slots are all exact L2-distance ties at the 10th-neighbor
boundary (verified by recomputing the distances), where set-intersection
recall depends on which of the tied vectors the index happens to return.
Read every subject's curve against that ceiling.

Same-parameter HNSW head-to-head: hnswlib is ~1.7–2.2x faster than FAISS's
IndexHNSWFlat at equal recall on this host — a known ann-benchmarks result,
reproduced not assumed.

KyzoDB: does not complete at this pin. The build (M=16, efConstruction=200, same as above) OOMs
under the house's 12 GiB address-space cap after ~12.5 minutes, failing on a single 8 GiB
allocation — no recall/QPS curve to report. Filed as evidence on
[kyzo#76](https://github.com/kyzodb/kyzo/issues/76) alongside this bench's own build-time-vs-scale
sweep (exponent ≈1.44 measured at n=1k-16k, `--build-only`/`--n` flags on `kyzo-vector-runner`),
which tracked the same build as superlinear in build time.

kyzo#76 has since closed upstream (commit `f280483`): two candidate mechanisms for the residual
superlinearity were formed and tested directly, and per-insert search cost during `::hnsw` build
is now a machine-checked, non-ignored law (`per_insert_search_cost_is_bounded_by_construction`,
bounded above by `ef_construction × m_max0`) — the closing evidence is a decaying build-time
exponent (1.63 → 1.22 across n=1k-32k) read as warm-up approaching that ceiling, not runaway
growth. **That resolution does not by itself prove this bench's specific symptom (an 8 GiB single
allocation failing at n=1,000,000) is gone**: the issue's own closing comment scopes the 1M-vector
campaign as a separate, not-yet-run follow-up, and this bench has never re-measured against a
build containing the fix. This standings row stays as the historical record of what the old pin
produced; it is not evidence about the current engine either way. The honest next step is
re-running this exact sweep once KyzoDB is pinned to a tagged release, not reading this paragraph
as a resolution.

## Run it

    ./fetch-datasets.sh                       # SIFT1M, hash-verified
    /usr/bin/python -m venv .venv && .venv/bin/python -m pip install -r requirements.txt
    .venv/bin/python ann_baseline.py --subject hnswlib     --land
    .venv/bin/python ann_baseline.py --subject faiss-hnsw  --land
    .venv/bin/python ann_baseline.py --subject faiss-flat  --land
    .venv/bin/python export-flat.py           # HDF5 -> flat binary for the Rust runner
    cargo build --release -p kyzo-vector-runner
    ../../target/release/kyzo-vector-runner --flat ../../datasets/vector/flat \
        --store /tmp/kyzo-vec --runs 3

Each subject prints the envelope (`harness/envelope.py`) to stdout; `--land` writes it into
`results/` instead, refusing to overwrite a committed file.
