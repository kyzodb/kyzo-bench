//! kyzo#71 gate 1 resolution for kyzo#22 (datalog) — a real, run, not a read
//! of SurrealDB's docs alone. ArcadeData's LDBC Graphalytics platform
//! writeup (tested against SurrealDB v2.6.4, March 2026) found
//! `{..+collect}` hangs indefinitely, even on a 3-node graph, and that
//! `{1..N}` silently doesn't compose past one hop. SurrealDB 3.1.5's own
//! changelog lists "fixes to graph recursion" as a named, dated change
//! between that report and the `3.2.0` pin this repo uses — so the only
//! honest way to decide whether `tc.dl` (transitive closure — exactly what
//! `{..+collect}` computes, per-source-node) is in scope is to run it
//! against the pinned version, not to trust either the old bug report or
//! the current docs' description of intended behavior.
//!
//! This is a `#[test]`, not a throwaway script: the gate-1 scope table this
//! proves is a standing claim in `kyzo#71`, and a claim that stands without
//! a test backing it is exactly what `.claude/skills/verify-the-number`
//! forbids.

#[cfg(test)]
mod tests {
    use surrealdb::engine::local::RocksDb;
    use surrealdb::Surreal;

    /// A 5-node directed chain, 0->1->2->3->4, plus an isolated node 5 with
    /// no outgoing edge. Exact transitive closure from 0 is {1,2,3,4}; from
    /// 5 is {}. If `{..+collect}` hangs, this test hangs — which is itself
    /// the answer (and `cargo test` under any CI timeout will report it as
    /// a failure, not a false pass).
    #[tokio::test]
    async fn recursive_collect_computes_transitive_closure_on_a_chain() {
        let store = std::env::temp_dir().join(format!(
            "kb-graph-recursion-probe-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&store);
        let db = Surreal::new::<RocksDb>(store.to_str().unwrap())
            .await
            .unwrap();
        db.use_ns("kyzo_bench").use_db("probe").await.unwrap();
        db.query(
            "CREATE pt:0, pt:1, pt:2, pt:3, pt:4, pt:5;
             RELATE pt:0->edge->pt:1;
             RELATE pt:1->edge->pt:2;
             RELATE pt:2->edge->pt:3;
             RELATE pt:3->edge->pt:4;",
        )
        .await
        .unwrap()
        .check()
        .unwrap();

        // Bounded depth (not open-ended `{..}`) even though 5 nodes makes
        // an open-ended range safe here: the `tc` workloads this decides
        // scope for (sparse-n10k-m30k, snap-p2p-Gnutella08, snap-wiki-Vote)
        // are real graphs with unknown diameter, and RELATE's own docs
        // warn open-ended ranges can recurse to depth 256 — so any actual
        // `tc` bridge must bound depth explicitly. `{..40}` here is well
        // past this tiny graph's diameter of 4, proving the range form
        // (not just the open-ended form) also collects correctly.
        let mut res = db
            .query(
                "RETURN pt:0.{..40+collect}(->edge->pt).map(|$r| record::id($r));",
            )
            .await
            .unwrap()
            .check()
            .unwrap();
        let reached: Vec<i64> = res.take(0).unwrap();
        let mut got = reached;
        got.sort_unstable();
        assert_eq!(got, vec![1, 2, 3, 4], "transitive closure from pt:0");

        let mut res5 = db
            .query(
                "RETURN pt:5.{..40+collect}(->edge->pt).map(|$r| record::id($r));",
            )
            .await
            .unwrap()
            .check()
            .unwrap();
        let reached5: Vec<i64> = res5.take(0).unwrap();
        assert!(reached5.is_empty(), "pt:5 has no outgoing edge");

        std::fs::remove_dir_all(&store).unwrap();
    }
}
