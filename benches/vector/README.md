# Vector search — ann-benchmarks and the big-ann filtered track

Story: [kyzo#25](https://github.com/kyzodb/kyzo/issues/25) · Epic: [kyzo#39](https://github.com/kyzodb/kyzo/issues/39)

Recall@k vs QPS curves on standard ann-benchmarks datasets against hnswlib and FAISS raw, and
embedded vector engines, class differences declared. We expect to lose raw unfiltered throughput to
FAISS and will publish it. The big-ann filtered track is the fight that matters: filtered vector
search is where "a vector search is a join" becomes a measurable curve.

Status: rig not yet built. Needs the engine's HNSW operator; opponent baselines startable now.
