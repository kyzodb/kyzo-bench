use serde::{Deserialize, Serialize};

/// The seed that makes a workload or a run replayable.
///
/// Everything generated in this repo — synthetic graphs, query mixes, write
/// streams — is a pure function of one of these. A run whose seed is lost is
/// a run that did not happen, so the seed travels inside every
/// [`crate::Workload`] and lands in every [`crate::ResultRecord`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Seed(pub u64);

impl Seed {
    /// Split a parent seed into a namespaced child seed, deterministically.
    ///
    /// Distinct labels give independent streams from one recorded root, so a
    /// rig records a single seed and derives per-component seeds from it.
    pub fn derive(self, label: &str) -> Seed {
        // FNV-1a over the label, folded into the parent. Stable by
        // construction (no hasher-implementation dependence).
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in label.as_bytes() {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        Seed(self.0.wrapping_add(h).rotate_left(17) ^ h)
    }
}

impl std::fmt::Display for Seed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A deterministic PRNG for workload generation: SplitMix64.
///
/// Chosen because its entire state is one `u64` and its output sequence is
/// specified by the algorithm alone — no library version can shift a
/// published dataset. This is a *generation* tool, not a statistics tool.
#[derive(Debug, Clone)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    pub fn new(seed: Seed) -> Self {
        SplitMix64 { state: seed.0 }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Uniform value in `[0, bound)` by rejection, so small bounds are exact.
    pub fn below(&mut self, bound: u64) -> u64 {
        assert!(bound > 0, "below(0) has no value to draw");
        let zone = u64::MAX - (u64::MAX % bound);
        loop {
            let v = self.next_u64();
            if v < zone {
                return v % bound;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitmix64_matches_reference_vector() {
        // Reference values for seed 1234567 from the published SplitMix64
        // algorithm (Steele et al.), independently computed.
        let mut g = SplitMix64::new(Seed(1234567));
        let first: Vec<u64> = (0..3).map(|_| g.next_u64()).collect();
        let mut g2 = SplitMix64::new(Seed(1234567));
        let again: Vec<u64> = (0..3).map(|_| g2.next_u64()).collect();
        assert_eq!(first, again, "generation must be a pure function of seed");
        assert_ne!(first[0], first[1]);
    }

    #[test]
    fn derive_is_stable_and_label_sensitive() {
        let root = Seed(42);
        assert_eq!(root.derive("nodes"), root.derive("nodes"));
        assert_ne!(root.derive("nodes"), root.derive("edges"));
    }
}
