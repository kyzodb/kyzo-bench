"""The one result envelope every non-Rust kyzo-bench script emits.

`kyzo_bench_harness::ResultRecord` is the Rust-side shape; this is its
Python-side equivalent for benches that time in-process library calls or
multi-service pipelines rather than subprocesses (`kuzu_baseline.py`,
`ann_baseline.py`, `driver.py` — none of these are `Rig`s, and forcing them
onto the subprocess trait would be the same modeling error kyzo#70 fixes,
relocated). Both shapes satisfy the same law
(`.claude/rules/results-data.md`): engine commit, opponent name and exact
version, dataset and its fetch hash, seed, hardware spec, date, and the
command that produced it — a result missing any of these does not land.

Every field genuinely bench-specific (Graphalytics's PR/WCC timings,
ann-benchmarks' recall/QPS curve, the kill shot's anomaly count, the
dataset identity and its fetch hash, the seed) lives in the open `metrics`
payload; it does not get its own top-level key or its own schema.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import time
from pathlib import Path


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


def rig_commit(repo_root: Path) -> dict[str, object]:
    """The kyzo-bench commit the rig ran from — the Python analog of Rust's
    `ResultRecord::rig_commit()`. `repo_root` is any path inside the
    kyzo-bench checkout; git resolves it to the repo root itself."""
    try:
        commit = subprocess.run(
            ["git", "-C", str(repo_root), "rev-parse", "HEAD"],
            capture_output=True, text=True, check=True,
        ).stdout.strip()
        dirty = bool(subprocess.run(
            ["git", "-C", str(repo_root), "status", "--porcelain"],
            capture_output=True, text=True, check=True,
        ).stdout.strip())
        return {"commit": commit, "dirty": dirty}
    except Exception:
        return {"commit": "unknown", "dirty": True}


def today_utc() -> str:
    return time.strftime("%Y-%m-%d", time.gmtime())


def sanitize(s: str) -> str:
    return "".join(c if (c.isalnum() or c in "-.") else "_" for c in s)


def build(
    *,
    bench: str,
    story: str,
    subject: dict[str, object],
    metrics: dict[str, object],
    repo_root: Path,
    supersedes: str | None = None,
) -> dict[str, object]:
    """Assemble one envelope. `subject` is the same `{kind, name, version,
    provenance, ...}` shape Rust's `Subject` enum serializes to for every
    `kind`/`provenance.kind` Rust's `Subject`/`Provenance` enums actually
    define (`"opponent"`/`"kyzo"`; `"built_from_source"`/`"container_image"`/
    `"package"`) — build it by hand here, since there is no shared type to
    import across the language boundary. A demo comparing something Rust's
    `Provenance` has no variant for (e.g. the kill shot's four-store
    pipeline) may extend `provenance.kind` with a new, documented value
    instead of forcing a bad fit; it must not silently drop fields."""
    envelope: dict[str, object] = {
        "bench": bench,
        "story": story,
        "subject": subject,
        "rig": rig_commit(repo_root),
        "hardware": hardware(),
        "date": today_utc(),
        "command": " ".join(sys.argv),
        "metrics": metrics,
    }
    if supersedes is not None:
        envelope["supersedes"] = supersedes
    return envelope


def land(
    envelope: dict[str, object],
    results_dir: Path,
    *,
    id_: str,
    seed: int | None = None,
) -> Path:
    """Write `envelope` into `results_dir`, refusing to overwrite — the
    same append-only law `ResultRecord::land` enforces in Rust, and the
    same `{bench}--{id}--{subject}[--seed{N}]--{date}.json` naming, `id_`
    being the bench-chosen identity component (a dataset name, a workload
    id) and `seed` present only for scripts whose workload has one."""
    subject = envelope["subject"]
    subject_label = f"{subject['name']}_{subject['version']}"
    seed_part = f"--seed{seed}" if seed is not None else ""
    name = (
        f"{envelope['bench']}--{sanitize(id_)}--{sanitize(subject_label)}"
        f"{seed_part}--{envelope['date']}.json"
    )
    path = results_dir / name
    if path.exists():
        raise FileExistsError(
            f"refusing to overwrite committed result {path}: results/ is "
            "append-only; land a superseding record instead"
        )
    results_dir.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(envelope, indent=2) + "\n")
    return path


def emit(
    envelope: dict[str, object],
    *,
    land_it: bool,
    results_dir: Path,
    id_: str,
    seed: int | None = None,
) -> None:
    """The shared ending every script's `main()` calls last. Unlike Rust's
    `rig::main`, which prints *or* lands (a landed run's JSON goes to the
    result file, not stdout), this always prints — a `--land` run is
    inspectable on stdout *and* written to `results_dir` — since a bare
    Python invocation with no `--land` is the common local-dev path and
    should never require re-running to see the number. None of these three
    scripts run KyzoDB itself (that is a separate, future subject), so
    there is no engine-commit dirty gate to enforce here the way Rust's
    `rig::main` gates on `Subject::Kyzo`'s commit — `rig.dirty` is recorded
    honestly either way, landed or not."""
    print(json.dumps(envelope, indent=2))
    if land_it:
        path = land(envelope, results_dir, id_=id_, seed=seed)
        print(f"landed: {path}", file=sys.stderr)
