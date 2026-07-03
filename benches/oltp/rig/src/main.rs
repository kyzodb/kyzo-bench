//! kyzo#26 — embedded OLTP vs SQLite.
//!
//! One deterministic op stream (bulk load + mixed 60/20/10/10
//! read/update/insert/delete), rendered per subject and replayed in three
//! externally timed phases: `load`, `mixed`, `dump`. The read outputs and
//! the final table dump must be byte-identical across subjects — every
//! individual read is verified by its op index, not just the final state.
//!
//! Usage (from the repo root):
//!     oltp-rig list
//!     oltp-rig run --workload oltp/r100k-o20k --subject sqlite [--runs 5] [--land]
//!     oltp-rig suite [--runs 5] [--land]

mod ops;

use kyzo_bench_harness::{
    CapPolicy, DatasetDigest, Hardware, Measurement, ResultRecord, RunSet, Runner, Seed, Subject,
    Workload, run::Warmth,
};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

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
    #[error("run this from the kyzo-bench repo root (benches/oltp not found here)")]
    NotRepoRoot,
    #[error("unknown workload {0:?}; `oltp-rig list` shows the suite")]
    UnknownWorkload(String),
    #[error("unknown subject {0:?}; subjects: kyzo, sqlite")]
    UnknownSubject(String),
    #[error("sqlite3 opponent not built at {0}; run opponents/sqlite/build.sh")]
    SqliteNotBuilt(PathBuf),
    #[error("kyzo-oltp-runner not built: {0}; run `cargo build --release -p kyzo-oltp-runner`")]
    KyzoRunnerNotBuilt(PathBuf),
    #[error("engine repo not found as sibling ../kyzo (or `git rev-parse` failed there)")]
    EngineNotFound,
    #[error(
        "refusing to land: the engine working tree is dirty; a result must name a commit \
         that exists"
    )]
    DirtyEngine,
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
        "answer instability: {subject} produced different outputs across iterations of \
         {workload}; no number is publishable for an unstable answer"
    )]
    UnstableAnswer { subject: String, workload: String },
    #[error(
        "usage: oltp-rig list | run --workload <id> --subject <s> [--runs N] [--land] | suite [--runs N] [--land]"
    )]
    Usage,
    #[error("run failed: {0}")]
    Run(#[from] kyzo_bench_harness::RunError),
    #[error("landing failed: {0}")]
    Land(#[from] kyzo_bench_harness::LandError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// The registered OLTP workloads. Seeds are fixed forever.
#[derive(Debug, Clone, Copy)]
struct Registered {
    id: &'static str,
    description: &'static str,
    seed: Seed,
    rows: u64,
    ops: u64,
}

fn suite() -> Vec<Registered> {
    vec![
        Registered {
            id: "oltp/r100k-o20k",
            description: "100k-row table, 20k mixed ops (60r/20u/10i/10d)",
            seed: Seed(26_101),
            rows: 100_000,
            ops: 20_000,
        },
        Registered {
            id: "oltp/r1m-o100k",
            description: "1M-row table, 100k mixed ops (60r/20u/10i/10d)",
            seed: Seed(26_102),
            rows: 1_000_000,
            ops: 100_000,
        },
    ]
}

fn cli() -> Result<(), RigError> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = repo_root()?;
    match args.first().map(String::as_str) {
        Some("list") => {
            for w in suite() {
                println!("{:<24} {}", w.id, w.description);
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
                for subject in ["kyzo", "sqlite"] {
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
                    "[agreement] {}: both subjects produced answer {}",
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
    if cwd.join("benches/oltp").is_dir() {
        Ok(cwd)
    } else {
        Err(RigError::NotRepoRoot)
    }
}

/// A subject readied for one iteration: per-phase argv against a fresh
/// database, plus identity and the note that lands with the record.
struct Prepared {
    subject: Subject,
    load_argv: Vec<String>,
    mixed_argv: Vec<String>,
    dump_argv: Vec<String>,
    notes: String,
}

fn prepare_subject(
    root: &Path,
    subject_name: &str,
    scratch: &Path,
    reads_out: &Path,
    dump_out: &Path,
) -> Result<Prepared, RigError> {
    match subject_name {
        "sqlite" => {
            let bin = root.join("opponents/sqlite/dist/bin/sqlite3");
            if !bin.is_file() {
                return Err(RigError::SqliteNotBuilt(bin));
            }
            let db = scratch.join("db.sqlite");
            // Each dot-command is its own argv element; sqlite3 executes
            // them in order and exits.
            let invoke = |script: &str, redirect: Option<&Path>| {
                let mut v = vec![bin.display().to_string(), db.display().to_string()];
                if let Some(p) = redirect {
                    v.push(format!(".output {}", p.display()));
                }
                v.push(format!(".read {}", scratch.join(script).display()));
                v
            };
            Ok(Prepared {
                subject: Subject::Opponent(kyzo_bench_harness::Opponent {
                    name: "sqlite".into(),
                    version: "3.53.3".into(),
                    provenance: kyzo_bench_harness::Provenance::BuiltFromSource {
                        repo: "https://www.sqlite.org/2026/sqlite-autoconf-3530300.tar.gz".into(),
                        reference: "3.53.3".into(),
                        script: "opponents/sqlite/build.sh".into(),
                    },
                }),
                load_argv: invoke("load.sql", None),
                mixed_argv: invoke("mixed.sql", Some(reads_out)),
                dump_argv: invoke("dump.sql", Some(dump_out)),
                notes: "SQLite 3.53.3 CLI, WAL + synchronous=NORMAL (declared production \
                        config), autocommit per mixed op, 1000-row transactions in the load \
                        phase, ORDER BY id dump. Single connection, single thread — SQLite's \
                        native embedded shape."
                    .into(),
            })
        }
        "kyzo" => {
            let engine_repo = root
                .parent()
                .map(|p| p.join("kyzo"))
                .filter(|p| p.is_dir())
                .ok_or(RigError::EngineNotFound)?;
            let commit = kyzo_bench_harness::EngineCommit::capture(&engine_repo)
                .ok_or(RigError::EngineNotFound)?;
            let bin = root.join("target/release/kyzo-oltp-runner");
            if !bin.is_file() {
                return Err(RigError::KyzoRunnerNotBuilt(bin));
            }
            let stream = scratch.join("stream.ops");
            let store = scratch.join("kyzo-store");
            let invoke = |phase: &str, output: Option<&Path>| {
                let mut v = vec![
                    bin.display().to_string(),
                    "--stream".into(),
                    stream.display().to_string(),
                    "--phase".into(),
                    phase.into(),
                    "--store".into(),
                    store.display().to_string(),
                ];
                if let Some(p) = output {
                    v.push("--output".into());
                    v.push(p.display().to_string());
                }
                v
            };
            let notes = format!(
                "KyzoDB at engine commit {}{}; persistent fjall store, every op through \
                 `Db::run_script` (parse included, exactly as SQLite parses each SQL \
                 statement), one script per mixed op, 1000-row batch scripts in the load \
                 phase, sorted dump. Single connection, single thread.",
                commit.commit,
                if commit.dirty {
                    " (DIRTY TREE — not publishable)"
                } else {
                    ""
                },
            );
            Ok(Prepared {
                subject: Subject::Kyzo(commit),
                load_argv: invoke("load", None),
                mixed_argv: invoke("mixed", Some(reads_out)),
                dump_argv: invoke("dump", Some(dump_out)),
                notes,
            })
        }
        other => Err(RigError::UnknownSubject(other.to_owned())),
    }
}

/// SHA-256 over the raw (order-preserving) bytes of reads + dump. Read
/// lines carry their op index, so this verifies every read individually.
fn answer_hash(reads: &Path, dump: &Path) -> std::io::Result<(String, usize)> {
    let mut hasher = Sha256::new();
    let mut rows = 0usize;
    for p in [reads, dump] {
        let bytes = std::fs::read(p)?;
        rows += bytes.iter().filter(|b| **b == b'\n').count();
        hasher.update(&bytes);
        hasher.update(b"\x1e"); // file separator: reads/dump boundary is part of the answer
    }
    Ok((
        hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
        rows,
    ))
}

fn run_one(
    root: &Path,
    w: Registered,
    subject_name: &str,
    opt: &Options,
) -> Result<String, RigError> {
    let scratch = root
        .join("target/oltp-scratch")
        .join(w.id.replace('/', "_"))
        .join(subject_name);
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch)?;

    // Render the one stream into both subject languages; digest what the
    // subject actually consumes.
    let stream = ops::generate(w.seed, w.rows, w.ops);
    std::fs::write(scratch.join("load.sql"), ops::sqlite_load(&stream))?;
    std::fs::write(scratch.join("mixed.sql"), ops::sqlite_mixed(&stream))?;
    std::fs::write(scratch.join("dump.sql"), ops::sqlite_dump())?;
    std::fs::write(scratch.join("stream.ops"), ops::kyzo_stream(&stream))?;
    let datasets = DatasetDigest::of_dir(&scratch)?;

    let reads_out = scratch.join("reads.tsv");
    let dump_out = scratch.join("dump.tsv");
    let caps = CapPolicy::house();
    let runner = Runner::new(caps, scratch.clone());
    let Prepared {
        subject,
        load_argv,
        mixed_argv,
        dump_argv,
        notes,
    } = prepare_subject(root, subject_name, &scratch, &reads_out, &dump_out)?;

    eprintln!(
        "[{}] {} — warm-up + {} iterations (fresh db each)…",
        subject.label(),
        w.id,
        opt.runs
    );

    let mut load_runs: Vec<Measurement> = Vec::with_capacity(opt.runs);
    let mut mixed_runs: Vec<Measurement> = Vec::with_capacity(opt.runs);
    let mut dump_ms_last = 0.0f64;
    let mut answer: Option<(String, usize)> = None;

    // Warm-up iteration (discarded), then measured iterations. Every
    // iteration starts from a fresh database directory; the answer must be
    // identical every time, warm-up included.
    for i in 0..=opt.runs {
        let warm = if i == 0 { Warmth::Cold } else { Warmth::Warm };
        let db_file = scratch.join("db.sqlite");
        let _ = std::fs::remove_file(&db_file);
        let _ = std::fs::remove_file(scratch.join("db.sqlite-wal"));
        let _ = std::fs::remove_file(scratch.join("db.sqlite-shm"));
        let _ = std::fs::remove_dir_all(scratch.join("kyzo-store"));

        let m_load = runner.run(&load_argv, None, warm)?;
        let m_mixed = runner.run(&mixed_argv, Some(&reads_out), warm)?;
        let m_dump = runner.run(&dump_argv, Some(&dump_out), warm)?;
        dump_ms_last = m_dump.wall_micros as f64 / 1000.0;

        let a = answer_hash(&reads_out, &dump_out)?;
        match &answer {
            None => answer = Some(a),
            Some(prev) if *prev != a => {
                return Err(RigError::UnstableAnswer {
                    subject: subject.label(),
                    workload: w.id.to_owned(),
                });
            }
            Some(_) => {}
        }
        if i > 0 {
            load_runs.push(m_load);
            mixed_runs.push(m_mixed);
        }
    }
    let (answer_sha, answer_rows) = answer.expect("at least the warm-up ran");

    let load_set = RunSet {
        measurements: load_runs,
    };
    let mixed_set = RunSet {
        measurements: mixed_runs,
    };
    let load_median_ms = load_set.wall_micros_median() as f64 / 1000.0;
    let mixed_median_ms = mixed_set.wall_micros_median() as f64 / 1000.0;
    let ops_per_sec = w.ops as f64 / (mixed_median_ms / 1000.0);
    let rows_per_sec = w.rows as f64 / (load_median_ms / 1000.0);

    // The headline record is the mixed phase; load and dump numbers ride in
    // the notes so the record stays one-metric-one-record.
    let record = ResultRecord {
        bench: "oltp".to_owned(),
        story: "kyzo#26".to_owned(),
        subject: subject.clone(),
        rig: ResultRecord::rig_commit(),
        workload: Workload {
            id: w.id.to_owned(),
            description: w.description.to_owned(),
            seed: w.seed,
            correctness_sha256: Some(answer_sha.clone()),
        },
        datasets,
        hardware: Hardware::capture(),
        caps,
        runs: mixed_set,
        date: ResultRecord::today_utc(),
        notes: format!(
            "{notes} Mixed phase is the headline ({} ops → {ops_per_sec:.0} ops/s median). \
             Load: {} rows in median {load_median_ms:.1} ms ({rows_per_sec:.0} rows/s). \
             Dump (unmeasured sanity phase): {dump_ms_last:.1} ms. Answer rows: {answer_rows}.",
            w.ops, w.rows,
        ),
        supersedes: None,
    };

    let (min, max) = record.runs.wall_micros_min_max();
    eprintln!(
        "[{}] {}: mixed median {:.1} ms ({:.0} ops/s; min {:.1} / max {:.1}), load median {:.1} ms ({:.0} rows/s), peak RSS {} KiB, answer {}",
        subject.label(),
        w.id,
        mixed_median_ms,
        ops_per_sec,
        min as f64 / 1000.0,
        max as f64 / 1000.0,
        load_median_ms,
        rows_per_sec,
        record.runs.peak_rss_kib_max(),
        &answer_sha[..12],
    );

    if opt.land {
        if let Subject::Kyzo(c) = &record.subject
            && c.dirty
        {
            return Err(RigError::DirtyEngine);
        }
        let path = record.land(&root.join("results"))?;
        eprintln!("landed: {}", path.display());
    } else {
        println!(
            "{}",
            serde_json::to_string_pretty(&record).expect("record serializes")
        );
    }
    Ok(answer_sha)
}
