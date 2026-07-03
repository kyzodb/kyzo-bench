# Demo: the consistency kill shot

Story: [kyzo#35](https://github.com/kyzodb/kyzo/issues/35) · Epic: [kyzo#41](https://github.com/kyzodb/kyzo/issues/41)

A hybrid answer assembled from four excellent stores can describe **a moment that never existed**.
This rig makes that visible, counts it, and prints the witnesses.

## The pipeline side (runnable now)

Four best-of-breed stores, pinned, vendor-default configurations, wired the way production
hybrid-search / RAG pipelines wire them:

| Role | Store | Pin |
| --- | --- | --- |
| canonical rows | PostgreSQL | `postgres:16.13-alpine` |
| vectors | Qdrant | `qdrant/qdrant:v1.18.0` |
| full text | Elasticsearch | `elasticsearch:9.2.4` |
| graph | Neo4j | `neo4j:5.26.28-community` |

One writer updates documents: for each update it writes **all four stores, each write durable and
immediately visible before the next** (Postgres commit; Qdrant `wait=True`; Elasticsearch
`refresh=true`; Neo4j managed transaction). That is the strongest thing a stitched pipeline can do
short of building a distributed transaction by hand. One reader assembles hybrid answers the way a
RAG stack does — vector hit, canonical row, keyword index, graph hop — and records the version each
store reported for the same document.

An **anomaly** is an answer whose four versions disagree: a cross-store read tear. The answer
corresponds to no committed state of the pipeline — the application committed "document at version
N in all four stores", and no anomalous answer is that, for any N. (Stated precisely: some torn
quadruples do transiently exist mid-write, since a four-store write is four commit points; the
tear is that the *reader* can be served from inside another writer's half-applied update, which
single-system snapshot isolation makes impossible by construction.) No store misbehaved; every
anomaly is the seam *between* them.
That is the fairness claim, and it is why the driver refuses every cheap shot: no artificial delays,
no misconfigured refresh intervals, no dropped writes, separate connections per thread.

## Run it

    ./run.sh [seed] [reads]        # defaults: seed 35001, 2000 reads

Brings the pipeline up with `docker compose up --wait`, runs the seeded workload, prints a witness
JSON (anomaly count, rate, and the first ten witnesses with the exact version quadruples), tears
everything down. One measured run on the rig hardware (32-core, seed 35001, 2000 reads, 296
concurrent writes): **81 anomalies, 4.05%** — see `results/killshot--stitched-pipeline--*.json`.

The anomaly *count* varies with scheduling — it is a race being made visible; the seeded workload
is deterministic, the interleaving is the operating system's. What is reproducible from a clean
clone: anomalies occur, structurally, at a rate that grows with write contention.

## The KyzoDB side (lands with the engine's public query API, kyzo#4)

The same workload against one KyzoDB instance: each document update writes the row, the vector, the
text, and the graph edges **in one transaction**; each hybrid answer is **one query against one
snapshot**. The anomaly count is zero not because KyzoDB is fast but because the moment the answer
describes is a real committed state — serializable snapshot isolation over one keyspace. When the
engine API lands, this directory gains `driver_kyzo.py` and the side-by-side becomes the demo.

No hype vocabulary: run both sides, read the witness files, draw the conclusion.
