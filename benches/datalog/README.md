# Recursive Datalog — vs Souffle and DDlog

Story: [kyzo#22](https://github.com/kyzodb/kyzo/issues/22) · Epic: [kyzo#39](https://github.com/kyzodb/kyzo/issues/39)

Transitive closure, same-generation, and context-insensitive points-to on Doop-style Java facts and
Graspan's Linux/PostgreSQL dataflow graphs. Opponents: Souffle (compiled mode) and DDlog, pinned and
configured per their own performance docs. Metrics: wall clock and peak memory, correctness checked
by output hash on both sides. Souffle compiles to C++; within striking distance interpreted is the
win we claim.

Status: rig not yet built. Startable now; KyzoDB-side numbers gate on engine product green
([kyzo#4](https://github.com/kyzodb/kyzo/issues/4)).
