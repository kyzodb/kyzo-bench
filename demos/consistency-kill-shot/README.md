# Demo: the consistency kill shot

Story: [kyzo#35](https://github.com/kyzodb/kyzo/issues/35) · Epic: [kyzo#41](https://github.com/kyzodb/kyzo/issues/41)

Side by side under concurrent writes: a four-service pipeline (Postgres + Qdrant + Elasticsearch +
Neo4j) assembles a hybrid answer from different moments in time, provably inconsistent; the same
workload on KyzoDB cannot produce the anomaly, because one transaction answered. The stitched-systems
story told as a correctness failure, not a convenience pitch. Ships as a docker-compose rig anyone
can run.

Status: not yet built. Fully startable now (the pipeline side needs no KyzoDB at all).
