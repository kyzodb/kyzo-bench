use crate::dataset::DatasetDigest;
use crate::hardware::Hardware;
use crate::run::{CapPolicy, RunSet};
use crate::subject::Subject;
use crate::workload::Workload;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// One publishable result: everything `.claude/rules/results-data.md`
/// demands, as required fields. If you can construct this, the methodology
/// traveled with the number; there is no way to leave the seed, the
/// hardware, or the dataset identity behind.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResultRecord {
    /// Bench directory this belongs to, e.g. "datalog".
    pub bench: String,
    /// Story this executes, e.g. "kyzo#22".
    pub story: String,
    pub subject: Subject,
    /// The kyzo-bench commit the rig ran from (and whether the tree was
    /// dirty), so every number is anchored to the exact measurement code
    /// that produced it. Captured, never typed.
    pub rig: crate::subject::EngineCommit,
    pub workload: Workload,
    pub datasets: Vec<DatasetDigest>,
    pub hardware: Hardware,
    pub caps: CapPolicy,
    pub runs: RunSet,
    /// UTC date of the run, YYYY-MM-DD.
    pub date: String,
    /// Free-form notes a reader needs (opponent tuning cited, class
    /// differences declared). Empty is acceptable; missing is not.
    pub notes: String,
    /// Set when this record corrects an earlier one: the path of the flawed
    /// file (which stays) and one sentence naming the flaw.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supersedes: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum LandError {
    #[error(
        "refusing to overwrite committed result {path}: results/ is append-only; \
         land a superseding record instead"
    )]
    AlreadyExists { path: PathBuf },
    #[error("io error landing record: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization failed: {0}")]
    Serde(#[from] serde_json::Error),
}

impl ResultRecord {
    /// Write this record into `results_dir`, refusing to overwrite.
    /// Returns the path it landed at.
    ///
    /// The filename carries the run's identity down to the day, not the
    /// second, so a same-day re-land of the identical (bench, workload,
    /// subject, seed) collides by construction. A plain re-run refuses,
    /// full stop — that's the append-only guarantee working. A *correction*
    /// (`self.supersedes` is `Some`, naming the flawed file it replaces)
    /// is expected to land the same day it was caught, so it may claim the
    /// next free `-2`, `-3`, ... suffix instead of waiting for the date to
    /// roll over.
    pub fn land(&self, results_dir: &Path) -> Result<PathBuf, LandError> {
        let base = format!(
            "{}--{}--{}--seed{}--{}",
            self.bench,
            sanitize(&self.workload.id),
            sanitize(&self.subject.label()),
            self.workload.seed,
            self.date,
        );
        let first = results_dir.join(format!("{base}.json"));
        let path = if !first.exists() {
            first
        } else if self.supersedes.is_some() {
            (2..)
                .map(|n| results_dir.join(format!("{base}-{n}.json")))
                .find(|p| !p.exists())
                .expect("integers are infinite")
        } else {
            return Err(LandError::AlreadyExists { path: first });
        };
        std::fs::create_dir_all(results_dir)?;
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, json + "\n")?;
        Ok(path)
    }

    /// The rig's own commit, from the current directory's repo.
    pub fn rig_commit() -> crate::subject::EngineCommit {
        crate::subject::EngineCommit::capture(std::path::Path::new(".")).unwrap_or(
            crate::subject::EngineCommit {
                commit: "unknown".into(),
                dirty: true,
            },
        )
    }

    /// Today's UTC date as YYYY-MM-DD, from the system clock, via a civil
    /// conversion that is a pure function of the unix timestamp.
    pub fn today_utc() -> String {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let days = i64::try_from(secs / 86_400).expect("era");
        let (y, m, d) = civil_from_days(days);
        format!("{y:04}-{m:02}-{d:02}")
    }
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

/// Howard Hinnant's days-from-civil inverse: unix day count to (y, m, d).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (
        if m <= 2 { y + 1 } else { y },
        u32::try_from(m).expect("month"),
        u32::try_from(d).expect("day"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::{Measurement, Warmth};
    use crate::seed::Seed;
    use crate::subject::{Opponent, Provenance};

    fn record() -> ResultRecord {
        ResultRecord {
            bench: "datalog".into(),
            story: "kyzo#22".into(),
            subject: Subject::Opponent(Opponent {
                name: "souffle".into(),
                version: "2.5".into(),
                provenance: Provenance::BuiltFromSource {
                    repo: "https://github.com/souffle-lang/souffle".into(),
                    reference: "2.5".into(),
                    script: "opponents/souffle/build.sh".into(),
                },
            }),
            rig: ResultRecord::rig_commit(),
            workload: Workload {
                id: "tc/test".into(),
                description: "test".into(),
                seed: Seed(7),
                correctness_sha256: None,
            },
            datasets: vec![],
            hardware: Hardware::capture(),
            caps: CapPolicy::house(),
            runs: RunSet {
                measurements: vec![Measurement {
                    argv: vec!["true".into()],
                    wall_micros: 1,
                    peak_rss_kib: 1,
                    output_sha256: "00".into(),
                    warm: Warmth::Cold,
                }],
            },
            date: ResultRecord::today_utc(),
            notes: String::new(),
            supersedes: None,
        }
    }

    #[test]
    fn land_refuses_overwrite() {
        let dir = std::env::temp_dir().join(format!("kb-land-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let r = record();
        let first = r.land(&dir).expect("first landing");
        assert!(first.exists());
        let err = r.land(&dir).unwrap_err();
        assert!(matches!(err, LandError::AlreadyExists { .. }), "got: {err}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn land_supersedes_claims_a_numbered_suffix_instead_of_overwriting() {
        let dir = std::env::temp_dir().join(format!("kb-land-super-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let flawed = record();
        let flawed_path = flawed.land(&dir).expect("first landing");

        let mut correction = record();
        correction.supersedes = Some(format!(
            "{}: contaminated by a concurrently running benchmark",
            flawed_path.display()
        ));
        let corrected_path = correction.land(&dir).expect("superseding landing");

        assert_ne!(flawed_path, corrected_path);
        assert!(flawed_path.exists(), "the flawed record stays; append-only");
        assert!(corrected_path.exists());
        assert!(
            corrected_path
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .ends_with("-2.json"),
            "got: {}",
            corrected_path.display()
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn record_round_trips_through_json() {
        let r = record();
        let json = serde_json::to_string(&r).unwrap();
        let back: ResultRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(back.workload.seed, r.workload.seed);
        assert_eq!(back.subject.label(), r.subject.label());
    }

    #[test]
    fn civil_date_matches_known_days() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
    }
}
