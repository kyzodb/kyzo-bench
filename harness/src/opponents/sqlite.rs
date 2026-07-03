//! The SQLite opponent adapter, shared by every bench that compares against
//! it (`oltp-rig`'s `"sqlite"`, `fts-rig`'s `"fts5"`). SQLite gets its best
//! game once, pinned by `opponents/sqlite/build.sh`; each bench only differs
//! in the subject *name* it records, since the same binary competes under a
//! different label depending on which of its features is under test.

use crate::subject::{Opponent, Provenance, Subject};
use std::path::{Path, PathBuf};

pub const SQLITE_VERSION: &str = "3.53.3";

#[derive(Debug, thiserror::Error)]
pub enum SqliteError {
    #[error("sqlite3 opponent not built at {0}; run opponents/sqlite/build.sh")]
    NotBuilt(PathBuf),
}

/// Verify the pinned SQLite binary is built and return its subject identity
/// (recorded under `name`, e.g. `"sqlite"` for the OLTP bench or
/// `"sqlite-fts5"` for the FTS bench) plus the binary path to invoke.
pub fn sqlite_subject(root: &Path, name: &str) -> Result<(Subject, PathBuf), SqliteError> {
    let bin = root.join("opponents/sqlite/dist/bin/sqlite3");
    if !bin.is_file() {
        return Err(SqliteError::NotBuilt(bin));
    }
    let subject = Subject::Opponent(Opponent {
        name: name.to_owned(),
        version: SQLITE_VERSION.to_owned(),
        provenance: Provenance::BuiltFromSource {
            repo: "https://www.sqlite.org/2026/sqlite-autoconf-3530300.tar.gz".to_owned(),
            reference: SQLITE_VERSION.to_owned(),
            script: "opponents/sqlite/build.sh".to_owned(),
        },
    });
    Ok((subject, bin))
}
