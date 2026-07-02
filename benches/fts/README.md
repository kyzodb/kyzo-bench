# Full-text search — vs Tantivy and SQLite FTS5

Story: [kyzo#27](https://github.com/kyzodb/kyzo/issues/27) · Epic: [kyzo#39](https://github.com/kyzodb/kyzo/issues/39)

Latency and throughput against Tantivy standalone and SQLite FTS5. Tantivy alone will likely win the
standalone comparison and that result gets published. The claim we make is FTS composed inside
joins, negation, and recursion, a fight a standalone search library cannot enter.

Status: rig not yet built. Needs the engine's FTS operator; opponent baselines startable now.
