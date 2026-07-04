//! The measurement kernel for kyzo-bench.
//!
//! Every number this repo publishes flows through these types, and the types
//! are the methodology: a [`ResultRecord`] cannot be constructed without the
//! seed, the captured hardware, the pinned subject, the dataset digests, and
//! at least one real [`Measurement`] — and a `Measurement` can only be minted
//! by [`Runner::run`], which actually executed the command under caps and
//! hashed its output. There is no `new` that takes a bare number; a claim you
//! cannot construct is a claim you cannot publish.
//!
//! The append-only law of `results/` is enforced at the same level:
//! [`ResultRecord::land`] refuses to overwrite an existing file.

pub mod canon;
pub mod dataset;
pub mod envelope;
pub mod flat_vectors;
pub mod hardware;
pub mod opponents;
pub mod record;
pub mod rig;
pub mod run;
pub mod seed;
pub mod subject;
pub mod workload;

pub use canon::{CanonicalAnswer, canonical_answer, raw_answer};
pub use dataset::DatasetDigest;
pub use envelope::EnvelopeError;
pub use flat_vectors::FlatVectors;
pub use hardware::Hardware;
pub use record::{LandError, ResultRecord};
pub use rig::{AnswerSpec, HashKind, Phase, PreparedSubject, Rig, RigError};
pub use run::{CapPolicy, Measurement, RunError, RunSet, Runner};
pub use seed::{Seed, SplitMix64};
pub use subject::{
    EngineCommit, EngineLocateError, EnginePin, Opponent, Provenance, Subject, locate_kyzo,
};
pub use workload::Workload;
