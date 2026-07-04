//! kyzo#27 — full-text search vs Tantivy and SQLite FTS5.
//!
//! One corpus (Gutenberg paragraphs), one seeded query set (term / AND /
//! OR / phrase, all terms provably tokenizer-neutral), two externally
//! timed phases per subject: `index` (build a persistent index from
//! docs.tsv) and `query` (the query set, `PASSES` timing passes plus one
//! verified pass). Match sets must agree across subjects; ranked BM25
//! top-10 is each engine's own and is recorded, never cross-compared.
//!
//! Usage (from the repo root):
//!     fts-rig list
//!     fts-rig run --workload fts/gutenberg40 --subject tantivy [--runs 5] [--land]
//!     fts-rig suite [--runs 5] [--land]

mod corpus;
mod queries;

use kyzo_bench_harness::opponents::sqlite::sqlite_subject;
use kyzo_bench_harness::rig::{AnswerSpec, HashKind, Phase, PreparedSubject, Rig, RigError};
use kyzo_bench_harness::{DatasetDigest, Runner, RunSet, Seed, Subject};
use std::path::Path;

/// Query-set repetitions inside the measured query phase: enough work to
/// time a 120-query set that individually runs in microseconds.
const PASSES: u32 = 20;

fn main() -> std::process::ExitCode {
    kyzo_bench_harness::rig::main::<FtsRig>()
}

struct FtsRig;

#[derive(Debug, Clone, Copy)]
struct Registered {
    id: &'static str,
    description: &'static str,
    seed: Seed,
}

fn suite() -> Vec<Registered> {
    vec![Registered {
        id: "fts/gutenberg40",
        description: "40 Gutenberg books as paragraphs; 120 queries (40 term / 30 AND / 30 OR / 20 phrase)",
        seed: Seed(27_001),
    }]
}

/// Load the corpus and regenerate its deterministic query set. Called once
/// by `generate_inputs` (to render the input files) and once by
/// `prepare_subject` (to build subject-specific query scripts) — cheap and
/// pure, so recomputing it twice is simpler than threading it between two
/// independent trait methods.
fn load(root: &Path, w: &Registered) -> std::io::Result<(Vec<corpus::Doc>, Vec<queries::Query>)> {
    let docs = corpus::load(&root.join("datasets/gutenberg"))?;
    let query_set = queries::generate(w.seed, &docs);
    Ok((docs, query_set))
}

impl Rig for FtsRig {
    type Workload = Registered;
    type Extra = ();

    const BENCH: &'static str = "fts";
    const STORY: &'static str = "kyzo#27";
    const SUBJECTS: &'static [&'static str] = &["tantivy", "fts5"];
    const USAGE: &'static str = "usage: fts-rig list | run --workload <id> --subject <s> [--runs N] [--land] [--supersedes <path> <reason>] | suite [--runs N] [--land]";

    fn workloads() -> Vec<Registered> {
        suite()
    }
    fn workload_id(w: &Registered) -> &str {
        w.id
    }
    fn workload_description(w: &Registered) -> &str {
        w.description
    }
    fn workload_seed(w: &Registered) -> Seed {
        w.seed
    }

    fn generate_inputs(
        root: &Path,
        w: &Registered,
        scratch: &Path,
    ) -> std::io::Result<Vec<DatasetDigest>> {
        let input = scratch.join("input");
        std::fs::create_dir_all(&input)?;
        let (docs, query_set) = load(root, w)?;
        let docs_tsv = input.join("docs.tsv");
        {
            use std::io::Write;
            let mut f = std::io::BufWriter::new(std::fs::File::create(&docs_tsv)?);
            for d in &docs {
                writeln!(f, "{}\t{}", d.id, d.text)?;
            }
        }
        std::fs::write(input.join("queries.tsv"), queries::to_file(&query_set))?;
        DatasetDigest::of_dir(&input)
    }

    fn prepare_subject(
        root: &Path,
        w: &Registered,
        subject_name: &str,
        _runner: &Runner,
        scratch: &Path,
        _extra: &(),
    ) -> Result<PreparedSubject, RigError> {
        let input = scratch.join("input");
        let docs_tsv = input.join("docs.tsv");
        let queries_tsv = input.join("queries.tsv");
        let (_docs, query_set) = load(root, w).map_err(RigError::Io)?;

        let matches_out = scratch.join("matches.tsv");
        let ranked_out = scratch.join("ranked.tsv");
        let answer = AnswerSpec {
            compared: (vec![matches_out.clone()], HashKind::Canonical),
            stability_only: vec![(ranked_out.clone(), HashKind::Canonical)],
        };

        match subject_name {
            "tantivy" => {
                let bin = root.join("target/release/tantivy-runner");
                if !bin.is_file() {
                    return Err(RigError::Bench(Box::new(TantivyNotBuilt(bin))));
                }
                let index_dir = scratch.join("tantivy-index");
                let base = |phase: &str| {
                    vec![
                        bin.display().to_string(),
                        "--index".into(),
                        index_dir.display().to_string(),
                        "--phase".into(),
                        phase.into(),
                    ]
                };
                let mut index_argv = base("index");
                index_argv.extend(["--docs".into(), docs_tsv.display().to_string()]);
                let mut query_argv = base("query");
                query_argv.extend([
                    "--queries".into(),
                    queries_tsv.display().to_string(),
                    "--matches".into(),
                    matches_out.display().to_string(),
                    "--ranked".into(),
                    ranked_out.display().to_string(),
                    "--passes".into(),
                    PASSES.to_string(),
                ]);
                Ok(PreparedSubject {
                    subject: Subject::Opponent(kyzo_bench_harness::Opponent {
                        name: "tantivy".into(),
                        version: "0.26.1".into(),
                        provenance: kyzo_bench_harness::Provenance::Package {
                            ecosystem: "cargo".into(),
                            package: "tantivy".into(),
                            version: "0.26.1".into(),
                        },
                    }),
                    phases: vec![
                        Phase {
                            name: "index",
                            argv: index_argv,
                            output_file: None,
                        },
                        Phase {
                            name: "query",
                            argv: query_argv,
                            output_file: Some(matches_out),
                        },
                    ],
                    reset: vec![index_dir],
                    answer,
                    notes: format!(
                        "Tantivy 0.26.1 as a library behind opponents/tantivy-runner; default \
                         tokenizer, positions indexed, multithreaded writer as shipped (1 GiB \
                         heap budget), queries built programmatically and run single-threaded. \
                         Query phase = {PASSES} passes over the set + 1 verified pass."
                    ),
                })
            }
            "fts5" => {
                let (subject, bin) = sqlite_subject(root, "sqlite-fts5")?;
                let db = scratch.join("fts5.sqlite");
                let index_sql = scratch.join("index.sql");
                let query_sql = scratch.join("query.sql");
                // `.mode ascii` with explicit separators imports raw bytes;
                // `.mode tabs` would CSV-quote-process lines starting with
                // `"` and silently merge documents.
                std::fs::write(
                    &index_sql,
                    format!(
                        ".mode ascii\n.separator \"\\t\" \"\\n\"\n\
                         PRAGMA journal_mode=WAL;\nPRAGMA synchronous=NORMAL;\n\
                         CREATE TABLE rawdocs(id INTEGER PRIMARY KEY, body TEXT);\n\
                         .import {} rawdocs\n\
                         CREATE VIRTUAL TABLE d USING fts5(body, detail=full);\n\
                         INSERT INTO d(rowid, body) SELECT id, body FROM rawdocs;\n",
                        docs_tsv.display()
                    ),
                )
                .map_err(RigError::Io)?;
                std::fs::write(
                    &query_sql,
                    queries::sqlite_query_script(&query_set, &matches_out, &ranked_out, PASSES),
                )
                .map_err(RigError::Io)?;
                let invoke = |script: &Path| {
                    vec![
                        bin.display().to_string(),
                        db.display().to_string(),
                        format!(".read {}", script.display()),
                    ]
                };
                Ok(PreparedSubject {
                    subject,
                    phases: vec![
                        Phase {
                            name: "index",
                            argv: invoke(&index_sql),
                            output_file: None,
                        },
                        Phase {
                            name: "query",
                            argv: invoke(&query_sql),
                            output_file: Some(matches_out),
                        },
                    ],
                    reset: vec![
                        db.clone(),
                        scratch.join("fts5.sqlite-wal"),
                        scratch.join("fts5.sqlite-shm"),
                    ],
                    answer,
                    notes: format!(
                        "SQLite FTS5 (3.53.3, --enable-fts5, unicode61 tokenizer, detail=full), \
                         WAL + synchronous=NORMAL, single connection single thread — FTS5 has no \
                         multithreaded mode. Query phase = {PASSES} passes over the set + 1 \
                         verified pass; ranked = ORDER BY bm25(d) LIMIT 10."
                    ),
                })
            }
            other => Err(RigError::UnknownSubject(
                other.to_owned(),
                Self::SUBJECTS.join(", "),
            )),
        }
    }

    fn headline_phase(_w: &Registered) -> &'static str {
        "query"
    }

    fn extra_notes(
        root: &Path,
        w: &Registered,
        phase_runs: &std::collections::BTreeMap<&'static str, RunSet>,
        compared: &(String, usize),
        stability_only: &[(String, usize)],
    ) -> String {
        let (docs, query_set) = match load(root, w) {
            Ok(v) => v,
            Err(_) => return String::new(),
        };
        let index_median_ms = phase_runs["index"].wall_micros_median() as f64 / 1000.0;
        let query_median_ms = phase_runs["query"].wall_micros_median() as f64 / 1000.0;
        let total_queries = (query_set.len() as u32 * (PASSES + 1)) as f64;
        let qps = total_queries / (query_median_ms / 1000.0);
        let (matches_sha, _) = compared;
        let (ranked_sha, _) = &stability_only[0];
        format!(
            "Corpus: {} paragraph docs. Headline is the query phase ({total_queries:.0} query \
             executions → {qps:.0} q/s median). Index build: median {index_median_ms:.1} ms. \
             Match-set sha {}; ranked top-10 is this engine's own BM25, sha {} — not \
             cross-comparable.",
            docs.len(),
            &matches_sha[..12.min(matches_sha.len())],
            &ranked_sha[..12.min(ranked_sha.len())],
        )
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{0} not built; run `cargo build --release -p tantivy-runner`")]
struct TantivyNotBuilt(std::path::PathBuf);
