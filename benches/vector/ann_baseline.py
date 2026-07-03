#!/usr/bin/env python3
"""kyzo#25 — opponent baselines on ann-benchmarks SIFT1M (128-dim, Euclidean).

Subjects, both embedded libraries in their maintainers' intended
configuration:

- hnswlib 0.8.0: HNSW with the ann-benchmarks-standard build parameters
  (M=16, efConstruction=200), sweeping efSearch for the recall/QPS curve.
- faiss-cpu 1.14.3: IndexHNSWFlat with the same M/efConstruction band for
  a like-for-like HNSW comparison, plus IndexFlatL2 (exact brute force)
  as the recall = 1.0 anchor.

Method, ann-benchmarks practice throughout:
- recall@10 against the dataset's published exact ground truth;
- queries single-threaded (declared; thread knobs pinned to 1) so the
  curve measures the algorithm, not the host's core count — KyzoDB's
  future entry runs under the same rule;
- build is separately timed and may use the library's default threading
  (declared per record);
- each efSearch point runs the full 10k-query set 3 times; QPS is the
  median pass, recall is identical across passes (deterministic search).

Usage: ann_baseline.py --subject hnswlib|faiss-hnsw|faiss-flat [--runs 3] [--land]
"""

from __future__ import annotations

import argparse
import os
import resource
import sys
import time
from pathlib import Path

import h5py
import numpy as np

REPO_ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO_ROOT / "harness"))
import envelope  # noqa: E402  (path must be set up first)

MEM_CAP_BYTES = 12 * 1024**3
K = 10
EF_SWEEP = [10, 20, 40, 80, 120, 200, 400, 800]
M = 16
EF_CONSTRUCTION = 200
PINS = {"hnswlib": "0.8.0", "faiss-cpu": "1.14.3"}


def recall_at_k(found: np.ndarray, truth: np.ndarray) -> float:
    hits = 0
    for row_found, row_truth in zip(found, truth):
        hits += len(set(row_found.tolist()) & set(row_truth[:K].tolist()))
    return hits / (K * len(found))


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--subject", required=True,
                    choices=["hnswlib", "faiss-hnsw", "faiss-flat"])
    ap.add_argument("--runs", type=int, default=3)
    ap.add_argument("--land", action="store_true")
    args = ap.parse_args()

    resource.setrlimit(resource.RLIMIT_AS, (MEM_CAP_BYTES, MEM_CAP_BYTES))

    here = Path(__file__).resolve().parent
    data = here.parent.parent / "datasets" / "vector" / "sift-128-euclidean.hdf5"
    dataset_sha = (
        data.parent / f"{data.name}.sha256").read_text().split()[0]

    with h5py.File(data, "r") as f:
        train = np.asarray(f["train"], dtype=np.float32)
        test = np.asarray(f["test"], dtype=np.float32)
        truth = np.asarray(f["neighbors"], dtype=np.int64)
    n, dim = train.shape

    curve: list[dict[str, float]] = []
    build_threads: str

    from importlib.metadata import version as pkg_version

    if args.subject == "hnswlib":
        import hnswlib

        if pkg_version("hnswlib") != PINS["hnswlib"]:
            print(f"hnswlib {pkg_version('hnswlib')} != pin", file=sys.stderr)
            return 1
        index = hnswlib.Index(space="l2", dim=dim)
        index.init_index(max_elements=n, M=M, ef_construction=EF_CONSTRUCTION)
        t0 = time.monotonic()
        index.add_items(train)  # library default threading
        build_seconds = time.monotonic() - t0
        build_threads = f"library default ({os.cpu_count()} cores available)"
        index.set_num_threads(1)
        for ef in EF_SWEEP:
            index.set_ef(max(ef, K))
            passes = []
            found = None
            for _ in range(args.runs):
                t0 = time.monotonic()
                found, _ = index.knn_query(test, k=K)
                passes.append(time.monotonic() - t0)
            curve.append({
                "ef_search": ef,
                "recall_at_10": recall_at_k(found, truth),
                "qps": len(test) / sorted(passes)[len(passes) // 2],
            })
        version = PINS["hnswlib"]
        config = {"index": "hnsw", "M": M, "ef_construction": EF_CONSTRUCTION}
        package = "hnswlib"
    else:
        import faiss

        if pkg_version("faiss-cpu") != PINS["faiss-cpu"]:
            print(f"faiss-cpu {pkg_version('faiss-cpu')} != pin", file=sys.stderr)
            return 1
        if args.subject == "faiss-hnsw":
            index = faiss.IndexHNSWFlat(dim, M)
            index.hnsw.efConstruction = EF_CONSTRUCTION
            config = {"index": "IndexHNSWFlat", "M": M,
                      "ef_construction": EF_CONSTRUCTION}
        else:
            index = faiss.IndexFlatL2(dim)
            config = {"index": "IndexFlatL2 (exact)"}
        t0 = time.monotonic()
        index.add(train)  # library default threading
        build_seconds = time.monotonic() - t0
        build_threads = f"library default ({os.cpu_count()} cores available)"
        faiss.omp_set_num_threads(1)
        sweep = EF_SWEEP if args.subject == "faiss-hnsw" else [0]
        for ef in sweep:
            if args.subject == "faiss-hnsw":
                index.hnsw.efSearch = max(ef, K)
            passes = []
            found = None
            for _ in range(args.runs):
                t0 = time.monotonic()
                _, found = index.search(test, K)
                passes.append(time.monotonic() - t0)
            point = {
                "recall_at_10": recall_at_k(found, truth),
                "qps": len(test) / sorted(passes)[len(passes) // 2],
            }
            if args.subject == "faiss-hnsw":
                point["ef_search"] = ef
            curve.append(point)
        version = PINS["faiss-cpu"]
        package = "faiss-cpu"

    peak_rss_kib = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss

    metrics = {
        "dataset": {
            "name": "ann-benchmarks/sift-128-euclidean",
            "sha256": dataset_sha,
            "base_vectors": int(n),
            "dim": int(dim),
            "queries": int(test.shape[0]),
            "metric": "euclidean",
        },
        "scope_note": (
            "ann-benchmarks method: recall@10 vs single-threaded QPS against "
            "the dataset's exact ground truth. Build uses the library's "
            "default threading and is reported separately, outside the query "
            "clock. Each sweep point is the median of "
            f"{args.runs} full 10k-query passes. Memory capped via RLIMIT_AS "
            f"at {MEM_CAP_BYTES} bytes. Embedded-library class — a future "
            "server-class comparison (Qdrant/LanceDB) is a separate record."
        ),
        "config": config,
        "build_seconds": build_seconds,
        "build_threading": build_threads,
        "query_threads": 1,
        "curve": curve,
        "peak_rss_kib": peak_rss_kib,
    }
    env = envelope.build(
        bench="vector",
        story="kyzo#25",
        subject={
            "kind": "opponent",
            "name": args.subject,
            "version": version,
            "provenance": {"kind": "package", "ecosystem": "pip",
                           "package": package, "version": version},
        },
        metrics=metrics,
        repo_root=REPO_ROOT,
    )
    envelope.emit(
        env,
        land_it=args.land,
        results_dir=REPO_ROOT / "results",
        id_="sift1m",
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
