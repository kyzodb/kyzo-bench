//! kyzo#22 — recursive Datalog vs Souffle.
//!
//! Usage (from the repo root):
//!     datalog-rig list
//!     datalog-rig run --workload tc/sparse-n2k-m6k --subject souffle [--runs 5] [--threads N] [--land]
//!     datalog-rig suite [--runs 5] [--threads N] [--land]
//!
//! `--land` writes an append-only `ResultRecord` into `results/`; without
//! it the record is printed to stdout for inspection. Everything but the
//! workload registry (`workloads`), the fact generator (`generate`), and
//! the Souffle adapter (`souffle`) lives in `kyzo_bench_harness::rig`.

mod generate;
mod souffle;
mod workloads;

use kyzo_bench_harness::rig::{AnswerSpec, HashKind, Phase, PreparedSubject, Rig, RigError};
use kyzo_bench_harness::{DatasetDigest, Runner, Seed, subject::locate_kyzo};
use souffle::{Mode, Souffle};
use std::path::Path;
use workloads::Registered;

fn main() -> std::process::ExitCode {
    kyzo_bench_harness::rig::main::<DatalogRig>()
}

struct DatalogRig;

/// The one bench-specific flag: Souffle's `-j` thread count. `kyzo-runner`
/// has no equivalent knob today — its parallelism is whatever the engine
/// defaults to, undeclared here — so `--threads` currently configures only
/// the Souffle side. Wiring a matching flag into `kyzo-runner` (an engine
/// change) is a prerequisite for this flag meaning "both sides" in fact,
/// not just in name.
struct Extra {
    threads: usize,
}

impl Default for Extra {
    fn default() -> Extra {
        Extra {
            threads: std::thread::available_parallelism().map_or(1, |n| n.get()),
        }
    }
}

impl Rig for DatalogRig {
    type Workload = Registered;
    type Extra = Extra;

    const BENCH: &'static str = "datalog";
    const STORY: &'static str = "kyzo#22";
    const SUBJECTS: &'static [&'static str] = &["kyzo", "souffle", "souffle-compiled"];
    const USAGE: &'static str = "usage: datalog-rig list | run --workload <id> --subject <s> [--runs N] [--threads N] [--land] | suite [--runs N] [--threads N] [--land]";

    fn workloads() -> Vec<Registered> {
        workloads::suite()
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

    fn parse_extra_flag(
        name: &str,
        it: &mut std::slice::Iter<'_, String>,
        extra: &mut Extra,
    ) -> Result<bool, RigError> {
        if name != "--threads" {
            return Ok(false);
        }
        extra.threads = it
            .next()
            .and_then(|n| n.parse().ok())
            .filter(|n| *n > 0)
            .ok_or(RigError::Usage(Self::USAGE))?;
        Ok(true)
    }

    fn generate_inputs(
        root: &Path,
        w: &Registered,
        scratch: &Path,
    ) -> std::io::Result<Vec<DatasetDigest>> {
        let facts = scratch.join("facts");
        w.generate(&facts, root)?;
        DatasetDigest::of_dir(&facts)
    }

    fn prepare_subject(
        root: &Path,
        w: &Registered,
        subject_name: &str,
        runner: &Runner,
        scratch: &Path,
        extra: &Extra,
    ) -> Result<PreparedSubject, RigError> {
        let facts = scratch.join("facts");
        let outs = scratch.join("out");
        std::fs::create_dir_all(&outs)?;
        let output_file = outs.join(format!("{}.csv", w.output_relation()));

        match subject_name {
            "souffle" | "souffle-compiled" => {
                let mode = if subject_name == "souffle" {
                    Mode::Interpreted
                } else {
                    Mode::Compiled
                };
                let engine = Souffle::locate(root, mode, extra.threads)
                    .map_err(|e| RigError::Bench(Box::new(e)))?;
                let program = root
                    .join("benches/datalog/programs")
                    .join(w.souffle_program());
                let argv = engine
                    .prepare(runner, &program, &facts, &outs, scratch)
                    .map_err(|e| RigError::Bench(Box::new(e)))?;
                Ok(PreparedSubject {
                    subject: engine.subject(),
                    phases: vec![Phase {
                        name: "answer",
                        argv,
                        output_file: Some(output_file.clone()),
                    }],
                    reset: vec![],
                    answer: AnswerSpec {
                        compared: (vec![output_file], HashKind::Canonical),
                        stability_only: vec![],
                    },
                    notes: engine.notes(),
                })
            }
            "kyzo" => {
                let (commit, runner_bin) = locate_kyzo(root, "kyzo-runner")?;
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
                    commit.dirty_suffix(),
                );
                Ok(PreparedSubject {
                    subject: kyzo_bench_harness::Subject::Kyzo(commit),
                    phases: vec![Phase {
                        name: "answer",
                        argv,
                        output_file: Some(output_file.clone()),
                    }],
                    reset: vec![],
                    answer: AnswerSpec {
                        compared: (vec![output_file], HashKind::Canonical),
                        stability_only: vec![],
                    },
                    notes,
                })
            }
            other => Err(RigError::UnknownSubject(
                other.to_owned(),
                Self::SUBJECTS.join(", "),
            )),
        }
    }

    fn headline_phase(_w: &Registered) -> &'static str {
        "answer"
    }
}
