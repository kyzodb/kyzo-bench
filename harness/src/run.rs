use crate::dataset::hex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Instant;

/// The caps every run executes under. Uncapped runs cost machines
/// (the engine lane OOM'd two); there is no way to ask [`Runner`] for one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapPolicy {
    pub mem_bytes: u64,
    pub timeout_secs: u64,
}

impl CapPolicy {
    /// The house default: 12 GiB / 1800 s, matching the engine lane's law.
    pub fn house() -> CapPolicy {
        CapPolicy {
            mem_bytes: 12 * 1024 * 1024 * 1024,
            timeout_secs: 1800,
        }
    }
}

/// What one execution actually did. Only [`Runner::run`] mints these.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Measurement {
    /// The exact argv that ran (after the cap wrappers).
    pub argv: Vec<String>,
    pub wall_micros: u64,
    /// Peak resident set of the child process tree, KiB (`rusage.ru_maxrss`).
    pub peak_rss_kib: u64,
    /// SHA-256 of the bytes the subject wrote to the output artifact
    /// (a file for file-producing subjects, stdout otherwise).
    pub output_sha256: String,
    pub warm: Warmth,
}

/// Declared cache state of a run. Warm/cold is methodology, not a footnote,
/// so it is part of the measurement's type rather than prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Warmth {
    /// First execution after setup; page cache state undeclared.
    Cold,
    /// Executed after at least one discarded warm-up run of the same argv.
    Warm,
}

/// A set of repeated measurements of the same argv, with the repetition
/// count visible. Statistics are derived, never stored.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSet {
    pub measurements: Vec<Measurement>,
}

impl RunSet {
    pub fn wall_micros_median(&self) -> u64 {
        let mut v: Vec<u64> = self.measurements.iter().map(|m| m.wall_micros).collect();
        v.sort_unstable();
        v[v.len() / 2]
    }

    pub fn wall_micros_min_max(&self) -> (u64, u64) {
        let it = self.measurements.iter().map(|m| m.wall_micros);
        (it.clone().min().unwrap_or(0), it.max().unwrap_or(0))
    }

    pub fn peak_rss_kib_max(&self) -> u64 {
        self.measurements
            .iter()
            .map(|m| m.peak_rss_kib)
            .max()
            .unwrap_or(0)
    }

    /// All repetitions must have produced identical output bytes; a subject
    /// that answers differently run-to-run has no single number to report.
    pub fn output_unanimous(&self) -> Option<&str> {
        let first = self.measurements.first()?;
        self.measurements
            .iter()
            .all(|m| m.output_sha256 == first.output_sha256)
            .then_some(first.output_sha256.as_str())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("failed to spawn {argv:?}: {source}")]
    Spawn {
        argv: Vec<String>,
        source: std::io::Error,
    },
    #[error("run exceeded its {timeout_secs}s timeout: {argv:?}")]
    TimedOut {
        argv: Vec<String>,
        timeout_secs: u64,
    },
    #[error(
        "run killed by signal {signal:?} after {wall_ms:.0} ms with the clock unexpired — \
         most likely the {mem_bytes}-byte address-space cap: {argv:?}\nstderr tail:\n{stderr_tail}"
    )]
    Killed {
        argv: Vec<String>,
        signal: Option<i32>,
        wall_ms: f64,
        mem_bytes: u64,
        stderr_tail: String,
    },
    #[error("run exited non-zero ({code:?}): {argv:?}\nstderr tail:\n{stderr_tail}")]
    Failed {
        argv: Vec<String>,
        code: Option<i32>,
        stderr_tail: String,
    },
    #[error("output artifact missing after run: {path}")]
    NoOutput { path: PathBuf },
    #[error("io error during run: {0}")]
    Io(#[from] std::io::Error),
}

/// Executes subjects under caps and mints [`Measurement`]s.
///
/// Caps are applied with `timeout(1)` and `prlimit(1)` so the exact wrapper
/// is visible in the recorded argv, and peak RSS is taken from the kernel's
/// own accounting (`/proc/<pid>/status` `VmHWM` of the direct child,
/// sampled to exit; for in-process engines use the subject's own report).
pub struct Runner {
    pub caps: CapPolicy,
    /// Directory the subject runs in.
    pub cwd: PathBuf,
}

impl Runner {
    pub fn new(caps: CapPolicy, cwd: PathBuf) -> Runner {
        Runner { caps, cwd }
    }

    /// Run `argv`, capped; hash `output_file` if given, else stdout.
    /// The child's stdout goes to a file either way, so huge outputs never
    /// buffer in memory.
    pub fn run(
        &self,
        argv: &[String],
        output_file: Option<&std::path::Path>,
        warm: Warmth,
    ) -> Result<Measurement, RunError> {
        let mut full: Vec<String> = vec![
            "timeout".into(),
            "--signal=KILL".into(),
            self.caps.timeout_secs.to_string(),
            "prlimit".into(),
            format!("--as={}", self.caps.mem_bytes),
            "--".into(),
        ];
        full.extend(argv.iter().cloned());

        // Unique per invocation: concurrent runs in one cwd must not share
        // a scratch file.
        static RUN_SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let serial = RUN_SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let stdout_path = self
            .cwd
            .join(format!(".run-stdout.{}.{serial}", std::process::id()));
        let stdout_f = std::fs::File::create(&stdout_path)?;
        let start = Instant::now();
        let mut child = Command::new(&full[0])
            .args(&full[1..])
            .current_dir(&self.cwd)
            .stdout(Stdio::from(stdout_f))
            .stderr(Stdio::piped())
            .stdin(Stdio::null())
            .spawn()
            .map_err(|source| RunError::Spawn {
                argv: full.clone(),
                source,
            })?;

        // Sample the whole descendant tree until exit: the direct child is
        // the `timeout` wrapper, so the subject is one or more levels down.
        // Peak = max over samples of the tree's summed VmRSS, floored by the
        // largest single-process VmHWM seen (which catches spikes between
        // samples for the dominant process).
        let pid = child.id();
        let mut peak_sum_kib: u64 = 0;
        let mut peak_hwm_kib: u64 = 0;
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            let (sum, hwm) = sample_tree_rss(pid);
            peak_sum_kib = peak_sum_kib.max(sum);
            peak_hwm_kib = peak_hwm_kib.max(hwm);
            std::thread::sleep(std::time::Duration::from_millis(2));
        };
        let peak_kib = peak_sum_kib.max(peak_hwm_kib);
        let wall = start.elapsed();

        let mut stderr_tail = String::new();
        if let Some(mut e) = child.stderr.take() {
            use std::io::Read;
            let mut s = String::new();
            let _ = e.read_to_string(&mut s);
            stderr_tail = s
                .chars()
                .rev()
                .take(2000)
                .collect::<String>()
                .chars()
                .rev()
                .collect();
        }

        if !status.success() {
            // A signal death is only a timeout if the clock actually ran
            // out; otherwise it is the memory cap (or a crash) and must be
            // reported as what it is. Misfiling an OOM kill as a timeout
            // once sent this rig chasing the wrong cap.
            let clock_expired = wall.as_secs() >= self.caps.timeout_secs;
            if status.code() == Some(124) || (status.code().is_none() && clock_expired) {
                return Err(RunError::TimedOut {
                    argv: full,
                    timeout_secs: self.caps.timeout_secs,
                });
            }
            if status.code().is_none() {
                use std::os::unix::process::ExitStatusExt;
                return Err(RunError::Killed {
                    argv: full,
                    signal: status.signal(),
                    wall_ms: wall.as_secs_f64() * 1e3,
                    mem_bytes: self.caps.mem_bytes,
                    stderr_tail,
                });
            }
            return Err(RunError::Failed {
                argv: full,
                code: status.code(),
                stderr_tail,
            });
        }

        let artifact = match output_file {
            Some(p) => {
                let p = if p.is_absolute() {
                    p.to_path_buf()
                } else {
                    self.cwd.join(p)
                };
                if !p.exists() {
                    return Err(RunError::NoOutput { path: p });
                }
                p
            }
            None => stdout_path.clone(),
        };
        let output_sha256 = sha256_file(&artifact)?;
        let _ = std::fs::remove_file(&stdout_path);

        Ok(Measurement {
            argv: full,
            wall_micros: u64::try_from(wall.as_micros()).unwrap_or(u64::MAX),
            peak_rss_kib: peak_kib,
            output_sha256,
            warm,
        })
    }

    /// One discarded warm-up, then `n` measured warm runs of the same argv.
    pub fn run_n_warm(
        &self,
        argv: &[String],
        output_file: Option<&std::path::Path>,
        n: usize,
    ) -> Result<RunSet, RunError> {
        assert!(n > 0, "a run set with zero runs measures nothing");
        self.run(argv, output_file, Warmth::Cold)?; // discarded warm-up
        let mut measurements = Vec::with_capacity(n);
        for _ in 0..n {
            measurements.push(self.run(argv, output_file, Warmth::Warm)?);
        }
        Ok(RunSet { measurements })
    }
}

/// One sample over `root` and all its descendants: (sum of current VmRSS,
/// max single-process VmHWM), in KiB. Processes that vanish mid-walk read
/// as zero — correct for a sampler, which only ever underestimates between
/// samples.
fn sample_tree_rss(root: u32) -> (u64, u64) {
    let mut sum = 0u64;
    let mut hwm = 0u64;
    let mut stack = vec![root];
    while let Some(pid) = stack.pop() {
        let status = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap_or_default();
        for line in status.lines() {
            if let Some(v) = line.strip_prefix("VmRSS:") {
                sum += parse_kib(v);
            } else if let Some(v) = line.strip_prefix("VmHWM:") {
                hwm = hwm.max(parse_kib(v));
            }
        }
        let children =
            std::fs::read_to_string(format!("/proc/{pid}/task/{pid}/children")).unwrap_or_default();
        stack.extend(
            children
                .split_whitespace()
                .filter_map(|c| c.parse::<u32>().ok()),
        );
    }
    (sum, hwm)
}

fn parse_kib(v: &str) -> u64 {
    v.split_whitespace()
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

fn sha256_file(path: &std::path::Path) -> Result<String, std::io::Error> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex(&hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("kb-run-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn run_measures_and_hashes_stdout() {
        let r = Runner::new(
            CapPolicy {
                mem_bytes: 1 << 30,
                timeout_secs: 30,
            },
            tmp(),
        );
        let m = r
            .run(&["echo".into(), "hello".into()], None, Warmth::Cold)
            .expect("echo must run");
        // sha256 of "hello\n"
        assert_eq!(
            m.output_sha256,
            "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03"
        );
        assert!(m.wall_micros > 0);
    }

    #[test]
    fn nonzero_exit_is_a_typed_refusal() {
        let r = Runner::new(
            CapPolicy {
                mem_bytes: 1 << 30,
                timeout_secs: 30,
            },
            tmp(),
        );
        let err = r.run(&["false".into()], None, Warmth::Cold).unwrap_err();
        assert!(matches!(err, RunError::Failed { .. }), "got: {err}");
    }

    #[test]
    fn timeout_is_a_typed_refusal() {
        let r = Runner::new(
            CapPolicy {
                mem_bytes: 1 << 30,
                timeout_secs: 1,
            },
            tmp(),
        );
        let err = r
            .run(&["sleep".into(), "5".into()], None, Warmth::Cold)
            .unwrap_err();
        assert!(matches!(err, RunError::TimedOut { .. }), "got: {err}");
    }

    #[test]
    fn memory_cap_bites() {
        let r = Runner::new(
            CapPolicy {
                mem_bytes: 64 << 20,
                timeout_secs: 30,
            },
            tmp(),
        );
        // python allocating 1 GiB must die under a 64 MiB address-space cap.
        let err = r
            .run(
                &[
                    "python3".into(),
                    "-c".into(),
                    "x = bytearray(1024*1024*1024); print(len(x))".into(),
                ],
                None,
                Warmth::Cold,
            )
            .unwrap_err();
        assert!(
            matches!(err, RunError::Failed { .. } | RunError::TimedOut { .. }),
            "cap must kill the hog, got: {err}"
        );
    }

    #[test]
    fn run_set_demands_unanimous_output() {
        let r = Runner::new(
            CapPolicy {
                mem_bytes: 1 << 30,
                timeout_secs: 30,
            },
            tmp(),
        );
        let set = r
            .run_n_warm(&["echo".into(), "same".into()], None, 3)
            .expect("echo runs");
        assert_eq!(set.measurements.len(), 3);
        assert!(set.output_unanimous().is_some());
    }
}
