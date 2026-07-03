//! The shared subprocess-rig runner: everything that was identical text in
//! `datalog-rig`, `oltp-rig`, and `fts-rig` — `RigError`, CLI dispatch,
//! `repo_root`, the warm-up-then-measured-runs loop with answer-stability
//! checking, the `suite` cross-subject agreement loop, and the
//! land-or-print-with-dirty-gate ending — lives here exactly once.
//!
//! A bench implements [`Rig`] with only what is genuinely bench-specific:
//! its workload registry, how it renders inputs, and how it builds each
//! subject's argv. Everything else is [`main`].

use crate::canon::{canonical_answer, raw_answer};
use crate::dataset::DatasetDigest;
use crate::hardware::Hardware;
use crate::record::ResultRecord;
use crate::run::{CapPolicy, Measurement, RunSet, Runner, Warmth};
use crate::seed::Seed;
use crate::subject::Subject;
use crate::workload::Workload;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One externally-timed phase of a workload run against one subject.
pub struct Phase {
    /// Stable within one [`PreparedSubject`]; used to pick the headline
    /// phase and to report per-phase timings.
    pub name: &'static str,
    pub argv: Vec<String>,
    /// The file the subject writes its answer to, if not stdout. Hashed by
    /// [`Runner::run`] into `Measurement::output_sha256` regardless; this is
    /// a *separate* concern from [`AnswerSpec`], which may hash different
    /// files entirely (e.g. fts's ranked-output side file).
    pub output_file: Option<PathBuf>,
}

/// How one phase's or several phases' output files become a single answer
/// identity. There is no interface across the two kinds because they mean
/// different things: a set has no order, a log has one.
pub enum HashKind {
    /// Sorted-unique-lines SHA-256 ([`canonical_answer`]) — the answer is a
    /// set, row order is an evaluation accident. Always names exactly one
    /// file; there is no proven need yet to hash a canonical answer that
    /// spans multiple files, so this does not speculatively support it.
    Canonical,
    /// Raw-byte SHA-256 over one or more files concatenated in declared
    /// order ([`raw_answer`]) — order is part of the answer.
    Raw,
}

/// Which files must agree, and which must merely reproduce themselves.
pub struct AnswerSpec {
    /// The file(s) whose hash becomes the workload's `correctness_sha256`
    /// and what `suite` compares across subjects.
    pub compared: (Vec<PathBuf>, HashKind),
    /// File(s) that must independently reproduce identical bytes across
    /// every repeated run (warm-up included) but are excluded from
    /// `compared` — e.g. fts's ranked BM25 top-10, which is each engine's
    /// own and is never cross-compared. Folding a never-cross-compared
    /// file into `compared` is the exact fairness bug this split exists to
    /// make structurally impossible.
    pub stability_only: Vec<(PathBuf, HashKind)>,
}

/// A subject fully readied to run every phase of one workload.
pub struct PreparedSubject {
    pub subject: Subject,
    pub phases: Vec<Phase>,
    /// Paths to remove before every iteration (warm-up included) so each
    /// iteration starts from the same reset state — e.g. oltp's database
    /// files, fts's index directory. Empty for benches with no reusable
    /// on-disk state between iterations (e.g. datalog).
    pub reset: Vec<PathBuf>,
    pub answer: AnswerSpec,
    pub notes: String,
}

/// Errors every subprocess-rig CLI can produce. `Bench` is the one escape
/// hatch for a genuinely bench-specific failure (e.g. Souffle's own
/// compile-and-synthesize error) — box it at the one call site that has the
/// concrete type, do not add a second payload-carrying variant here for a
/// second bench's error before it actually recurs.
#[derive(Debug, thiserror::Error)]
pub enum RigError {
    #[error("run this from the kyzo-bench repo root (benches/{0} not found here)")]
    NotRepoRoot(&'static str),
    #[error("unknown workload {0:?}; `list` shows the suite")]
    UnknownWorkload(String),
    #[error("unknown subject {0:?}; subjects: {1}")]
    UnknownSubject(String, String),
    #[error(transparent)]
    EngineLocate(#[from] crate::subject::EngineLocateError),
    #[error(transparent)]
    Sqlite(#[from] crate::opponents::sqlite::SqliteError),
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
        "answer instability: {subject} produced different output across runs of {workload}; \
         no number is publishable for an unstable answer"
    )]
    UnstableAnswer { subject: String, workload: String },
    #[error("{0}")]
    Usage(&'static str),
    #[error("run failed: {0}")]
    Run(#[from] crate::run::RunError),
    #[error("landing failed: {0}")]
    Land(#[from] crate::record::LandError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Bench(Box<dyn std::error::Error + Send + Sync>),
}

/// What one subprocess-timed comparative bench provides. Everything not
/// listed here — CLI parsing, the measured-run loop, cross-subject
/// agreement, landing — is [`main`]'s, not the implementor's.
pub trait Rig {
    type Workload: Copy;
    /// Bench-specific CLI state beyond `--workload/--subject/--runs/--land`
    /// (datalog's `--threads` is the only current occupant). `()` for a
    /// bench with no extra flags.
    type Extra: Default;

    /// Matches the directory name under `benches/` and the `bench` field
    /// landed in every record from this rig.
    const BENCH: &'static str;
    /// The story id landed in every record, e.g. `"kyzo#22"`.
    const STORY: &'static str;
    /// Fixed subject list, in the order `suite` runs and compares them.
    const SUBJECTS: &'static [&'static str];
    /// The full `usage: ...` line printed on a CLI error.
    const USAGE: &'static str;

    fn workloads() -> Vec<Self::Workload>;
    fn workload_id(w: &Self::Workload) -> &str;
    fn workload_description(w: &Self::Workload) -> &str;
    fn workload_seed(w: &Self::Workload) -> Seed;

    /// Parse one flag `main`'s built-ins didn't recognize. `Ok(true)` if
    /// `name` was consumed (and, if it takes a value, `it.next()` was
    /// called for it); `Ok(false)` if `name` isn't one of this bench's
    /// flags either, which the shared parser turns into a usage error.
    /// Return `Err` directly for a recognized flag with a bad value, using
    /// `RigError::Usage(Self::USAGE)` — same contract as the shared
    /// parser's own flags.
    fn parse_extra_flag(
        name: &str,
        it: &mut std::slice::Iter<'_, String>,
        extra: &mut Self::Extra,
    ) -> Result<bool, RigError> {
        let _ = (name, it, extra);
        Ok(false)
    }

    /// Regenerate every input file for `w` under `scratch` (already created
    /// fresh); return the dataset digests of whatever was written.
    fn generate_inputs(
        root: &Path,
        w: &Self::Workload,
        scratch: &Path,
    ) -> std::io::Result<Vec<DatasetDigest>>;

    /// Build every phase's argv for `subject_name` against the inputs
    /// `generate_inputs` just wrote. `runner` is available for untimed but
    /// still-capped setup (e.g. Souffle's compiled-mode synthesis step).
    fn prepare_subject(
        root: &Path,
        w: &Self::Workload,
        subject_name: &str,
        runner: &Runner,
        scratch: &Path,
        extra: &Self::Extra,
    ) -> Result<PreparedSubject, RigError>;

    /// The name of the [`Phase`] whose [`RunSet`] becomes the landed
    /// record's headline `runs` (oltp: `"mixed"`; fts: `"query"`; datalog's
    /// one phase names itself).
    fn headline_phase(w: &Self::Workload) -> &'static str;

    /// Extra prose appended to the landed record's `notes`, after
    /// `PreparedSubject::notes` — e.g. oltp's ops/rows-per-sec line, fts's
    /// q/s and match-row-count line. Given every phase's full `RunSet` so
    /// non-headline phases (index build, load, dump) can be reported too,
    /// plus the final `(sha256, rows)` of the compared answer and of each
    /// `stability_only` entry in declared order, since prose like fts's
    /// "ranked sha, not cross-comparable" needs the number the run just
    /// produced, not a value known before any run happened. `root` is
    /// available for the rare case a bench needs to recompute a count
    /// (fts's query total) from the same deterministic inputs it already
    /// regenerated in `prepare_subject`.
    fn extra_notes(
        root: &Path,
        w: &Self::Workload,
        phase_runs: &BTreeMap<&'static str, RunSet>,
        compared: &(String, usize),
        stability_only: &[(String, usize)],
    ) -> String {
        let _ = (root, w, phase_runs, compared, stability_only);
        String::new()
    }
}

/// The one entry point every migrated rig's `main` calls.
pub fn main<R: Rig>() -> std::process::ExitCode {
    match cli::<R>() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn cli<R: Rig>() -> Result<(), RigError> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = repo_root::<R>()?;
    match args.first().map(String::as_str) {
        Some("list") => {
            for w in R::workloads() {
                println!("{:<28} {}", R::workload_id(&w), R::workload_description(&w));
            }
            Ok(())
        }
        Some("run") => {
            let opt = Options::<R>::parse(&args[1..])?;
            let workload = opt.workload.as_deref().ok_or(RigError::Usage(R::USAGE))?;
            let subject = opt.subject.as_deref().ok_or(RigError::Usage(R::USAGE))?;
            let w = find_workload::<R>(workload)?;
            run_one::<R>(&root, w, subject, &opt)?;
            Ok(())
        }
        Some("suite") => {
            let opt = Options::<R>::parse(&args[1..])?;
            for w in R::workloads() {
                // Every subject must produce the identical compared answer,
                // or the whole workload is refused as a result.
                let mut answers: Vec<(&str, String)> = Vec::new();
                for &subject in R::SUBJECTS {
                    let hash = run_one::<R>(&root, w, subject, &opt)?;
                    answers.push((subject, hash));
                }
                let (first_subject, first_hash) = &answers[0];
                for (subject, hash) in &answers[1..] {
                    if hash != first_hash {
                        return Err(RigError::CrossSubjectDisagreement {
                            workload: R::workload_id(&w).to_owned(),
                            a: format!("{first_subject}={first_hash}"),
                            b: format!("{subject}={hash}"),
                        });
                    }
                }
                eprintln!(
                    "[agreement] {}: all {} subjects produced answer {}",
                    R::workload_id(&w),
                    answers.len(),
                    &first_hash[..12.min(first_hash.len())]
                );
            }
            Ok(())
        }
        _ => Err(RigError::Usage(R::USAGE)),
    }
}

fn find_workload<R: Rig>(id: &str) -> Result<R::Workload, RigError> {
    R::workloads()
        .into_iter()
        .find(|w| R::workload_id(w) == id)
        .ok_or_else(|| RigError::UnknownWorkload(id.to_owned()))
}

struct Options<R: Rig> {
    workload: Option<String>,
    subject: Option<String>,
    runs: usize,
    land: bool,
    extra: R::Extra,
}

impl<R: Rig> Options<R> {
    fn parse(args: &[String]) -> Result<Options<R>, RigError> {
        let mut o = Options {
            workload: None,
            subject: None,
            runs: 5,
            land: false,
            extra: R::Extra::default(),
        };
        let mut it = args.iter();
        while let Some(a) = it.next() {
            match a.as_str() {
                "--workload" => {
                    o.workload = Some(it.next().ok_or(RigError::Usage(R::USAGE))?.clone())
                }
                "--subject" => {
                    o.subject = Some(it.next().ok_or(RigError::Usage(R::USAGE))?.clone())
                }
                "--runs" => {
                    o.runs = it
                        .next()
                        .and_then(|n| n.parse().ok())
                        .filter(|n| *n > 0)
                        .ok_or(RigError::Usage(R::USAGE))?;
                }
                "--land" => o.land = true,
                other => {
                    if !R::parse_extra_flag(other, &mut it, &mut o.extra)? {
                        return Err(RigError::Usage(R::USAGE));
                    }
                }
            }
        }
        Ok(o)
    }
}

fn repo_root<R: Rig>() -> Result<PathBuf, RigError> {
    let cwd = std::env::current_dir()?;
    if cwd.join("benches").join(R::BENCH).is_dir() {
        Ok(cwd)
    } else {
        Err(RigError::NotRepoRoot(R::BENCH))
    }
}

fn hash_paths(kind: &HashKind, paths: &[PathBuf]) -> std::io::Result<(String, usize)> {
    match kind {
        HashKind::Canonical => {
            assert_eq!(
                paths.len(),
                1,
                "HashKind::Canonical names exactly one file; see its doc comment"
            );
            let a = canonical_answer(&paths[0])?;
            Ok((a.sha256, a.rows))
        }
        HashKind::Raw => {
            let refs: Vec<&Path> = paths.iter().map(PathBuf::as_path).collect();
            raw_answer(&refs)
        }
    }
}

fn run_one<R: Rig>(
    root: &Path,
    w: R::Workload,
    subject_name: &str,
    opt: &Options<R>,
) -> Result<String, RigError> {
    let scratch = root
        .join(format!("target/{}-scratch", R::BENCH))
        .join(R::workload_id(&w).replace('/', "_"))
        .join(subject_name.replace(['@', '/'], "_"));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch)?;

    let datasets = R::generate_inputs(root, &w, &scratch)?;

    let caps = CapPolicy::house();
    let runner = Runner::new(caps, scratch.clone());
    let prepared = R::prepare_subject(root, &w, subject_name, &runner, &scratch, &opt.extra)?;

    eprintln!(
        "[{}] {} — warm-up + {} runs…",
        prepared.subject.label(),
        R::workload_id(&w),
        opt.runs
    );

    let mut phase_measurements: BTreeMap<&'static str, Vec<Measurement>> = prepared
        .phases
        .iter()
        .map(|p| (p.name, Vec::with_capacity(opt.runs)))
        .collect();
    let (compared_paths, compared_kind) = &prepared.answer.compared;
    let mut compared_answer: Option<(String, usize)> = None;
    let mut stability_answers: Vec<Option<(String, usize)>> =
        vec![None; prepared.answer.stability_only.len()];

    // Warm-up (discarded from the RunSet), then measured runs; the answer
    // must be identical across every run, warm-up included.
    for i in 0..=opt.runs {
        let warm = if i == 0 { Warmth::Cold } else { Warmth::Warm };
        for p in &prepared.reset {
            let _ = std::fs::remove_dir_all(p);
            let _ = std::fs::remove_file(p);
        }
        for phase in &prepared.phases {
            let m = runner.run(&phase.argv, phase.output_file.as_deref(), warm)?;
            if i > 0 {
                phase_measurements
                    .get_mut(phase.name)
                    .expect("phase set is fixed across iterations")
                    .push(m);
            }
        }

        let answer = hash_paths(compared_kind, compared_paths)?;
        match &compared_answer {
            None => compared_answer = Some(answer),
            Some(prev) if *prev != answer => {
                return Err(RigError::UnstableAnswer {
                    subject: prepared.subject.label(),
                    workload: R::workload_id(&w).to_owned(),
                });
            }
            Some(_) => {}
        }
        for (idx, (path, kind)) in prepared.answer.stability_only.iter().enumerate() {
            let a = hash_paths(kind, std::slice::from_ref(path))?;
            match &stability_answers[idx] {
                None => stability_answers[idx] = Some(a),
                Some(prev) if *prev != a => {
                    return Err(RigError::UnstableAnswer {
                        subject: prepared.subject.label(),
                        workload: R::workload_id(&w).to_owned(),
                    });
                }
                Some(_) => {}
            }
        }
    }
    let (answer_sha, answer_rows) = compared_answer.expect("at least the warm-up ran");
    let stability_final: Vec<(String, usize)> = stability_answers
        .into_iter()
        .map(|a| a.expect("stability_only entries execute every iteration"))
        .collect();

    let phase_runs: BTreeMap<&'static str, RunSet> = phase_measurements
        .into_iter()
        .map(|(name, measurements)| (name, RunSet { measurements }))
        .collect();
    let headline_name = R::headline_phase(&w);
    let headline = phase_runs
        .get(headline_name)
        .unwrap_or_else(|| panic!("headline_phase {headline_name:?} names no real phase"))
        .clone();

    let extra_notes = R::extra_notes(
        root,
        &w,
        &phase_runs,
        &(answer_sha.clone(), answer_rows),
        &stability_final,
    );
    let notes = if extra_notes.is_empty() {
        format!("{} Answer rows: {answer_rows}.", prepared.notes)
    } else {
        format!("{} {extra_notes} Answer rows: {answer_rows}.", prepared.notes)
    };
    let record = ResultRecord {
        bench: R::BENCH.to_owned(),
        story: R::STORY.to_owned(),
        subject: prepared.subject.clone(),
        rig: ResultRecord::rig_commit(),
        workload: Workload {
            id: R::workload_id(&w).to_owned(),
            description: R::workload_description(&w).to_owned(),
            seed: R::workload_seed(&w),
            correctness_sha256: Some(answer_sha.clone()),
        },
        datasets,
        hardware: Hardware::capture(),
        caps,
        runs: headline,
        date: ResultRecord::today_utc(),
        notes,
        supersedes: None,
    };

    let median_ms = record.runs.wall_micros_median() as f64 / 1000.0;
    let (min, max) = record.runs.wall_micros_min_max();
    eprintln!(
        "[{}] {} ({headline_name}): median {:.1} ms (min {:.1} / max {:.1}), peak RSS {} KiB, \
         answer {}",
        prepared.subject.label(),
        R::workload_id(&w),
        median_ms,
        min as f64 / 1000.0,
        max as f64 / 1000.0,
        record.runs.peak_rss_kib_max(),
        &answer_sha[..12.min(answer_sha.len())],
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
