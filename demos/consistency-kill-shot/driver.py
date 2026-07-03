#!/usr/bin/env python3
"""The consistency kill shot — pipeline side.

One writer keeps a tiny knowledge base up to date across four excellent
stores (Postgres, Qdrant, Elasticsearch, Neo4j), exactly the way production
hybrid-search pipelines wire them. One reader assembles a hybrid answer the
way a RAG stack does: vector hit, canonical row, keyword index, graph hop.

Every single write is durable and immediately visible before the writer
moves on (Postgres commit; Qdrant wait=True; Elasticsearch refresh=true;
Neo4j managed transaction). No store is misconfigured, no write is dropped,
nothing races inside any one store. The anomaly this demo counts is
therefore structural: there is no transaction that spans the four stores,
so a reader touching all four can observe a moment that never existed.

The same workload against KyzoDB is one transaction per update and one
snapshot per answer; that side of the demo lands when the engine's public
query API does (kyzo#4), and its anomaly count is zero by construction.

Deterministic: all document choices flow from --seed. The anomaly *count*
depends on scheduling (it is a race being made visible), which is the
point: the demo prints witnesses, not just a number.
"""

from __future__ import annotations

import argparse
import json
import sys
import threading
import time
from dataclasses import dataclass, asdict


DOCS = 20  # small on purpose: contention is the subject being demonstrated
VEC_DIM = 8
COLLECTION = "killshot_docs"
ES_INDEX = "killshot-docs"


def splitmix64(state: int) -> tuple[int, int]:
    """The same SplitMix64 the Rust harness uses; one algorithm everywhere."""
    state = (state + 0x9E3779B97F4A7C15) & 0xFFFFFFFFFFFFFFFF
    z = state
    z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & 0xFFFFFFFFFFFFFFFF
    z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & 0xFFFFFFFFFFFFFFFF
    return state, z ^ (z >> 31)


class SeededChoice:
    def __init__(self, seed: int) -> None:
        self._state = seed

    def below(self, bound: int) -> int:
        self._state, v = splitmix64(self._state)
        return v % bound


def doc_vector(doc_id: int, version: int) -> list[float]:
    """A deterministic stand-in embedding: a pure function of (id, version),
    like a real embedding is a pure function of the document text."""
    out = []
    state = (doc_id << 32) | (version & 0xFFFFFFFF)
    for _ in range(VEC_DIM):
        state, v = splitmix64(state)
        out.append((v % 10_000) / 10_000.0)
    return out


@dataclass
class HybridAnswer:
    """One assembled answer: the version of the SAME document as seen by
    each of the four stores during one read pass."""

    doc_id: int
    pg_version: int
    qdrant_version: int
    es_version: int
    neo4j_version: int
    at_unix: float

    def consistent(self) -> bool:
        return (
            self.pg_version
            == self.qdrant_version
            == self.es_version
            == self.neo4j_version
        )


class Pipeline:
    """The four clients, wired with per-write durability and visibility.

    Connecting and resetting are different acts: every thread connects,
    exactly one caller resets, once, before the run.
    """

    def __init__(self) -> None:
        import psycopg
        from qdrant_client import QdrantClient
        from qdrant_client.models import PointStruct
        from elasticsearch import Elasticsearch
        from neo4j import GraphDatabase

        self._PointStruct = PointStruct
        self.pg = psycopg.connect(
            "host=127.0.0.1 port=15432 user=killshot password=killshot dbname=killshot"
        )
        self.qdrant = QdrantClient(url="http://127.0.0.1:16333", timeout=30)
        self.es = Elasticsearch("http://127.0.0.1:19200", request_timeout=30)
        self.neo = GraphDatabase.driver(
            "bolt://127.0.0.1:17687", auth=("neo4j", "killshotpass")
        )

    def reset(self) -> None:
        """Blank slate in all four stores. Run once, before the workload."""
        from qdrant_client.models import Distance, VectorParams

        with self.pg.cursor() as cur:
            cur.execute("DROP TABLE IF EXISTS docs")
            cur.execute(
                "CREATE TABLE docs ("
                "id INT PRIMARY KEY, version INT NOT NULL, title TEXT NOT NULL)"
            )
        self.pg.commit()
        if self.qdrant.collection_exists(COLLECTION):
            self.qdrant.delete_collection(COLLECTION)
        self.qdrant.create_collection(
            COLLECTION,
            vectors_config=VectorParams(size=VEC_DIM, distance=Distance.COSINE),
        )
        if self.es.indices.exists(index=ES_INDEX):
            self.es.indices.delete(index=ES_INDEX)
        self.es.indices.create(index=ES_INDEX)
        with self.neo.session() as s:
            s.run("MATCH (d:Doc) DETACH DELETE d")

    def seed_docs(self) -> None:
        for doc_id in range(DOCS):
            self.write_doc(doc_id, 1)

    def write_doc(self, doc_id: int, version: int) -> None:
        """One pipeline update: four durable, immediately visible writes.
        This is the strongest thing a stitched pipeline can do — and it is
        still not atomic across the four."""
        title = f"doc-{doc_id}-v{version}"
        with self.pg.cursor() as cur:
            cur.execute(
                "INSERT INTO docs (id, version, title) VALUES (%s, %s, %s) "
                "ON CONFLICT (id) DO UPDATE SET version = %s, title = %s",
                (doc_id, version, title, version, title),
            )
        self.pg.commit()
        self.qdrant.upsert(
            COLLECTION,
            points=[
                self._PointStruct(
                    id=doc_id,
                    vector=doc_vector(doc_id, version),
                    payload={"version": version, "title": title},
                )
            ],
            wait=True,
        )
        self.es.index(
            index=ES_INDEX,
            id=str(doc_id),
            document={"version": version, "title": title},
            refresh=True,
        )
        with self.neo.session() as s:
            s.run(
                "MERGE (d:Doc {id: $id}) SET d.version = $version, d.title = $title",
                id=doc_id,
                version=version,
                title=title,
            )

    def read_answer(self, doc_id: int) -> HybridAnswer:
        """One hybrid answer, the four hops a RAG stack makes. Each read is
        the store's own current truth; the quadruple is the 'moment' the
        pipeline claims to be answering from."""
        points = self.qdrant.retrieve(COLLECTION, ids=[doc_id], with_payload=True)
        qdrant_version = int(points[0].payload["version"]) if points else -1
        with self.pg.cursor() as cur:
            cur.execute("SELECT version FROM docs WHERE id = %s", (doc_id,))
            row = cur.fetchone()
        self.pg.commit()
        pg_version = int(row[0]) if row else -1
        es_doc = self.es.get(index=ES_INDEX, id=str(doc_id))
        es_version = int(es_doc["_source"]["version"])
        with self.neo.session() as s:
            rec = s.run(
                "MATCH (d:Doc {id: $id}) RETURN d.version AS v", id=doc_id
            ).single()
        neo4j_version = int(rec["v"]) if rec else -1
        return HybridAnswer(
            doc_id=doc_id,
            pg_version=pg_version,
            qdrant_version=qdrant_version,
            es_version=es_version,
            neo4j_version=neo4j_version,
            at_unix=time.time(),
        )

    def close(self) -> None:
        self.pg.close()
        self.neo.close()
        self.es.close()
        self.qdrant.close()


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--seed", type=int, required=True)
    ap.add_argument("--reads", type=int, default=2000)
    ap.add_argument("--out", type=str, default=None, help="witness JSON file")
    args = ap.parse_args()

    pipeline = Pipeline()
    pipeline.reset()
    pipeline.seed_docs()

    stop = threading.Event()
    versions = {doc_id: 1 for doc_id in range(DOCS)}
    writes = 0

    def writer() -> None:
        nonlocal writes
        # The writer needs its own client connections: sharing live handles
        # across threads would be a driver bug, not a store anomaly.
        w = Pipeline()
        choose = SeededChoice(args.seed)
        try:
            while not stop.is_set():
                doc_id = choose.below(DOCS)
                versions[doc_id] += 1
                w.write_doc(doc_id, versions[doc_id])
                writes += 1
        finally:
            w.close()

    t = threading.Thread(target=writer, daemon=True)
    t.start()

    choose = SeededChoice(args.seed ^ 0xDEADBEEF)
    answers: list[HybridAnswer] = []
    anomalies: list[HybridAnswer] = []
    for _ in range(args.reads):
        a = pipeline.read_answer(choose.below(DOCS))
        answers.append(a)
        if not a.consistent():
            anomalies.append(a)

    stop.set()
    t.join(timeout=30)
    pipeline.close()

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
        import os

        return {
            "cpu_model": cpu,
            "logical_cpus": os.cpu_count(),
            "mem_total_kib": mem_kib,
            "arch": os.uname().machine,
            "kernel": os.uname().release,
        }

    def rig_commit() -> dict[str, object]:
        import subprocess

        here = os.path.dirname(os.path.abspath(__file__))
        try:
            commit = subprocess.run(
                ["git", "-C", here, "rev-parse", "HEAD"],
                capture_output=True, text=True, check=True,
            ).stdout.strip()
            dirty = bool(subprocess.run(
                ["git", "-C", here, "status", "--porcelain"],
                capture_output=True, text=True, check=True,
            ).stdout.strip())
            return {"commit": commit, "dirty": dirty}
        except Exception:
            return {"commit": "unknown", "dirty": True}

    result = {
        "demo": "consistency-kill-shot",
        "side": "stitched-pipeline",
        "story": "kyzo#35",
        "date_utc": time.strftime("%Y-%m-%d", time.gmtime()),
        "command": " ".join(sys.argv),
        "hardware": hardware(),
        "rig": rig_commit(),
        "stores": {
            "postgres": "16.13",
            "qdrant": "1.18.0",
            "elasticsearch": "9.2.4",
            "neo4j": "5.26.28-community",
        },
        "seed": args.seed,
        "docs": DOCS,
        "reads": len(answers),
        "writes_completed": writes,
        "anomalies": len(anomalies),
        "anomaly_rate": len(anomalies) / len(answers) if answers else 0.0,
        "first_witnesses": [asdict(a) for a in anomalies[:10]],
    }
    text = json.dumps(result, indent=2)
    print(text)
    if args.out:
        with open(args.out, "w") as f:
            f.write(text + "\n")
    # Exit 0 either way: the number is the result. The README interprets it.
    return 0


if __name__ == "__main__":
    sys.exit(main())
