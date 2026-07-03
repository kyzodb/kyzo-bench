//! Canonical answers. An answer set is a set; the bytes an engine happens
//! to emit are an ordering accident. The canonical form — unique lines,
//! byte-sorted, newline-joined — is what two subjects must agree on, and
//! what repeated runs of one subject must reproduce exactly.

use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::io::{BufRead, BufReader};
use std::path::Path;

/// The canonical identity of an answer set: its size and the SHA-256 of its
/// sorted unique lines. Minted only by [`canonical_answer`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalAnswer {
    pub rows: usize,
    pub sha256: String,
}

pub fn canonical_answer(output_file: &Path) -> std::io::Result<CanonicalAnswer> {
    let reader = BufReader::new(std::fs::File::open(output_file)?);
    let mut lines: BTreeSet<Vec<u8>> = BTreeSet::new();
    for line in reader.split(b'\n') {
        let line = line?;
        if !line.is_empty() {
            lines.insert(line);
        }
    }
    let mut hasher = Sha256::new();
    for line in &lines {
        hasher.update(line);
        hasher.update(b"\n");
    }
    Ok(CanonicalAnswer {
        rows: lines.len(),
        sha256: hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn order_and_duplicates_do_not_change_the_answer() {
        let dir = std::env::temp_dir().join(format!("kb-canon-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a.csv");
        let b = dir.join("b.csv");
        std::fs::File::create(&a)
            .unwrap()
            .write_all(b"2\t3\n1\t2\n")
            .unwrap();
        std::fs::File::create(&b)
            .unwrap()
            .write_all(b"1\t2\n2\t3\n2\t3\n")
            .unwrap();
        let ca = canonical_answer(&a).unwrap();
        let cb = canonical_answer(&b).unwrap();
        assert_eq!(ca.sha256, cb.sha256);
        assert_eq!(ca.rows, 2);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn different_answers_have_different_identities() {
        let dir = std::env::temp_dir().join(format!("kb-canon2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a.csv");
        let b = dir.join("b.csv");
        std::fs::File::create(&a)
            .unwrap()
            .write_all(b"1\t2\n")
            .unwrap();
        std::fs::File::create(&b)
            .unwrap()
            .write_all(b"1\t3\n")
            .unwrap();
        assert_ne!(
            canonical_answer(&a).unwrap().sha256,
            canonical_answer(&b).unwrap().sha256
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
