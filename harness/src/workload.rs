use crate::seed::Seed;
use serde::{Deserialize, Serialize};

/// One named unit of comparable work.
///
/// A workload is what both sides run: the same dataset, the same logical
/// query, the same seed. Subjects differ; the workload must not. The
/// `correctness` field is the output hash both sides must agree on — a
/// performance number for a wrong answer is not a result, so the hash rides
/// with the workload identity rather than in prose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workload {
    /// Stable identifier, e.g. "tc/rand-sparse-100k".
    pub id: String,
    /// What this measures, one sentence, for the reader of results/.
    pub description: String,
    pub seed: Seed,
    /// SHA-256 of the canonical answer (sorted output bytes), once known.
    /// `None` only while a rig is under construction; a published record
    /// with `None` here documents that correctness was checked another way.
    pub correctness_sha256: Option<String>,
}
