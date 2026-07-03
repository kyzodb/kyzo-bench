//! Adapters for opponents that recur across more than one bench. An
//! opponent used by exactly one bench keeps its adapter in that bench's own
//! crate (e.g. `datalog-rig`'s `souffle` module); it only moves here once a
//! second bench needs the same lookup, so this module never grows ahead of
//! actual duplication.

pub mod sqlite;
