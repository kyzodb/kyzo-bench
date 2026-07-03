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

use kyzo_bench_harness::{
    CanonicalAnswer, CapPolicy, DatasetDigest, Hardware, Measurement, ResultRecord, RunSet, Runner,
    Seed, Subject, Workload, canonical_answer, run::Warmth,
};
use std::path::{Path, PathBuf};

/// Query-set repetitions inside the measured query phase: enough work to
/// time a 120-query set that individually runs in microseconds.
const PASSES: u32 = 20;

fn main() -> std::process::ExitCode {
    match cli() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[derive(Debug, thiserror::Error)]
enum RigError {
    #[error("run this from the kyzo-bench repo root (benches/fts not found here)")]
    NotRepoRoot,
    #[error("unknown workload {0:?}; `fts-rig list` shows the suite")]
    UnknownWorkload(String),
    #[error("unknown subject {0:?}; subjects: tantivy, fts5")]
    UnknownSubject(String),
    #[error("{0} not built; run `cargo build --release -p tantivy-runner`")]
    TantivyNotBuilt(PathBuf),
    #[error("sqlite3 opponent not built at {0}; run opponents/sqlite/build.sh")]
    SqliteNotBuilt(PathBuf),
    #[error(
        "cross-subject disagreement on {workload}: {a} vs {b}; a benchmark number for a \
         disputed answer is not a result — file the defect"
    )]
    CrossSubjectDisagreement {
        workload: String,
        a: String,
        b: String,
    },
    #[error(
        "answer instability: {subject} produced different match sets across iterations of \
         {workload}; no number is publishable for an unstable answer"
    )]
    UnstableAnswer { subject: String, workload: String },
    #[error(
        "usage: fts-rig list | run --workload <id> --subject <s> [--runs N] [--land] | suite [--runs N] [--land]"
    )]
    Usage,
    #[error("run failed: {0}")]
    Run(#[from] kyzo_bench_harness::RunError),
    #[error("landing failed: {0}")]
    Land(#[from] kyzo_bench_harness::LandError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

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

fn cli() -> Result<(), RigError> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = repo_root()?;
    match args.first().map(String::as_str) {
        Some("list") => {
            for w in suite() {
                println!("{:<20} {}", w.id, w.description);
            }
            Ok(())
        }
        Some("run") => {
            let opt = Options::parse(&args[1..])?;
            let workload = opt.workload.as_deref().ok_or(RigError::Usage)?;
            let subject = opt.subject.as_deref().ok_or(RigError::Usage)?;
            let w = suite()
                .into_iter()
                .find(|w| w.id == workload)
                .ok_or_else(|| RigError::UnknownWorkload(workload.to_owned()))?;
            run_one(&root, w, subject, &opt)?;
            Ok(())
        }
        Some("suite") => {
            let opt = Options::parse(&args[1..])?;
            for w in suite() {
                let mut answers: Vec<(&str, String)> = Vec::new();
                for subject in ["tantivy", "fts5"] {
                    let hash = run_one(&root, w, subject, &opt)?;
                    answers.push((subject, hash));
                }
                let (sa, ha) = &answers[0];
                let (sb, hb) = &answers[1];
                if ha != hb {
                    return Err(RigError::CrossSubjectDisagreement {
                        workload: w.id.to_owned(),
                        a: format!("{sa}={ha}"),
                        b: format!("{sb}={hb}"),
                    });
                }
                eprintln!(
                    "[agreement] {}: all subjects produced match sets {}",
                    w.id,
                    &ha[..12]
                );
            }
            Ok(())
        }
        _ => Err(RigError::Usage),
    }
}

struct Options {
    workload: Option<String>,
    subject: Option<String>,
    runs: usize,
    land: bool,
}

impl Options {
    fn parse(args: &[String]) -> Result<Options, RigError> {
        let mut o = Options {
            workload: None,
            subject: None,
            runs: 5,
            land: false,
        };
        let mut it = args.iter();
        while let Some(a) = it.next() {
            match a.as_str() {
                "--workload" => o.workload = Some(it.next().ok_or(RigError::Usage)?.clone()),
                "--subject" => o.subject = Some(it.next().ok_or(RigError::Usage)?.clone()),
                "--runs" => {
                    o.runs = it
                        .next()
                        .and_then(|n| n.parse().ok())
                        .filter(|n| *n > 0)
                        .ok_or(RigError::Usage)?;
                }
                "--land" => o.land = true,
                _ => return Err(RigError::Usage),
            }
        }
        Ok(o)
    }
}

fn repo_root() -> Result<PathBuf, RigError> {
    let cwd = std::env::current_dir().map_err(RigError::Io)?;
    if cwd.join("benches/fts").is_dir() {
        Ok(cwd)
    } else {
        Err(RigError::NotRepoRoot)
    }
}

struct Prepared {
    subject: Subject,
    index_argv: Vec<String>,
    query_argv: Vec<String>,
    /// Paths whose deletion resets the subject to unindexed.
    reset: Vec<PathBuf>,
    notes: String,
}

#[allow(clippy::too_many_arguments)]
fn prepare_subject(
    root: &Path,
    subject_name: &str,
    scratch: &Path,
    docs_tsv: &Path,
    queries_tsv: &Path,
    query_set: &[queries::Query],
    matches_out: &Path,
    ranked_out: &Path,
) -> Result<Prepared, RigError> {
    match subject_name {
        "tantivy" => {
            let bin = root.join("target/release/tantivy-runner");
            if !bin.is_file() {
                return Err(RigError::TantivyNotBuilt(bin));
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
            Ok(Prepared {
                subject: Subject::Opponent(kyzo_bench_harness::Opponent {
                    name: "tantivy".into(),
                    version: "0.26.1".into(),
                    provenance: kyzo_bench_harness::Provenance::Package {
                        ecosystem: "cargo".into(),
                        package: "tantivy".into(),
                        version: "0.26.1".into(),
                    },
                }),
                index_argv,
                query_argv,
                reset: vec![index_dir],
                notes: format!(
                    "Tantivy 0.26.1 as a library behind opponents/tantivy-runner; default \
                     tokenizer, positions indexed, multithreaded writer as shipped (1 GiB \
                     heap budget), queries built programmatically and run single-threaded. \
                     Query phase = {PASSES} passes over the set + 1 verified pass."
                ),
            })
        }
        "fts5" => {
            let bin = root.join("opponents/sqlite/dist/bin/sqlite3");
            if !bin.is_file() {
                return Err(RigError::SqliteNotBuilt(bin));
            }
            let db = scratch.join("fts5.sqlite");
            let index_sql = scratch.join("index.sql");
            let query_sql = scratch.join("query.sql");
            // `.mode ascii` with explicit separators imports raw bytes;
            // `.mode tabs` would CSV-quote-process lines starting with `"`
            // and silently merge documents.
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
            )?;
            std::fs::write(
                &query_sql,
                queries::sqlite_query_script(query_set, matches_out, ranked_out, PASSES),
            )?;
            let invoke = |script: &Path| {
                vec![
                    bin.display().to_string(),
                    db.display().to_string(),
                    format!(".read {}", script.display()),
                ]
            };
            Ok(Prepared {
                subject: Subject::Opponent(kyzo_bench_harness::Opponent {
                    name: "sqlite-fts5".into(),
                    version: "3.53.3".into(),
                    provenance: kyzo_bench_harness::Provenance::BuiltFromSource {
                        repo: "https://www.sqlite.org/2026/sqlite-autoconf-3530300.tar.gz".into(),
                        reference: "3.53.3".into(),
                        script: "opponents/sqlite/build.sh".into(),
                    },
                }),
                index_argv: invoke(&index_sql),
                query_argv: invoke(&query_sql),
                reset: vec![
                    db.clone(),
                    scratch.join("fts5.sqlite-wal"),
                    scratch.join("fts5.sqlite-shm"),
                ],
                notes: format!(
                    "SQLite FTS5 (3.53.3, --enable-fts5, unicode61 tokenizer, detail=full), \
                     WAL + synchronous=NORMAL, single connection single thread — FTS5 has no \
                     multithreaded mode. Query phase = {PASSES} passes over the set + 1 \
                     verified pass; ranked = ORDER BY bm25(d) LIMIT 10."
                ),
            })
        }
        other => Err(RigError::UnknownSubject(other.to_owned())),
    }
}

fn run_one(
    root: &Path,
    w: Registered,
    subject_name: &str,
    opt: &Options,
) -> Result<String, RigError> {
    let scratch = root
        .join("target/fts-scratch")
        .join(w.id.replace('/', "_"))
        .join(subject_name);
    let input = scratch.join("input");
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&input)?;

    let docs = corpus::load(&root.join("datasets/gutenberg")).map_err(RigError::Io)?;
    let query_set = queries::generate(w.seed, &docs);
    let docs_tsv = input.join("docs.tsv");
    {
        use std::io::Write;
        let mut f = std::io::BufWriter::new(std::fs::File::create(&docs_tsv)?);
        for d in &docs {
            writeln!(f, "{}\t{}", d.id, d.text)?;
        }
    }
    let queries_tsv = input.join("queries.tsv");
    std::fs::write(&queries_tsv, queries::to_file(&query_set))?;
    let datasets = DatasetDigest::of_dir(&input)?;

    let matches_out = scratch.join("matches.tsv");
    let ranked_out = scratch.join("ranked.tsv");
    let caps = CapPolicy::house();
    let runner = Runner::new(caps, scratch.clone());
    let Prepared {
        subject,
        index_argv,
        query_argv,
        reset,
        notes,
    } = prepare_subject(
        root,
        subject_name,
        &scratch,
        &docs_tsv,
        &queries_tsv,
        &query_set,
        &matches_out,
        &ranked_out,
    )?;

    eprintln!(
        "[{}] {} — {} docs, {} queries; warm-up + {} iterations…",
        subject.label(),
        w.id,
        docs.len(),
        query_set.len(),
        opt.runs
    );

    let mut index_runs: Vec<Measurement> = Vec::with_capacity(opt.runs);
    let mut query_runs: Vec<Measurement> = Vec::with_capacity(opt.runs);
    let mut matches_answer: Option<CanonicalAnswer> = None;
    let mut ranked_answer: Option<CanonicalAnswer> = None;

    for i in 0..=opt.runs {
        let warm = if i == 0 { Warmth::Cold } else { Warmth::Warm };
        for p in &reset {
            let _ = std::fs::remove_dir_all(p);
            let _ = std::fs::remove_file(p);
        }
        let m_index = runner.run(&index_argv, None, warm)?;
        let m_query = runner.run(&query_argv, Some(&matches_out), warm)?;

        let matches = canonical_answer(&matches_out)?;
        let ranked = canonical_answer(&ranked_out)?;
        let stable = match (&matches_answer, &ranked_answer) {
            (None, None) => {
                matches_answer = Some(matches);
                ranked_answer = Some(ranked);
                true
            }
            (Some(pm), Some(pr)) => *pm == matches && *pr == ranked,
            _ => unreachable!("both set together"),
        };
        if !stable {
            return Err(RigError::UnstableAnswer {
                subject: subject.label(),
                workload: w.id.to_owned(),
            });
        }
        if i > 0 {
            index_runs.push(m_index);
            query_runs.push(m_query);
        }
    }
    let matches_answer = matches_answer.expect("warm-up ran");
    let ranked_answer = ranked_answer.expect("warm-up ran");

    let index_set = RunSet {
        measurements: index_runs,
    };
    let query_set_runs = RunSet {
        measurements: query_runs,
    };
    let index_median_ms = index_set.wall_micros_median() as f64 / 1000.0;
    let query_median_ms = query_set_runs.wall_micros_median() as f64 / 1000.0;
    let total_queries = (query_set.len() as u32 * (PASSES + 1)) as f64;
    let qps = total_queries / (query_median_ms / 1000.0);

    let record = ResultRecord {
        bench: "fts".to_owned(),
        story: "kyzo#27".to_owned(),
        subject: subject.clone(),
        rig: ResultRecord::rig_commit(),
        workload: Workload {
            id: w.id.to_owned(),
            description: w.description.to_owned(),
            seed: w.seed,
            correctness_sha256: Some(matches_answer.sha256.clone()),
        },
        datasets,
        hardware: Hardware::capture(),
        caps,
        runs: query_set_runs,
        date: ResultRecord::today_utc(),
        notes: format!(
            "{notes} Corpus: {} paragraph docs. Headline is the query phase \
             ({total_queries:.0} query executions → {qps:.0} q/s median). Index build: \
             median {index_median_ms:.1} ms. Match-set rows: {} (sha {}); ranked top-10 is \
             this engine's own BM25, sha {} — not cross-comparable.",
            docs.len(),
            matches_answer.rows,
            &matches_answer.sha256[..12],
            &ranked_answer.sha256[..12],
        ),
        supersedes: None,
    };

    let (min, max) = record.runs.wall_micros_min_max();
    eprintln!(
        "[{}] {}: query median {:.1} ms ({:.0} q/s; min {:.1} / max {:.1}), index median {:.1} ms, peak RSS {} KiB, match sets {}",
        subject.label(),
        w.id,
        query_median_ms,
        qps,
        min as f64 / 1000.0,
        max as f64 / 1000.0,
        index_median_ms,
        record.runs.peak_rss_kib_max(),
        &matches_answer.sha256[..12],
    );

    if opt.land {
        let path = record.land(&root.join("results"))?;
        eprintln!("landed: {}", path.display());
    } else {
        println!(
            "{}",
            serde_json::to_string_pretty(&record).expect("record serializes")
        );
    }
    Ok(matches_answer.sha256)
}
