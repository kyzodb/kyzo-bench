use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

/// The identity of the exact bytes a run consumed.
///
/// A dataset is named by content, not by filename: this digest is computed
/// from the file itself by [`DatasetDigest::of_file`], so a result in
/// `results/` can prove which bytes it ran against and a re-fetch can prove
/// it got the same ones. No constructor accepts a bare hex string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatasetDigest {
    /// Path relative to the repo root (or `datasets/`), for the reader.
    pub name: String,
    /// SHA-256 of the file's bytes, lowercase hex.
    pub sha256: String,
    pub bytes: u64,
}

impl DatasetDigest {
    pub fn of_file(name: &str, path: &Path) -> io::Result<DatasetDigest> {
        let mut f = std::fs::File::open(path)?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 1 << 20];
        let mut total: u64 = 0;
        loop {
            let n = f.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            total += n as u64;
        }
        Ok(DatasetDigest {
            name: name.to_owned(),
            sha256: hex(&hasher.finalize()),
            bytes: total,
        })
    }

    /// Digest every regular file under a directory, sorted by relative path,
    /// as one digest list. Directories are how multi-file fact sets ship.
    pub fn of_dir(dir: &Path) -> io::Result<Vec<DatasetDigest>> {
        let mut files: Vec<PathBuf> = Vec::new();
        walk(dir, &mut files)?;
        files.sort();
        files
            .iter()
            .map(|p| {
                let rel = p.strip_prefix(dir).unwrap_or(p).display().to_string();
                DatasetDigest::of_file(&rel, p)
            })
            .collect()
    }
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            walk(&path, out)?;
        } else {
            out.push(path);
        }
    }
    Ok(())
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn digest_is_content_addressed() {
        let dir = std::env::temp_dir().join(format!("kb-digest-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.facts");
        let mut f = std::fs::File::create(&p).unwrap();
        f.write_all(b"1\t2\n2\t3\n").unwrap();
        drop(f);
        let d = DatasetDigest::of_file("a.facts", &p).unwrap();
        // Independently verifiable: echo -ne '1\t2\n2\t3\n' | sha256sum
        assert_eq!(d.bytes, 8);
        assert_eq!(d.sha256.len(), 64);
        let d2 = DatasetDigest::of_file("a.facts", &p).unwrap();
        assert_eq!(d, d2);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
