# Time travel — as-of overhead vs history depth

Story: [kyzo#28](https://github.com/kyzodb/kyzo/issues/28) · Epic: [kyzo#39](https://github.com/kyzodb/kyzo/issues/39)

No standard benchmark exists for as-of queries, so this directory defines the first honest one: a
write stream with as-of reads at random depths, producing overhead-vs-history-length curves.
Comparators where shapes fit: Dolt and XTDB. Defining the reference benchmark makes us the
reference; that only works if the definition is scrupulously neutral, so the workload spec here gets
the fairness review before any engine runs it.

Status: benchmark definition not yet drafted. Gates on engine time travel verified end-to-end
([kyzo#4](https://github.com/kyzodb/kyzo/issues/4)) for KyzoDB numbers.
