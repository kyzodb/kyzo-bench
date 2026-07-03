//! The workload registry: the closed set of things this rig measures.
//!
//! Each entry is fully determined by its definition here — generator shape,
//! seed, and program — so "which workload was that" is always answerable by
//! reading this file at the commit a result names.

use crate::generate;
use kyzo_bench_harness::Seed;
use std::path::Path;

/// One benchmark workload: a program plus a deterministically generated
/// fact set. The closed enum is the registry; adding a workload is a
/// reviewed change to this type, not a config file nobody diffs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkloadSpec {
    /// Transitive closure on a uniform sparse digraph.
    Tc { n: u64, m: u64 },
    /// Same-generation on a layered ancestry graph.
    Sg {
        layers: u64,
        width: u64,
        parents_per: u64,
    },
    /// Andersen points-to on a synthetic statement mix.
    PointsTo {
        vars: u64,
        addrs: u64,
        assigns: u64,
        loads: u64,
        stores: u64,
    },
    /// Transitive closure on a real SNAP graph, fetched and hash-recorded
    /// by `benches/datalog/fetch-datasets.sh`.
    TcSnap { file: &'static str },
}

/// A registered workload: spec + identity. Only [`suite`] mints these, so
/// every id/seed pair in results traces back to this file.
#[derive(Debug, Clone, Copy)]
pub struct Registered {
    pub id: &'static str,
    pub description: &'static str,
    pub seed: Seed,
    pub spec: WorkloadSpec,
}

/// The suite, smallest to largest per family. Seeds are arbitrary but
/// fixed forever; changing one is changing the dataset.
pub fn suite() -> Vec<Registered> {
    vec![
        Registered {
            id: "tc/sparse-n2k-m6k",
            description: "transitive closure, uniform digraph, 2k nodes / 6k edges",
            seed: Seed(22_101),
            spec: WorkloadSpec::Tc { n: 2_000, m: 6_000 },
        },
        Registered {
            id: "tc/sparse-n10k-m30k",
            description: "transitive closure, uniform digraph, 10k nodes / 30k edges",
            seed: Seed(22_102),
            spec: WorkloadSpec::Tc {
                n: 10_000,
                m: 30_000,
            },
        },
        Registered {
            id: "sg/layered-l12-w600-p2",
            description: "same-generation, 12 generations of 600, 2 parents each",
            seed: Seed(22_201),
            spec: WorkloadSpec::Sg {
                layers: 12,
                width: 600,
                parents_per: 2,
            },
        },
        Registered {
            id: "pointsto/v3k-a2k-s6k",
            description: "Andersen points-to, 3k vars, 2k addr-of, 6k assigns, 2k loads, 2k stores",
            seed: Seed(22_301),
            spec: WorkloadSpec::PointsTo {
                vars: 3_000,
                addrs: 2_000,
                assigns: 6_000,
                loads: 2_000,
                stores: 2_000,
            },
        },
        // Real graphs. The seed is irrelevant to the data (it comes from
        // disk) but stays in the identity so records stay uniform.
        Registered {
            id: "tc/snap-wiki-Vote",
            description: "transitive closure, SNAP wiki-Vote (7.1k nodes, 103.7k edges, real)",
            seed: Seed(22_401),
            spec: WorkloadSpec::TcSnap { file: "wiki-Vote" },
        },
        // p2p-Gnutella31 was tried and refused: its closure blows the
        // 12 GiB cap (Souffle OOM-killed at ~46 s). Same family, cap-sized:
        Registered {
            id: "tc/snap-p2p-Gnutella08",
            description: "transitive closure, SNAP p2p-Gnutella08 (6.3k nodes, 20.8k edges, real)",
            seed: Seed(22_402),
            spec: WorkloadSpec::TcSnap {
                file: "p2p-Gnutella08",
            },
        },
    ]
}

impl Registered {
    /// The Souffle program file for this workload, relative to
    /// `benches/datalog/programs/`.
    pub fn souffle_program(&self) -> &'static str {
        match self.spec {
            WorkloadSpec::Tc { .. } | WorkloadSpec::TcSnap { .. } => "tc.dl",
            WorkloadSpec::Sg { .. } => "sg.dl",
            WorkloadSpec::PointsTo { .. } => "pointsto.dl",
        }
    }

    /// The output relation whose canonical bytes are the answer.
    pub fn output_relation(&self) -> &'static str {
        match self.spec {
            WorkloadSpec::Tc { .. } | WorkloadSpec::TcSnap { .. } => "tc",
            WorkloadSpec::Sg { .. } => "sg",
            WorkloadSpec::PointsTo { .. } => "pt",
        }
    }

    /// The KyzoScript program file, beside the Souffle one.
    pub fn kyzo_program(&self) -> &'static str {
        match self.spec {
            WorkloadSpec::Tc { .. } | WorkloadSpec::TcSnap { .. } => "tc.kz",
            WorkloadSpec::Sg { .. } => "sg.kz",
            WorkloadSpec::PointsTo { .. } => "pointsto.kz",
        }
    }

    /// The input relations and arities, as `kyzo-runner --relations` spec.
    pub fn relations_spec(&self) -> &'static str {
        match self.spec {
            WorkloadSpec::Tc { .. } | WorkloadSpec::TcSnap { .. } => "edge:2",
            WorkloadSpec::Sg { .. } => "parent:2",
            WorkloadSpec::PointsTo { .. } => "addr_of:2,assign:2,load:2,store:2",
        }
    }

    /// Generate this workload's fact files into `dir` — deterministically
    /// from the seed, or from the hash-recorded fetched file for real
    /// graphs. `repo_root` locates `datasets/`.
    pub fn generate(&self, dir: &Path, repo_root: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let rels = match self.spec {
            WorkloadSpec::Tc { n, m } => vec![generate::random_digraph(self.seed, n, m)],
            WorkloadSpec::TcSnap { file } => {
                let path = repo_root.join("datasets/snap").join(format!("{file}.txt"));
                if !path.is_file() {
                    return Err(std::io::Error::other(format!(
                        "{} not fetched; run benches/datalog/fetch-datasets.sh",
                        path.display()
                    )));
                }
                vec![generate::snap_edge_list(&path)?]
            }
            WorkloadSpec::Sg {
                layers,
                width,
                parents_per,
            } => {
                vec![generate::layered_parents(
                    self.seed,
                    layers,
                    width,
                    parents_per,
                )]
            }
            WorkloadSpec::PointsTo {
                vars,
                addrs,
                assigns,
                loads,
                stores,
            } => generate::pointsto_program(self.seed, vars, addrs, assigns, loads, stores),
        };
        for rel in rels {
            rel.write_facts(dir)?;
        }
        Ok(())
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suite_ids_are_unique() {
        let s = suite();
        let mut ids: Vec<&str> = s.iter().map(|w| w.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), s.len(), "duplicate workload id");
    }

    #[test]
    fn generation_lands_expected_files() {
        let dir = std::env::temp_dir().join(format!("kb-wl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let w = suite()
            .into_iter()
            .find(|w| w.id == "pointsto/v3k-a2k-s6k")
            .unwrap();
        w.generate(&dir, Path::new(".")).unwrap();
        for f in ["addr_of.facts", "assign.facts", "load.facts", "store.facts"] {
            assert!(dir.join(f).exists(), "missing {f}");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
