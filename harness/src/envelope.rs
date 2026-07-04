//! The Rust-side twin of `harness/envelope.py`, for subject binaries that
//! sweep many measurement points inside one long-running process (an index
//! build, then N query-parameter points against it) rather than being
//! measured as N repeated subprocess invocations — `kyzo-vector-runner`,
//! `surrealdb-runner`'s vector modes, and any future binary of that shape.
//! `harness::rig`'s `Rig` trait and `ResultRecord`/`RunSet` are the right
//! fit when "run this argv, hash its output, repeat" is the whole
//! methodology; they are the wrong fit here for the same reason `envelope.py`
//! exists instead of forcing `kuzu_baseline.py` onto `Rig` — forcing this
//! shape onto `RunSet::measurements` would be recording a hash of "the
//! sweep happened" in place of the curve itself.
//!
//! Same law as the Python side (`.claude/rules/results-data.md`): engine
//! commit, opponent name and exact version, dataset identity, hardware,
//! date, and the command that produced it are structural, not optional;
//! everything bench-specific lives in the open `metrics` field.

use crate::hardware::Hardware;
use crate::subject::EngineCommit;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum EnvelopeError {
    #[error(
        "refusing to overwrite committed result {path}: results/ is append-only; \
         land a superseding record instead"
    )]
    AlreadyExists { path: PathBuf },
    #[error("io error landing envelope: {0}")]
    Io(#[from] std::io::Error),
}

/// Assemble one envelope. `subject` is `kyzo_bench_harness::Subject`'s own
/// `Serialize` output — the same shape the Rust `Rig`s' `ResultRecord`
/// carries and `envelope.py`'s hand-built `subject` dict mirrors by hand
/// across the language boundary; here there is no boundary to cross, so it
/// is the real type, not a hand-copied shape of it.
pub fn build(
    bench: &str,
    story: &str,
    subject: &crate::subject::Subject,
    metrics: Value,
) -> Value {
    json!({
        "bench": bench,
        "story": story,
        "subject": subject,
        "rig": EngineCommit::capture(Path::new(".")).unwrap_or(EngineCommit {
            commit: "unknown".into(),
            dirty: true,
        }),
        "hardware": Hardware::capture(),
        "date": crate::record::ResultRecord::today_utc(),
        "command": std::env::args().collect::<Vec<_>>().join(" "),
        "metrics": metrics,
    })
}

/// Write `envelope` into `results_dir`, refusing to overwrite — identical
/// law and identical `{bench}--{id}--{subject}[--seed{N}]--{date}.json`
/// naming to `envelope.py::land`.
pub fn land(
    envelope: &Value,
    results_dir: &Path,
    id: &str,
    seed: Option<u64>,
) -> Result<PathBuf, EnvelopeError> {
    let bench = envelope["bench"].as_str().expect("bench is a string");
    let date = envelope["date"].as_str().expect("date is a string");
    let subject = &envelope["subject"];
    let subject_label = format!(
        "{}_{}",
        subject["name"].as_str().unwrap_or("unknown"),
        subject["version"].as_str().unwrap_or("unknown")
    );
    let seed_part = seed.map(|s| format!("--seed{s}")).unwrap_or_default();
    let name = format!(
        "{bench}--{}--{}{seed_part}--{date}.json",
        sanitize(id),
        sanitize(&subject_label),
    );
    let path = results_dir.join(name);
    if path.exists() {
        return Err(EnvelopeError::AlreadyExists { path });
    }
    std::fs::create_dir_all(results_dir)?;
    let text = serde_json::to_string_pretty(envelope).expect("Value always serializes");
    std::fs::write(&path, text + "\n")?;
    Ok(path)
}

/// Always print; `--land` additionally writes into `results_dir` — the same
/// "never require a re-run to see the number" rule `envelope.py::emit`
/// documents.
pub fn emit(
    envelope: Value,
    land_it: bool,
    results_dir: &Path,
    id: &str,
    seed: Option<u64>,
) -> Result<(), EnvelopeError> {
    println!("{}", serde_json::to_string_pretty(&envelope).expect("Value always serializes"));
    if land_it {
        let path = land(&envelope, results_dir, id, seed)?;
        eprintln!("landed: {}", path.display());
    }
    Ok(())
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subject::{Opponent, Provenance, Subject};

    fn envelope() -> Value {
        build(
            "vector",
            "kyzo#25",
            &Subject::Opponent(Opponent {
                name: "surrealdb".into(),
                version: "3.2.0".into(),
                provenance: Provenance::Package {
                    ecosystem: "cargo".into(),
                    package: "surrealdb".into(),
                    version: "3.2.0".into(),
                },
            }),
            json!({"curve": []}),
        )
    }

    #[test]
    fn land_refuses_overwrite() {
        let dir = std::env::temp_dir().join(format!("kb-envelope-land-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let env = envelope();
        let first = land(&env, &dir, "sift1m", None).expect("first landing");
        assert!(first.exists());
        let err = land(&env, &dir, "sift1m", None).unwrap_err();
        assert!(matches!(err, EnvelopeError::AlreadyExists { .. }), "got: {err}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn land_filename_matches_python_convention() {
        let dir = std::env::temp_dir().join(format!("kb-envelope-name-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = land(&envelope(), &dir, "sift1m", Some(42)).expect("landing");
        let name = path.file_name().unwrap().to_str().unwrap();
        assert!(name.starts_with("vector--sift1m--surrealdb_3.2.0--seed42--"), "got: {name}");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
