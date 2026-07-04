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

use kyzo_bench_harness::opponents::sqlite::sqlite_subject;
use kyzo_bench_harness::rig::{AnswerSpec, HashKind, Phase, PreparedSubject, Rig, RigError};
use kyzo_bench_harness::{DatasetDigest, Runner, RunSet, Seed, Subject, subject::locate_kyzo};
use std::path::Path;

fn main() -> std::process::ExitCode {
    kyzo_bench_harness::rig::main::<OltpRig>()
}

struct OltpRig;

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

impl Rig for OltpRig {
    type Workload = Registered;
    type Extra = ();

    const BENCH: &'static str = "oltp";
    const STORY: &'static str = "kyzo#26";
    const SUBJECTS: &'static [&'static str] = &["kyzo", "sqlite"];
    const USAGE: &'static str = "usage: oltp-rig list | run --workload <id> --subject <s> [--runs N] [--land] [--supersedes <path> <reason>] | suite [--runs N] [--land]";

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
        _root: &Path,
        w: &Registered,
        scratch: &Path,
    ) -> std::io::Result<Vec<DatasetDigest>> {
        let stream = ops::generate(w.seed, w.rows, w.ops);
        std::fs::write(scratch.join("load.sql"), ops::sqlite_load(&stream))?;
        std::fs::write(scratch.join("mixed.sql"), ops::sqlite_mixed(&stream))?;
        std::fs::write(scratch.join("dump.sql"), ops::sqlite_dump())?;
        std::fs::write(scratch.join("stream.ops"), ops::kyzo_stream(&stream))?;
        DatasetDigest::of_dir(scratch)
    }

    fn prepare_subject(
        root: &Path,
        _w: &Registered,
        subject_name: &str,
        _runner: &Runner,
        scratch: &Path,
        _extra: &(),
    ) -> Result<PreparedSubject, RigError> {
        let reads_out = scratch.join("reads.tsv");
        let dump_out = scratch.join("dump.tsv");
        let answer = AnswerSpec {
            compared: (vec![reads_out.clone(), dump_out.clone()], HashKind::Raw),
            stability_only: vec![],
        };

        match subject_name {
            "sqlite" => {
                let (subject, bin) = sqlite_subject(root, "sqlite")?;
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
                Ok(PreparedSubject {
                    subject,
                    phases: vec![
                        Phase {
                            name: "load",
                            argv: invoke("load.sql", None),
                            output_file: None,
                        },
                        Phase {
                            name: "mixed",
                            argv: invoke("mixed.sql", Some(&reads_out)),
                            output_file: Some(reads_out),
                        },
                        Phase {
                            name: "dump",
                            argv: invoke("dump.sql", Some(&dump_out)),
                            output_file: Some(dump_out),
                        },
                    ],
                    reset: vec![
                        db,
                        scratch.join("db.sqlite-wal"),
                        scratch.join("db.sqlite-shm"),
                    ],
                    answer,
                    notes: "SQLite 3.53.3 CLI, WAL + synchronous=NORMAL (declared production \
                            config), autocommit per mixed op, 1000-row transactions in the load \
                            phase, ORDER BY id dump. Single connection, single thread — SQLite's \
                            native embedded shape."
                        .into(),
                })
            }
            "kyzo" => {
                let (commit, bin) = locate_kyzo(root, "kyzo-oltp-runner")?;
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
                    "KyzoDB at engine commit {} (pre-release: git-rev pinned via this repo's \
                     own Cargo.lock, not a tagged version — see .claude/rules/methodology.md); \
                     persistent fjall store, every op through `Db::run_script` (parse \
                     included, exactly as SQLite parses each SQL statement), one script per \
                     mixed op, 1000-row batches in the load phase bound through a `$data` \
                     param (n-independent script text, matching the public bulk-`:put` \
                     calling convention) rather than inlined as a literal list, sorted dump. \
                     Single connection, single thread.",
                    commit.commit,
                );
                Ok(PreparedSubject {
                    subject: Subject::Kyzo(commit),
                    phases: vec![
                        Phase {
                            name: "load",
                            argv: invoke("load", None),
                            output_file: None,
                        },
                        Phase {
                            name: "mixed",
                            argv: invoke("mixed", Some(&reads_out)),
                            output_file: Some(reads_out),
                        },
                        Phase {
                            name: "dump",
                            argv: invoke("dump", Some(&dump_out)),
                            output_file: Some(dump_out),
                        },
                    ],
                    reset: vec![store],
                    answer,
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
        "mixed"
    }

    // The headline record is the mixed phase; load and dump numbers ride in
    // the notes so the record stays one-metric-one-record.
    fn extra_notes(
        _root: &Path,
        w: &Registered,
        phase_runs: &std::collections::BTreeMap<&'static str, RunSet>,
        _compared: &(String, usize),
        _stability_only: &[(String, usize)],
    ) -> String {
        let load = &phase_runs["load"];
        let mixed = &phase_runs["mixed"];
        let dump = &phase_runs["dump"];
        let load_median_ms = load.wall_micros_median() as f64 / 1000.0;
        let mixed_median_ms = mixed.wall_micros_median() as f64 / 1000.0;
        let dump_ms_last = dump
            .measurements
            .last()
            .map(|m| m.wall_micros as f64 / 1000.0)
            .unwrap_or(0.0);
        let ops_per_sec = w.ops as f64 / (mixed_median_ms / 1000.0);
        let rows_per_sec = w.rows as f64 / (load_median_ms / 1000.0);
        format!(
            "Mixed phase is the headline ({} ops → {ops_per_sec:.0} ops/s median). Load: {} \
             rows in median {load_median_ms:.1} ms ({rows_per_sec:.0} rows/s). Dump (unmeasured \
             sanity phase): {dump_ms_last:.1} ms.",
            w.ops, w.rows,
        )
    }
}
