//! Reader for the vector bench's `flat/` directory format (kyzo#25):
//! `shape.txt` (`n dim q k_truth`, whitespace-separated), `train.f32` /
//! `test.f32` (little-endian `f32`, row-major), `neighbors.i64`
//! (little-endian `i64`, row-major, `q * k_truth` entries — the dataset's
//! exact ground truth). `export-flat.py` writes it once from the
//! ann-benchmarks HDF5 source; every subject that reads vectors as raw
//! arrays (`kyzo-vector-runner`, `surrealdb-runner`) reads this, once,
//! here — the alternative is each subject's own HDF5-adjacent parser
//! drifting out of sync with what the other subjects actually got tested
//! against.

use std::io::Read;
use std::path::Path;

pub struct FlatVectors {
    pub n: usize,
    pub dim: usize,
    pub q: usize,
    pub k_truth: usize,
    pub train: Vec<f32>,
    pub test: Vec<f32>,
    pub neighbors: Vec<i64>,
}

impl FlatVectors {
    pub fn read(dir: &Path) -> Result<FlatVectors, Box<dyn std::error::Error>> {
        let shape = std::fs::read_to_string(dir.join("shape.txt"))?;
        let dims: Vec<usize> = shape
            .split_whitespace()
            .map(|s| s.parse())
            .collect::<Result<_, _>>()?;
        let [n, dim, q, k_truth] = dims[..] else {
            return Err(format!("shape.txt has {} fields, want 4", dims.len()).into());
        };
        let read_f32 = |name: &str, len: usize| -> Result<Vec<f32>, Box<dyn std::error::Error>> {
            let mut bytes = Vec::with_capacity(len * 4);
            std::fs::File::open(dir.join(name))?.read_to_end(&mut bytes)?;
            if bytes.len() != len * 4 {
                return Err(format!("{name}: {} bytes, want {}", bytes.len(), len * 4).into());
            }
            Ok(bytes
                .chunks_exact(4)
                .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect())
        };
        let mut nb = Vec::with_capacity(q * k_truth * 8);
        std::fs::File::open(dir.join("neighbors.i64"))?.read_to_end(&mut nb)?;
        if nb.len() != q * k_truth * 8 {
            return Err(format!("neighbors.i64: {} bytes, want {}", nb.len(), q * k_truth * 8).into());
        }
        Ok(FlatVectors {
            n,
            dim,
            q,
            k_truth,
            train: read_f32("train.f32", n * dim)?,
            test: read_f32("test.f32", q * dim)?,
            neighbors: nb
                .chunks_exact(8)
                .map(|c| i64::from_le_bytes(c.try_into().expect("chunk of 8")))
                .collect(),
        })
    }
}
