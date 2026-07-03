use serde::{Deserialize, Serialize};

/// Who produced a measurement: KyzoDB at an exact commit, or an opponent at
/// an exact pinned version. There is no third case and no "version unknown".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Subject {
    Kyzo(EngineCommit),
    Opponent(Opponent),
}

impl Subject {
    /// The label used in result filenames and tables.
    pub fn label(&self) -> String {
        match self {
            Subject::Kyzo(c) => format!("kyzo@{}", &c.commit[..12.min(c.commit.len())]),
            Subject::Opponent(o) => format!("{}@{}", o.name, o.version),
        }
    }
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
}
