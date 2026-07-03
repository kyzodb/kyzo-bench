//! kyzo#22 — recursive Datalog vs Souffle.
//!
//! Usage (from the repo root):
//!     datalog-rig list
//!     datalog-rig run --workload tc/sparse-n2k-m6k --subject souffle [--runs 5] [--threads N] [--land]
//!     datalog-rig suite [--runs 5] [--threads N] [--land]
//!
//! `--land` writes an append-only [`ResultRecord`] into `results/`;
//! without it the record is printed to stdout for inspection.

mod generate;
mod souffle;
mod workloads;

use kyzo_bench_harness::canon::{self, canonical_answer};
use kyzo_bench_harness::{CapPolicy, DatasetDigest, Hardware, ResultRecord, RunSet, Runner};
use souffle::{Mode, Souffle};
use std::path::{Path, PathBuf};
use workloads::Registered;

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
    #[error("run this from the kyzo-bench repo root (benches/datalog not found here)")]
    NotRepoRoot,
    #[error("unknown workload {0:?}; `datalog-rig list` shows the suite")]
    UnknownWorkload(String),
    #[error("unknown subject {0:?}; subjects: kyzo, souffle, souffle-compiled")]
    UnknownSubject(String),
    #[error("engine repo not found as sibling ../kyzo (or `git rev-parse` failed there)")]
    EngineNotFound,
    #[error("kyzo-runner not built: {0}; run `cargo build --release -p kyzo-runner` first")]
    KyzoRunnerNotBuilt(PathBuf),
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
        "usage: datalog-rig list | run --workload <id> --subject <s> [--runs N] [--threads N] [--land] | suite [--runs N] [--threads N] [--land]"
    )]
    Usage,
    #[error("{0}")]
    Souffle(#[from] souffle::SouffleError),
    #[error("run failed: {0}")]
    Run(#[from] kyzo_bench_harness::RunError),
    #[error(
        "answer instability: {subject} produced different answers across runs of {workload} \
         ({first} then {second}); no number is publishable for an unstable answer"
    )]
    UnstableAnswer {
        subject: String,
        workload: String,
        first: String,
        second: String,
    },
    #[error("landing failed: {0}")]
    Land(#[from] kyzo_bench_harness::LandError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

fn cli() -> Result<(), RigError> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = repo_root()?;
    match args.first().map(String::as_str) {
        Some("list") => {
            for w in workloads::suite() {
                println!("{:<28} {}", w.id, w.description);
            }
            Ok(())
        }
        Some("run") => {
            let opt = Options::parse(&args[1..])?;
            let workload = opt.workload.as_deref().ok_or(RigError::Usage)?;
            let subject = opt.subject.as_deref().ok_or(RigError::Usage)?;
            let w = Registered::find(workload)
                .ok_or_else(|| RigError::UnknownWorkload(workload.to_owned()))?;
            run_one(&root, w, subject, &opt)?;
            Ok(())
        }
        Some("suite") => {
            let opt = Options::parse(&args[1..])?;
            for w in workloads::suite() {
                // Cross-subject agreement is enforced, not hoped for: every
                // subject must produce the identical canonical answer, or
                // the whole workload is refused as a result.
                let mut answers: Vec<(&str, String)> = Vec::new();
                for subject in ["kyzo", "souffle", "souffle-compiled"] {
                    let hash = run_one(&root, w, subject, &opt)?;
                    answers.push((subject, hash));
                }
                let (first_subject, first_hash) = &answers[0];
                for (subject, hash) in &answers[1..] {
                    if hash != first_hash {
                        return Err(RigError::CrossSubjectDisagreement {
                            workload: w.id.to_owned(),
                            a: format!("{first_subject}={first_hash}"),
                            b: format!("{subject}={hash}"),
                        });
                    }
                }
                eprintln!(
                    "[agreement] {}: all {} subjects produced answer {}",
                    w.id,
                    answers.len(),
                    &first_hash[..12]
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
    threads: usize,
    land: bool,
}

impl Options {
    fn parse(args: &[String]) -> Result<Options, RigError> {
        let mut o = Options {
            workload: None,
            subject: None,
            runs: 5,
            threads: std::thread::available_parallelism().map_or(1, |n| n.get()),
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
                "--threads" => {
                    o.threads = it
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
    if cwd.join("benches/datalog").is_dir() {
        Ok(cwd)
    } else {
        Err(RigError::NotRepoRoot)
    }
}

/// A subject readied for measurement: its identity, the argv the runner
/// times, and the configuration note that lands with the record.
struct Prepared {
    subject: kyzo_bench_harness::Subject,
    argv: Vec<String>,
    notes: String,
}

#[allow(clippy::too_many_arguments)]
fn prepare_subject(
    root: &Path,
    w: Registered,
    subject_name: &str,
    opt: &Options,
    runner: &Runner,
    facts: &Path,
    outs: &Path,
    scratch: &Path,
    output_file: &Path,
) -> Result<Prepared, RigError> {
    match subject_name {
        "souffle" | "souffle-compiled" => {
            let mode = if subject_name == "souffle" {
                Mode::Interpreted
            } else {
                Mode::Compiled
            };
            let engine = Souffle::locate(root, mode, opt.threads)?;
            let program = root
                .join("benches/datalog/programs")
                .join(w.souffle_program());
            let argv = engine.prepare(runner, &program, facts, outs, scratch)?;
            Ok(Prepared {
                subject: engine.subject(),
                argv,
                notes: engine.notes(),
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
            let runner_bin = root.join("target/release/kyzo-runner");
            if !runner_bin.is_file() {
                return Err(RigError::KyzoRunnerNotBuilt(runner_bin));
            }
            let program = root.join("benches/datalog/programs").join(w.kyzo_program());
            let argv = vec![
                runner_bin.display().to_string(),
                "--facts".into(),
                facts.display().to_string(),
                "--relations".into(),
                w.relations_spec().to_owned(),
                "--program".into(),
                program.display().to_string(),
                "--output".into(),
                output_file.display().to_string(),
                "--store".into(),
                scratch.join("kyzo-store").display().to_string(),
            ];
            let notes = format!(
                "KyzoDB at engine commit {}{}; end-to-end per run, same shape as the \
                 Souffle invocation: fresh store, TSV facts loaded through \
                 `Db::run_script` mutations, query evaluated, answer written as TSV. \
                 The engine persists facts durably (fjall LSM) inside the measured \
                 window; Souffle holds them in memory — same window, different \
                 obligations, stated rather than hidden.",
                commit.commit,
                if commit.dirty {
                    " (DIRTY TREE — not publishable)"
                } else {
                    ""
                },
            );
            Ok(Prepared {
                subject: kyzo_bench_harness::Subject::Kyzo(commit),
                argv,
                notes,
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
    // Scratch: facts and outputs, regenerated fresh every invocation.
    let scratch = root
        .join("target/datalog-scratch")
        .join(w.id.replace('/', "_"))
        .join(subject_name.replace(['@', '/'], "_"));
    let facts = scratch.join("facts");
    let outs = scratch.join("out");
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&outs)?;
    w.generate(&facts, root)?;
    let datasets = DatasetDigest::of_dir(&facts)?;

    let output_file = outs.join(format!("{}.csv", w.output_relation()));

    let caps = CapPolicy::house();
    let runner = Runner::new(caps, scratch.clone());
    let Prepared {
        subject,
        argv,
        notes,
    } = prepare_subject(
        root,
        w,
        subject_name,
        opt,
        &runner,
        &facts,
        &outs,
        &scratch,
        &output_file,
    )?;

    eprintln!(
        "[{}] {} — warm-up + {} runs…",
        subject.label(),
        w.id,
        opt.runs
    );
    let mut measurements = Vec::with_capacity(opt.runs);
    let mut canonical: Option<canon::CanonicalAnswer> = None;
    // Warm-up (discarded), then measured runs; the answer must be identical
    // across every run, warm-up included.
    for i in 0..=opt.runs {
        let warm = if i == 0 {
            kyzo_bench_harness::run::Warmth::Cold
        } else {
            kyzo_bench_harness::run::Warmth::Warm
        };
        let m = runner.run(&argv, Some(&output_file), warm)?;
        let answer = canonical_answer(&output_file)?;
        match &canonical {
            None => canonical = Some(answer),
            Some(prev) if *prev != answer => {
                return Err(RigError::UnstableAnswer {
                    subject: subject.label(),
                    workload: w.id.to_owned(),
                    first: prev.sha256.clone(),
                    second: answer.sha256,
                });
            }
            Some(_) => {}
        }
        if i > 0 {
            measurements.push(m);
        }
    }
    let canonical = canonical.expect("at least the warm-up ran");

    let runs = RunSet { measurements };
    let record = ResultRecord {
        bench: "datalog".to_owned(),
        story: "kyzo#22".to_owned(),
        subject: subject.clone(),
        rig: ResultRecord::rig_commit(),
        workload: w.as_workload(Some(canonical.sha256.clone())),
        datasets,
        hardware: Hardware::capture(),
        caps,
        runs,
        date: ResultRecord::today_utc(),
        notes: format!("{notes} Answer rows: {}.", canonical.rows),
        supersedes: None,
    };

    let median_ms = record.runs.wall_micros_median() as f64 / 1000.0;
    let (min, max) = record.runs.wall_micros_min_max();
    eprintln!(
        "[{}] {}: median {:.1} ms (min {:.1} / max {:.1}), peak RSS {} KiB, {} rows, answer {}",
        subject.label(),
        w.id,
        median_ms,
        min as f64 / 1000.0,
        max as f64 / 1000.0,
        record.runs.peak_rss_kib_max(),
        canonical.rows,
        &canonical.sha256[..12],
    );

    if opt.land {
        if let kyzo_bench_harness::Subject::Kyzo(c) = &record.subject
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
    Ok(canonical.sha256)
}
