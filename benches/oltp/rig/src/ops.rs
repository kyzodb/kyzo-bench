//! The deterministic op stream both engines execute.
//!
//! One stream, minted once from the seed; each subject renders it into its
//! own language. Reads print their result rows, so a subject cannot elide
//! them; the concatenated read output plus the final table dump must be
//! byte-identical across subjects or the comparison is refused.

use kyzo_bench_harness::{Seed, SplitMix64};

/// One logical operation against the single `item(id => grp, val)` table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// Point read by primary key; the subject prints `grp\tval` per hit.
    Read { id: u64 },
    /// `val := val + 1` on one row (no-op if the row is gone).
    Update { id: u64 },
    /// Insert a fresh row (ids from a disjoint range, so never a conflict).
    Insert { id: u64, grp: u64, val: u64 },
    /// Delete one row by primary key (no-op if already gone).
    Delete { id: u64 },
}

/// The full workload: a bulk-load prefix and a mixed suffix.
#[derive(Debug, Clone)]
pub struct OpStream {
    /// `(id, grp, val)` rows bulk-loaded before the mixed phase.
    pub load: Vec<(u64, u64, u64)>,
    pub mixed: Vec<Op>,
}

/// Mix in speedtest1's spirit: read-heavy with a real write fraction.
/// 60% reads / 20% updates / 10% inserts / 10% deletes.
pub fn generate(seed: Seed, rows: u64, ops: u64) -> OpStream {
    let mut rng = SplitMix64::new(seed);
    let groups = (rows / 100).max(1);

    let load: Vec<(u64, u64, u64)> = (0..rows)
        .map(|id| (id, rng.next_u64() % groups, rng.next_u64() % 1_000_000))
        .collect();

    // Fresh inserts live above every loaded id so they never collide.
    let mut next_insert_id = rows;
    let mixed = (0..ops)
        .map(|_| {
            let roll = rng.next_u64() % 100;
            let id = rng.next_u64() % rows;
            if roll < 60 {
                Op::Read { id }
            } else if roll < 80 {
                Op::Update { id }
            } else if roll < 90 {
                let id = next_insert_id;
                next_insert_id += 1;
                Op::Insert {
                    id,
                    grp: rng.next_u64() % groups,
                    val: rng.next_u64() % 1_000_000,
                }
            } else {
                Op::Delete { id }
            }
        })
        .collect();

    OpStream { load, mixed }
}

/// Rows per bulk-load batch (one transaction per batch on both sides).
pub const LOAD_BATCH: usize = 1_000;

/// Render the load phase as SQLite SQL. WAL + synchronous=NORMAL is the
/// declared production configuration; it appears in the record's notes.
pub fn sqlite_load(s: &OpStream) -> String {
    let mut out = String::from(
        "PRAGMA journal_mode=WAL;\nPRAGMA synchronous=NORMAL;\n\
         CREATE TABLE item(id INTEGER PRIMARY KEY, grp INTEGER NOT NULL, val INTEGER NOT NULL);\n",
    );
    for batch in s.load.chunks(LOAD_BATCH) {
        out.push_str("BEGIN;\nINSERT INTO item VALUES\n");
        for (i, (id, grp, val)) in batch.iter().enumerate() {
            if i > 0 {
                out.push_str(",\n");
            }
            out.push_str(&format!("({id},{grp},{val})"));
        }
        out.push_str(";\nCOMMIT;\n");
    }
    out
}

/// Render the mixed phase as SQLite SQL: one statement per op, autocommit,
/// exactly the granularity the KyzoDB side gets (one script per op).
pub fn sqlite_mixed(s: &OpStream) -> String {
    let mut out = String::from(".separator \"\\t\"\nPRAGMA synchronous=NORMAL;\n");
    for (idx, op) in s.mixed.iter().enumerate() {
        match *op {
            Op::Read { id } => {
                // The op index rides along so the cross-subject comparison
                // verifies each individual read, not just the multiset.
                out.push_str(&format!(
                    "SELECT {idx}, grp, val FROM item WHERE id={id};\n"
                ));
            }
            Op::Update { id } => {
                out.push_str(&format!("UPDATE item SET val=val+1 WHERE id={id};\n"));
            }
            Op::Insert { id, grp, val } => {
                out.push_str(&format!("INSERT INTO item VALUES({id},{grp},{val});\n"));
            }
            Op::Delete { id } => {
                out.push_str(&format!("DELETE FROM item WHERE id={id};\n"));
            }
        }
    }
    out
}

/// Render the final-state dump: every surviving row, ordered by key.
pub fn sqlite_dump() -> String {
    ".separator \"\\t\"\nSELECT id, grp, val FROM item ORDER BY id;\n".to_owned()
}

/// Serialize the stream for the KyzoDB runner: a line-oriented file it can
/// replay without linking the generator (`L id grp val` / `R id` / …).
pub fn kyzo_stream(s: &OpStream) -> String {
    let mut out = String::new();
    for (id, grp, val) in &s.load {
        out.push_str(&format!("L {id} {grp} {val}\n"));
    }
    for op in &s.mixed {
        match *op {
            Op::Read { id } => out.push_str(&format!("R {id}\n")),
            Op::Update { id } => out.push_str(&format!("U {id}\n")),
            Op::Insert { id, grp, val } => out.push_str(&format!("I {id} {grp} {val}\n")),
            Op::Delete { id } => out.push_str(&format!("D {id}\n")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_is_deterministic() {
        let a = generate(Seed(26_001), 1000, 500);
        let b = generate(Seed(26_001), 1000, 500);
        assert_eq!(a.load, b.load);
        assert_eq!(a.mixed, b.mixed);
        let c = generate(Seed(26_002), 1000, 500);
        assert_ne!(a.mixed, c.mixed, "different seed, different stream");
    }

    #[test]
    fn inserts_never_collide_with_loaded_ids() {
        let s = generate(Seed(26_001), 1000, 5000);
        for op in &s.mixed {
            if let Op::Insert { id, .. } = op {
                assert!(*id >= 1000);
            }
        }
    }
}
