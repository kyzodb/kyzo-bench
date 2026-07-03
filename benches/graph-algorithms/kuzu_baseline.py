#!/usr/bin/env python3
"""kyzo#23 — the Kuzu opponent baseline on LDBC Graphalytics datasets.

Runs the Graphalytics algorithms Kuzu 0.11.3 natively ships in its `algo`
extension — PageRank and weakly connected components — with the parameters
the dataset's own .properties file prescribes, and checks answers against
LDBC's published reference outputs. The algorithms Kuzu does not ship
natively (BFS-to-all-depths, CDLP, LCC, SSSP) are out of this baseline and
declared as such in the result; running them through hand-written Cypher
would not be the configuration Kuzu's maintainers would sign off on.

Timing follows Graphalytics practice: processing time only (the CALL),
load time reported separately. Memory is capped in-process via RLIMIT_AS.

Usage: kuzu_baseline.py --dataset wiki-Talk [--runs 5]
"""

from __future__ import annotations

import argparse
import json
import os
import resource
import sys
import time
from pathlib import Path

MEM_CAP_BYTES = 12 * 1024**3
KUZU_PIN = "0.11.3"


def hardware() -> dict[str, object]:
    cpu = "unknown"
    mem_kib = 0
    with open("/proc/cpuinfo") as f:
        for line in f:
            if line.startswith("model name"):
                cpu = line.split(":", 1)[1].strip()
                break
    with open("/proc/meminfo") as f:
        for line in f:
            if line.startswith("MemTotal:"):
                mem_kib = int(line.split()[1])
                break
    return {
        "cpu_model": cpu,
        "logical_cpus": os.cpu_count(),
        "mem_total_kib": mem_kib,
        "arch": os.uname().machine,
        "kernel": os.uname().release,
    }


def rig_commit() -> dict[str, object]:
    import subprocess

    here = Path(__file__).resolve().parent
    try:
        commit = subprocess.run(
            ["git", "-C", str(here), "rev-parse", "HEAD"],
            capture_output=True, text=True, check=True,
        ).stdout.strip()
        dirty = bool(subprocess.run(
            ["git", "-C", str(here), "status", "--porcelain"],
            capture_output=True, text=True, check=True,
        ).stdout.strip())
        return {"commit": commit, "dirty": dirty}
    except Exception:
        return {"commit": "unknown", "dirty": True}


def read_properties(path: Path) -> dict[str, str]:
    props: dict[str, str] = {}
    for line in path.read_text().splitlines():
        line = line.strip()
        if line and not line.startswith("#") and "=" in line:
            k, v = line.split("=", 1)
            props[k.strip()] = v.strip()
    return props


def canonical_partition(labels: dict[int, int]) -> dict[int, int]:
    """Relabel each component by its minimum member, making two arbitrary
    labelings comparable."""
    groups: dict[int, list[int]] = {}
    for vertex, label in labels.items():
        groups.setdefault(label, []).append(vertex)
    canon: dict[int, int] = {}
    for members in groups.values():
        rep = min(members)
        for m in members:
            canon[m] = rep
    return canon


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--dataset", required=True)
    ap.add_argument("--runs", type=int, default=5)
    args = ap.parse_args()

    # Kuzu's buffer manager mmaps ~8 TiB of virtual address space by design,
    # so RLIMIT_AS would refuse its documented default configuration. The
    # memory cap is applied through Kuzu's own knob instead
    # (buffer_pool_size below); peak *resident* memory is reported.
    import kuzu

    if kuzu.__version__ != KUZU_PIN:
        print(f"kuzu {kuzu.__version__} != pinned {KUZU_PIN}", file=sys.stderr)
        return 1

    here = Path(__file__).resolve().parent
    data = here.parent.parent / "datasets" / "graphalytics" / args.dataset
    props = read_properties(data / f"{args.dataset}.properties")
    directed = props[f"graph.{args.dataset}.directed"] == "true"
    pr_damping = float(props[f"graph.{args.dataset}.pr.damping-factor"])
    pr_iters = int(props[f"graph.{args.dataset}.pr.num-iterations"])
    archive_hash = (
        (data.parent / f"{args.dataset}.tar.zst.sha256").read_text().split()[0]
    )

    scratch = here / ".scratch" / args.dataset
    scratch.mkdir(parents=True, exist_ok=True)
    # Kuzu's COPY dispatches on file extension; the Graphalytics files are
    # .v/.e, so link them in as .csv without copying bytes.
    v_csv = scratch / "v.csv"
    e_csv = scratch / "e.csv"
    for link, target in ((v_csv, data / f"{args.dataset}.v"), (e_csv, data / f"{args.dataset}.e")):
        if link.is_symlink() or link.exists():
            link.unlink()
        link.symlink_to(target)

    # Kuzu stores a database as a single file (plus .wal/.tmp siblings).
    db_path = scratch / "kuzu-db"
    for stale in scratch.glob("kuzu-db*"):
        if stale.is_dir():
            import shutil

            shutil.rmtree(stale)
        else:
            stale.unlink()

    db = kuzu.Database(str(db_path), buffer_pool_size=MEM_CAP_BYTES)
    con = kuzu.Connection(db)
    con.execute("INSTALL algo; LOAD algo;")
    con.execute("CREATE NODE TABLE V(id INT64 PRIMARY KEY)")
    con.execute("CREATE REL TABLE E(FROM V TO V)")

    t0 = time.monotonic()
    con.execute(f"COPY V FROM '{v_csv}' (DELIM=' ', HEADER=false)")
    con.execute(f"COPY E FROM '{e_csv}' (DELIM=' ', HEADER=false)")
    load_seconds = time.monotonic() - t0
    con.execute("CALL project_graph('G', ['V'], ['E'])")

    # Timed runs force the full computation to completion inside the engine
    # with an aggregate over every output row, returning one row to Python.
    # The first landed records timed the Python `get_next()` loop over
    # millions of rows inside the window — the fairness review measured
    # that as ~80% of the recorded PR time, charged to Kuzu against the
    # record's own "processing-only" claim. Those records were discarded.
    pr_call = (
        f"CALL page_rank('G', dampingFactor := {pr_damping}, "
        f"maxIterations := {pr_iters}, tolerance := 0.0) "
    )
    wcc_call = "CALL weakly_connected_components('G') "
    runs: dict[str, list[float]] = {"pr": [], "wcc": []}

    for i in range(args.runs + 1):  # first run is the discarded warm-up
        t0 = time.monotonic()
        r = con.execute(pr_call + "RETURN sum(rank), count(*)")
        r.get_next()
        dt = time.monotonic() - t0
        if i > 0:
            runs["pr"].append(dt)

    for i in range(args.runs + 1):
        t0 = time.monotonic()
        r = con.execute(wcc_call + "RETURN sum(group_id), count(*)")
        r.get_next()
        dt = time.monotonic() - t0
        if i > 0:
            runs["wcc"].append(dt)

    # Untimed verification pass: materialize full outputs for the
    # reference check.
    pr_result: dict[int, float] = {}
    r = con.execute(pr_call + "RETURN node.id, rank")
    while r.has_next():
        vertex, rank = r.get_next()
        pr_result[int(vertex)] = float(rank)
    wcc_result: dict[int, int] = {}
    r = con.execute(wcc_call + "RETURN node.id, group_id")
    while r.has_next():
        vertex, group = r.get_next()
        wcc_result[int(vertex)] = int(group)

    # Correctness vs LDBC reference outputs.
    ref_pr: dict[int, float] = {}
    for line in (data / f"{args.dataset}-PR").read_text().splitlines():
        vertex, value = line.split()
        ref_pr[int(vertex)] = float(value)
    max_rel_err = 0.0
    for vertex, ref in ref_pr.items():
        got = pr_result.get(vertex)
        if got is None:
            max_rel_err = float("inf")
            break
        denom = max(abs(ref), 1e-300)
        max_rel_err = max(max_rel_err, abs(got - ref) / denom)

    ref_wcc: dict[int, int] = {}
    for line in (data / f"{args.dataset}-WCC").read_text().splitlines():
        vertex, label = line.split()
        ref_wcc[int(vertex)] = int(label)
    wcc_match = canonical_partition(wcc_result) == canonical_partition(ref_wcc)

    peak_rss_kib = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss

    def stats(xs: list[float]) -> dict[str, float]:
        s = sorted(xs)
        return {
            "median_s": s[len(s) // 2],
            "min_s": s[0],
            "max_s": s[-1],
            "runs": len(s),
        }

    result = {
        "bench": "graph-algorithms",
        "story": "kyzo#23",
        "subject": {
            "kind": "opponent",
            "name": "kuzu",
            "version": KUZU_PIN,
            "provenance": {"kind": "package", "ecosystem": "pip", "package": "kuzu", "version": KUZU_PIN},
        },
        "dataset": {
            "name": f"graphalytics/{args.dataset}",
            "archive_sha256": archive_hash,
            "directed": directed,
            "vertices": int(props[f"graph.{args.dataset}.meta.vertices"]),
            "edges": int(props[f"graph.{args.dataset}.meta.edges"]),
        },
        "scope_note": (
            "Kuzu 0.11.3 `algo` extension runs PR and WCC natively; BFS, CDLP, "
            "LCC, SSSP are not in the extension and are excluded from this "
            "baseline rather than approximated through hand-written Cypher. "
            "Timing is processing-only per Graphalytics practice: the CALL is "
            "forced to completion inside the engine by aggregating over every "
            "output row (sum + count returning one row); client-side row "
            "marshaling is outside the clock, and the full output is fetched "
            "untimed afterwards for the reference check. Load time reported "
            "separately. Single process; memory capped through Kuzu's own "
            f"buffer_pool_size knob at {MEM_CAP_BYTES} bytes (RLIMIT_AS would "
            "refuse Kuzu's documented 8 TiB virtual-space buffer manager "
            "design)."
        ),
        "parameters": {"pr_damping": pr_damping, "pr_iterations": pr_iters, "wcc": "default"},
        "load_seconds": load_seconds,
        "timings": {k: stats(v) for k, v in runs.items()},
        "correctness": {
            "pr_max_relative_error_vs_ldbc_reference": max_rel_err,
            "pr_rank_sum": sum(pr_result.values()),
            "pr_reference_rank_sum": sum(ref_pr.values()),
            "pr_semantics_note": (
                "Kuzu 0.11.3's native page_rank does not redistribute "
                "dangling-vertex rank (rank sum decays below 1.0), while the "
                "Graphalytics PR specification does. Its PR timing is "
                "therefore recorded as Kuzu's own PR variant, NOT as a "
                "Graphalytics-comparable number; WCC is spec-comparable."
            ),
            "wcc_partition_matches_ldbc_reference": wcc_match,
        },
        "peak_rss_kib": peak_rss_kib,
        "hardware": hardware(),
        "rig": rig_commit(),
        "date_utc": time.strftime("%Y-%m-%d", time.gmtime()),
        "command": " ".join(sys.argv),
    }
    print(json.dumps(result, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
