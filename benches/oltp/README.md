# Embedded OLTP — vs SQLite

Story: [kyzo#26](https://github.com/kyzodb/kyzo/issues/26) · Epic: [kyzo#39](https://github.com/kyzodb/kyzo/issues/39)

SQLite-methodology mixed read/write/update workloads (speedtest1-style). The goal is not to beat
SQLite at being SQLite; it is to quantify the premium the multi-model engine pays for what it adds,
and show it is acceptable.

Status: rig not yet built. Gates on engine product green
([kyzo#4](https://github.com/kyzodb/kyzo/issues/4)) for KyzoDB numbers.
