//! Deterministic workload generation: every fact set is a pure function of
//! a [`Seed`], so the dataset needs no download and no trust — the generator
//! source *is* the dataset, and the digest in the result record proves the
//! bytes.

use kyzo_bench_harness::Seed;
use kyzo_bench_harness::seed::SplitMix64;
use std::collections::BTreeSet;
use std::io::{BufWriter, Write};
use std::path::Path;

/// A generated fact file: relation name → rows of tab-separated numbers.
/// `BTreeSet` fixes both dedup and emission order, so identical seeds give
/// byte-identical files on any platform.
pub struct FactRelation {
    pub name: &'static str,
    pub rows: BTreeSet<Vec<u64>>,
}

impl FactRelation {
    pub fn write_facts(&self, dir: &Path) -> std::io::Result<()> {
        let path = dir.join(format!("{}.facts", self.name));
        let mut w = BufWriter::new(std::fs::File::create(path)?);
        for row in &self.rows {
            let line: Vec<String> = row.iter().map(u64::to_string).collect();
            writeln!(w, "{}", line.join("\t"))?;
        }
        w.flush()
    }
}

/// A uniform random sparse digraph: `n` nodes, `m` distinct edges, no
/// self-loops. The standard subject for transitive closure.
pub fn random_digraph(seed: Seed, n: u64, m: u64) -> FactRelation {
    assert!(n >= 2, "a digraph needs at least two nodes");
    assert!(
        m <= n * (n - 1),
        "more distinct edges than the graph can hold"
    );
    let mut rng = SplitMix64::new(seed);
    let mut rows = BTreeSet::new();
    while (rows.len() as u64) < m {
        let x = rng.below(n);
        let y = rng.below(n);
        if x != y {
            rows.insert(vec![x, y]);
        }
    }
    FactRelation { name: "edge", rows }
}

/// A layered ancestry graph for same-generation: `layers` generations of
/// `width` people; each person has `parents_per` parents drawn from the
/// layer above. Recursion depth is exactly `layers - 1`.
pub fn layered_parents(seed: Seed, layers: u64, width: u64, parents_per: u64) -> FactRelation {
    assert!(layers >= 2 && width >= 2);
    assert!(parents_per >= 1 && parents_per <= width);
    let mut rng = SplitMix64::new(seed);
    let mut rows = BTreeSet::new();
    // Node id = layer * width + index; layer 0 is the oldest generation.
    for layer in 1..layers {
        for i in 0..width {
            let child = layer * width + i;
            while rows.iter().filter(|r: &&Vec<u64>| r[0] == child).count() < parents_per as usize {
                let parent = (layer - 1) * width + rng.below(width);
                rows.insert(vec![child, parent]);
            }
        }
    }
    FactRelation {
        name: "parent",
        rows,
    }
}

/// A real graph from a SNAP edge-list file: tab- or space-separated pairs,
/// `#` comment lines skipped, deduplicated, self-loops dropped (they add
/// nothing to closure workloads and some engines refuse them). The digest
/// of the emitted .facts file is what results name; the fetch script
/// separately records the archive hash.
pub fn snap_edge_list(path: &std::path::Path) -> std::io::Result<FactRelation> {
    use std::io::BufRead;
    let mut rows = BTreeSet::new();
    let reader = std::io::BufReader::new(std::fs::File::open(path)?);
    for line in reader.lines() {
        let line = line?;
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let mut it = line.split_whitespace();
        let (Some(x), Some(y)) = (it.next(), it.next()) else {
            return Err(std::io::Error::other(format!("bad edge line {line:?}")));
        };
        let parse = |s: &str| {
            s.parse::<u64>()
                .map_err(|_| std::io::Error::other(format!("non-integer vertex {s:?}")))
        };
        let (x, y) = (parse(x)?, parse(y)?);
        if x != y {
            rows.insert(vec![x, y]);
        }
    }
    Ok(FactRelation { name: "edge", rows })
}

/// A synthetic points-to input in the shape real Andersen inputs take:
/// `vars` pointer variables, `addrs` address-taken statements, plus assign,
/// load, and store statements in the given counts.
pub fn pointsto_program(
    seed: Seed,
    vars: u64,
    addrs: u64,
    assigns: u64,
    loads: u64,
    stores: u64,
) -> Vec<FactRelation> {
    assert!(vars >= 2);
    let mk = |label: &str, count: u64, name: &'static str| {
        let mut rng = SplitMix64::new(seed.derive(label));
        let mut rows = BTreeSet::new();
        while (rows.len() as u64) < count {
            let y = rng.below(vars);
            let x = rng.below(vars);
            if y != x {
                rows.insert(vec![y, x]);
            }
        }
        FactRelation { name, rows }
    };
    vec![
        mk("addr_of", addrs, "addr_of"),
        mk("assign", assigns, "assign"),
        mk("load", loads, "load"),
        mk("store", stores, "store"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_is_seed_pure() {
        let a = random_digraph(Seed(22_001), 100, 300);
        let b = random_digraph(Seed(22_001), 100, 300);
        assert_eq!(a.rows, b.rows);
        let c = random_digraph(Seed(22_002), 100, 300);
        assert_ne!(a.rows, c.rows, "different seeds must differ");
        assert_eq!(a.rows.len(), 300);
        assert!(a.rows.iter().all(|r| r[0] != r[1]), "no self-loops");
    }

    #[test]
    fn layered_parents_stay_in_adjacent_layers() {
        let g = layered_parents(Seed(7), 4, 10, 2);
        for row in &g.rows {
            let (child, parent) = (row[0], row[1]);
            assert_eq!(child / 10, parent / 10 + 1, "parent must be one layer up");
        }
        // every non-root person has exactly 2 parents
        for layer in 1..4u64 {
            for i in 0..10u64 {
                let child = layer * 10 + i;
                assert_eq!(g.rows.iter().filter(|r| r[0] == child).count(), 2);
            }
        }
    }

    #[test]
    fn pointsto_relations_have_requested_sizes() {
        let rels = pointsto_program(Seed(3), 50, 40, 60, 20, 20);
        let sizes: Vec<usize> = rels.iter().map(|r| r.rows.len()).collect();
        assert_eq!(sizes, vec![40, 60, 20, 20]);
    }
}
