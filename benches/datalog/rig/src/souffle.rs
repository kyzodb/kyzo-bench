//! The Souffle opponent adapter.
//!
//! Souffle gets its best game (`.claude/rules/methodology.md`): the pinned
//! release built by `opponents/souffle/build.sh`, run in its documented
//! performance configuration — compiled mode (`-c`) with a thread count —
//! alongside the interpreted mode KyzoDB actually competes with. Both are
//! recorded as distinct subjects; nothing is averaged across modes.

use kyzo_bench_harness::{Opponent, Provenance, Subject};
use std::path::{Path, PathBuf};

pub const SOUFFLE_VERSION: &str = "2.5";

/// How Souffle executes: its interpreter, or synthesized C++ (`-c`), which
/// its own documentation names as the performance mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Interpreted,
    Compiled,
}

/// A runnable, version-verified Souffle installation. Construction checks
/// the binary exists and reports exactly the pinned version, so a record
/// can never carry a phantom opponent.
pub struct Souffle {
    binary: PathBuf,
    pub mode: Mode,
    pub threads: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum SouffleError {
    #[error(
        "souffle binary not found at {0}; build the pinned opponent first: \
         opponents/souffle/build.sh"
    )]
    NotBuilt(PathBuf),
    #[error("souffle at {binary} reports version {found:?}, rig pins {pinned}")]
    WrongVersion {
        binary: PathBuf,
        found: String,
        pinned: &'static str,
    },
    #[error("could not interrogate souffle: {0}")]
    Io(#[from] std::io::Error),
    #[error("souffle -o (synthesize + compile) failed: {0}")]
    CompileFailed(String),
}

impl Souffle {
    /// `repo_root` is the kyzo-bench checkout root.
    pub fn locate(repo_root: &Path, mode: Mode, threads: usize) -> Result<Souffle, SouffleError> {
        let binary = repo_root.join("opponents/souffle/dist/bin/souffle");
        if !binary.is_file() {
            return Err(SouffleError::NotBuilt(binary));
        }
        let out = std::process::Command::new(&binary)
            .arg("--version")
            .output()?;
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        let version_line = text
            .lines()
            .find(|l| l.trim_start().starts_with("Version:"))
            .unwrap_or("")
            .to_owned();
        if !version_line.contains(SOUFFLE_VERSION) {
            return Err(SouffleError::WrongVersion {
                binary,
                found: version_line,
                pinned: SOUFFLE_VERSION,
            });
        }
        Ok(Souffle {
            binary,
            mode,
            threads,
        })
    }

    /// The subject identity this adapter produces measurements for.
    pub fn subject(&self) -> Subject {
        let name = match self.mode {
            Mode::Interpreted => "souffle",
            Mode::Compiled => "souffle-compiled",
        };
        Subject::Opponent(Opponent {
            name: name.to_owned(),
            version: SOUFFLE_VERSION.to_owned(),
            provenance: Provenance::BuiltFromSource {
                repo: "https://github.com/souffle-lang/souffle".to_owned(),
                reference: SOUFFLE_VERSION.to_owned(),
                script: "opponents/souffle/build.sh".to_owned(),
            },
        })
    }

    /// Untimed setup, then the argv whose executions get measured.
    ///
    /// Interpreted mode needs no setup. Compiled mode synthesizes and
    /// compiles the standalone executable once (`souffle -o`) — Souffle's
    /// documented deployment flow — so the measured runs pay execution
    /// only, exactly like a user who compiled their analysis once. The
    /// setup itself runs under the same caps as everything else.
    pub fn prepare(
        &self,
        runner: &kyzo_bench_harness::Runner,
        program: &Path,
        fact_dir: &Path,
        out_dir: &Path,
        scratch: &Path,
    ) -> Result<Vec<String>, SouffleError> {
        let data_flags = |bin: String| {
            vec![
                bin,
                "-F".to_owned(),
                fact_dir.display().to_string(),
                "-D".to_owned(),
                out_dir.display().to_string(),
                "-j".to_owned(),
                self.threads.to_string(),
            ]
        };
        match self.mode {
            Mode::Interpreted => {
                let mut argv = data_flags(self.binary.display().to_string());
                argv.push(program.display().to_string());
                Ok(argv)
            }
            Mode::Compiled => {
                let synth = scratch.join("synthesized");
                let compile_argv = vec![
                    self.binary.display().to_string(),
                    "-o".to_owned(),
                    synth.display().to_string(),
                    "-j".to_owned(),
                    self.threads.to_string(),
                    program.display().to_string(),
                ];
                runner
                    .run(
                        &compile_argv,
                        Some(&synth),
                        kyzo_bench_harness::run::Warmth::Cold,
                    )
                    .map_err(|e| SouffleError::CompileFailed(e.to_string()))?;
                Ok(data_flags(synth.display().to_string()))
            }
        }
    }

    /// One sentence for the record's notes field, citing the configuration.
    pub fn notes(&self) -> String {
        let mode = match self.mode {
            Mode::Interpreted => "interpreted mode",
            Mode::Compiled => {
                "compiled mode: synthesized once with `souffle -o` (untimed setup, \
                 Souffle's documented deployment flow), measured runs execute the binary"
            }
        };
        format!(
            "Souffle {SOUFFLE_VERSION} built from its release tag ({}); {mode}, -j {}.",
            "opponents/souffle/build.sh", self.threads
        )
    }
}
