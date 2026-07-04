use serde::{Deserialize, Serialize};

/// Who produced a measurement: KyzoDB at an exact commit, or an opponent at
/// an exact pinned version. There is no third case and no "version unknown".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Subject {
    Kyzo(EnginePin),
    Opponent(Opponent),
}

impl Subject {
    /// The label used in result filenames and tables.
    pub fn label(&self) -> String {
        match self {
            Subject::Kyzo(p) => format!("kyzo@{}", &p.commit[..12.min(p.commit.len())]),
            Subject::Opponent(o) => format!("{}@{}", o.name, o.version),
        }
    }
}

/// KyzoDB as a benchmark subject: the exact commit this repo's own
/// `Cargo.lock` resolved a `git` dependency on `kyzodb/kyzo` to — pinned the
/// same way `.claude/skills/add-opponent/SKILL.md` gate 2 pins every
/// opponent, never a live sibling working tree. There is no "dirty" case
/// here: a `git` dependency is an immutable, content-addressed checkout
/// Cargo manages in its own cache, so the question "was the engine tree
/// dirty at build time" cannot even be asked. Pre-release only: once the
/// engine cuts a tagged release, `rev` becomes `=X.Y.Z` and this is a
/// released version exactly like every `Opponent`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnginePin {
    pub commit: String,
}

/// KyzoDB as a benchmark subject: the engine repo commit the artifact was
/// built from. Captured with `git -C <engine> rev-parse HEAD` by the rig,
/// never typed by hand.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineCommit {
    pub commit: String,
    /// `true` when the engine working tree had uncommitted changes at build
    /// time; such runs are for development only and must not be published.
    pub dirty: bool,
}

impl EngineCommit {
    pub fn capture(engine_repo: &std::path::Path) -> Option<EngineCommit> {
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .arg("-C")
                .arg(engine_repo)
                .args(args)
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        };
        let commit = git(&["rev-parse", "HEAD"])?;
        let dirty = !git(&["status", "--porcelain"])?.is_empty();
        Some(EngineCommit { commit, dirty })
    }

    /// The prose fragment every rig's notes append when the engine tree was
    /// dirty at capture time — one implementation instead of a copy-pasted
    /// `if commit.dirty { ... } else { "" }` per rig.
    pub fn dirty_suffix(&self) -> &'static str {
        if self.dirty {
            " (DIRTY TREE — not publishable)"
        } else {
            ""
        }
    }
}

/// Failure locating the engine's `Cargo.lock` pin or its built runner
/// binary — the one lookup every `kyzo`-subject rig arm needs.
#[derive(Debug, thiserror::Error)]
pub enum EngineLocateError {
    #[error("could not read or parse {0}: {1}")]
    LockfileUnreadable(std::path::PathBuf, String),
    #[error(
        "no `kyzo` package in {0}, or it is not pinned via a `git` dependency \
         (found a path or registry source instead); every kyzo-runner Cargo.toml \
         must depend on it as `kyzo = {{ git = \"https://github.com/kyzodb/kyzo\", \
         rev = \"...\" }}` (or, post-release, `kyzo = \"=X.Y.Z\"`)"
    )]
    NotPinned(std::path::PathBuf),
    #[error("{0} not built: run `cargo build --release -p {1}`")]
    RunnerNotBuilt(std::path::PathBuf, &'static str),
}

#[derive(Deserialize)]
struct CargoLock {
    #[serde(default, rename = "package")]
    packages: Vec<LockedPackage>,
}

#[derive(Deserialize)]
struct LockedPackage {
    name: String,
    source: Option<String>,
}

/// Read `Cargo.lock`, find the resolved `kyzo` package, and verify
/// `target/release/<runner_binary>` exists — exactly the "kyzo" arm every
/// `prepare_subject` used to hand-roll, once.
///
/// The commit comes from `Cargo.lock`, never from a neighboring checkout's
/// `git status`: this repo's own `kyzo = { git = "...", rev = "..." }`
/// dependency (or, post-release, `kyzo = "=X.Y.Z"`) is what Cargo actually
/// built, and `Cargo.lock` is Cargo's own record of exactly which commit
/// (or version) that resolved to — `source = "git+https://.../kyzo?rev=...#<sha>"`
/// names the full 40-char sha after the `#`. A build can never observe
/// someone else's half-finished edit in a sibling working tree, because
/// there is no sibling working tree in this path at all.
pub fn locate_kyzo(
    root: &std::path::Path,
    runner_binary: &'static str,
) -> Result<(EnginePin, std::path::PathBuf), EngineLocateError> {
    let lockfile = root.join("Cargo.lock");
    let text = std::fs::read_to_string(&lockfile)
        .map_err(|e| EngineLocateError::LockfileUnreadable(lockfile.clone(), e.to_string()))?;
    let commit = engine_commit_from_lockfile(&lockfile, &text)?;
    let bin = root.join("target/release").join(runner_binary);
    if !bin.is_file() {
        return Err(EngineLocateError::RunnerNotBuilt(bin, runner_binary));
    }
    Ok((EnginePin { commit }, bin))
}

/// The lockfile-parsing half of [`locate_kyzo`], split out so it can be
/// exercised against synthetic `Cargo.lock` text without a real build tree.
fn engine_commit_from_lockfile(
    lockfile: &std::path::Path,
    text: &str,
) -> Result<String, EngineLocateError> {
    let lock: CargoLock = toml::from_str(text)
        .map_err(|e| EngineLocateError::LockfileUnreadable(lockfile.to_owned(), e.to_string()))?;
    lock.packages
        .iter()
        .find(|p| p.name == "kyzo")
        .and_then(|p| p.source.as_deref())
        .and_then(|s| s.rsplit_once('#'))
        .map(|(_, sha)| sha.to_owned())
        .filter(|sha| sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| EngineLocateError::NotPinned(lockfile.to_owned()))
}

/// An opponent engine, pinned. `provenance` records how the exact artifact
/// was obtained so a stranger can reconstruct it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Opponent {
    pub name: String,
    /// Exact released version, e.g. "2.5" — never "latest".
    pub version: String,
    pub provenance: Provenance,
}

/// How an opponent artifact came to exist on this machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Provenance {
    /// Built from source at an exact tag/commit by a script in this repo.
    BuiltFromSource {
        repo: String,
        reference: String,
        script: String,
    },
    /// A pinned container image, by digest when available.
    ContainerImage { image: String },
    /// A pinned package from a language ecosystem (pip, cargo, npm).
    Package {
        ecosystem: String,
        package: String,
        version: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_stable() {
        let s = Subject::Opponent(Opponent {
            name: "souffle".into(),
            version: "2.5".into(),
            provenance: Provenance::BuiltFromSource {
                repo: "https://github.com/souffle-lang/souffle".into(),
                reference: "2.5".into(),
                script: "opponents/souffle/build.sh".into(),
            },
        });
        assert_eq!(s.label(), "souffle@2.5");
    }

    #[test]
    fn engine_commit_from_lockfile_reads_the_git_pin() {
        let lock = r#"
[[package]]
name = "kyzo"
version = "0.1.0"
source = "git+https://github.com/kyzodb/kyzo?rev=d2436cd8a0e142ac3b24b7bd7385aa649effb24e#d2436cd8a0e142ac3b24b7bd7385aa649effb24e"
dependencies = []
"#;
        let commit =
            engine_commit_from_lockfile(std::path::Path::new("Cargo.lock"), lock).unwrap();
        assert_eq!(commit, "d2436cd8a0e142ac3b24b7bd7385aa649effb24e");
    }

    #[test]
    fn engine_commit_from_lockfile_refuses_a_path_dependency() {
        // No `source` field at all — Cargo's own signature for a local path
        // dependency. The whole point of the git pin is that this can never
        // silently resolve to a sibling working tree again.
        let lock = r#"
[[package]]
name = "kyzo"
version = "0.1.0"
dependencies = []
"#;
        let err =
            engine_commit_from_lockfile(std::path::Path::new("Cargo.lock"), lock).unwrap_err();
        assert!(matches!(err, EngineLocateError::NotPinned(_)), "got: {err}");
    }

    #[test]
    fn engine_commit_from_lockfile_refuses_a_registry_pin_without_a_commit() {
        // Post-release shape (`kyzo = "=0.1.0"` from crates.io) has a
        // `registry+` source with no `#<sha>` fragment — correctly rejected
        // by this function, whose whole job is resolving a pre-release git
        // pin. The post-release path reads the version straight off
        // `Opponent`/`Subject`, not through `locate_kyzo`.
        let lock = r#"
[[package]]
name = "kyzo"
version = "0.1.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
dependencies = []
"#;
        let err =
            engine_commit_from_lockfile(std::path::Path::new("Cargo.lock"), lock).unwrap_err();
        assert!(matches!(err, EngineLocateError::NotPinned(_)), "got: {err}");
    }

    #[test]
    fn engine_commit_from_lockfile_reports_missing_package() {
        let err = engine_commit_from_lockfile(std::path::Path::new("Cargo.lock"), "").unwrap_err();
        assert!(matches!(err, EngineLocateError::NotPinned(_)), "got: {err}");
    }
}
